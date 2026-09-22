#!/usr/bin/env python3
"""Train the Soshal on-device image moderation CNN (gore / nudity / juvenile).

Architecture must match moderation-core/src/image_nn.rs EXACTLY:
    conv3x3/2: 3 -> 16  (48x48) ReLU
    conv3x3/2: 16 -> 32 (24x24) ReLU
    conv3x3/2: 32 -> 32 (12x12) ReLU
    concat(GAP, GMP) -> 64 -> fc -> 3 sigmoid heads  [gore, nudity, juvenile]

Pooling concatenates per-channel global mean AND max: the max branch
preserves local/texture cues (age, gore details) that plain GAP averages
away — measurably better juvenile-head discrimination (see ml/README.md).

Data inputs (all legal/public — no CSAM material is ever part of this pipeline):
    --juvenile-dir   UTKFace aligned_cropped dir (filename "age_gender_race_..."
                     encodes age). juvenile = age < 18. Public/academic dataset.
    --nudity-manifest CSV: <path>,<label 0/1>  — user-curated, cleared-for-use
                     adult nudity corpus. REQUIRED for the CSAM-risk composition
                     (nudity x juvenile). Until a data owner provides this, the
                     nudity head stays zero and the composed risk never fires.
    --gore-manifest  CSV: <path>,<label 0/1>  — OPTIONAL, cleared-for-use gore
                     corpus. No legal public gore corpus is bundled; when absent
                     the gore head is exported all-zero (Rust treats it as
                     absent and chrominance heuristics stay authoritative for
                     gore blocks).

The CSAM-risk composition (nudity x juvenile) needs both heads real, so the
exporter refuses to ship unless juvenile + nudity have cleared data. When a
head is absent its export slice is zeroed and its BCE term is masked out.
Missing everything → the asset is a single 0x00 placeholder byte (Rust
`image_nn_available()` returns false) until required heads are cleared.
Training on GPU when available.
"""

from __future__ import annotations

import argparse
import glob
import os
import struct
import sys
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn

INPUT = 96
N_HEADS = 3  # gore, nudity, juvenile
ASSET = Path(__file__).resolve().parent.parent.parent / "moderation-core" / "assets"


class Cnn(nn.Module):
    def __init__(self) -> None:
        super().__init__()
        self.conv1 = nn.Conv2d(3, 16, 3, stride=2)
        self.conv2 = nn.Conv2d(16, 32, 3, stride=2)
        self.conv3 = nn.Conv2d(32, 32, 3, stride=2)
        self.fc = nn.Linear(2 * 32, N_HEADS)
        self._init()

    def _init(self) -> None:
        nn.init.kaiming_normal_(self.conv1.weight, nonlinearity="relu")
        nn.init.kaiming_normal_(self.conv2.weight, nonlinearity="relu")
        nn.init.kaiming_normal_(self.conv3.weight, nonlinearity="relu")
        for m in (self.conv1, self.conv2, self.conv3, self.fc):
            nn.init.zeros_(m.bias)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = torch.relu(self.conv1(x))
        x = torch.relu(self.conv2(x))
        x = torch.relu(self.conv3(x))
        gavg = x.mean(dim=(2, 3))
        gmax = x.amax(dim=(2, 3))
        return self.fc(torch.cat([gavg, gmax], dim=1))  # raw logits


def load_image(path: str) -> np.ndarray | None:
    try:
        from PIL import Image  # type: ignore
    except ImportError:
        # fall back to torchvision-free torch.io? keep simple: require PIL
        return None
    try:
        with Image.open(path) as im:
            im = im.convert("RGB").resize((INPUT, INPUT), Image.BILINEAR)
            return (np.asarray(im, dtype=np.float32) / 255.0).transpose(2, 0, 1)
    except Exception:
        return None


def utkface_rows(juvenile_dir: str) -> list[tuple[np.ndarray, np.ndarray]]:
    """UTKFace: filename encodes age; juvenile = age in [5, 18)."""
    rows: list[tuple[np.ndarray, np.ndarray]] = []
    for p in glob.glob(os.path.join(juvenile_dir, "*.jpg")):
        name = os.path.basename(p)
        try:
            age = int(name.split("_")[0])
        except ValueError:
            continue
        if age < 5 or age > 80:  # skip newborns / very old (noise)
            continue
        img = load_image(p)
        if img is None:
            continue
        y = np.zeros(N_HEADS, dtype=np.float32)
        if age < 18:
            y[2] = 1.0
        rows.append((img, y))
    return rows


def manifest_rows(manifest: str, head_idx: int) -> list[tuple[np.ndarray, np.ndarray]]:
    rows: list[tuple[np.ndarray, np.ndarray]] = []
    for line in Path(manifest).read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.rsplit(",", 1)
        if len(parts) != 2:
            continue
        path, label = parts
        img = load_image(path)
        if img is None:
            continue
        y = np.zeros(N_HEADS, dtype=np.float32)
        y[head_idx] = 1.0 if label.strip()[0] in "1yY" else 0.0
        rows.append((img, y))
    return rows


def export(model: nn.Module, out: Path, heads_present: set[str]) -> None:
    sd = model.state_dict()
    c1, c2, c3 = 16, 32, 32
    w1 = sd["conv1.weight"].permute(1, 2, 3, 0).reshape(-1).cpu().numpy().astype("<f4")
    b1 = sd["conv1.bias"].cpu().numpy().astype("<f4")
    w2 = sd["conv2.weight"].permute(1, 2, 3, 0).reshape(-1).cpu().numpy().astype("<f4")
    b2 = sd["conv2.bias"].cpu().numpy().astype("<f4")
    w3 = sd["conv3.weight"].permute(1, 2, 3, 0).reshape(-1).cpu().numpy().astype("<f4")
    b3 = sd["conv3.bias"].cpu().numpy().astype("<f4")
    wh = sd["fc.weight"].T.reshape(-1).cpu().numpy().astype("<f4")
    bh = sd["fc.bias"].cpu().numpy().astype("<f4")
    assert w1.shape[0] == c1 * 27 and w2.shape[0] == c2 * 9 * c1 and w3.shape[0] == c3 * 9 * c2
    assert wh.shape[0] == 2 * c3 * N_HEADS
    # fc layout is [2*c3, N_HEADS] flattened (head k at wh[k::N_HEADS]):
    # slots 2c = avg(ch c), 2c+1 = max(ch c) — mirrors concat(GAP, GMP).
    # Any head without cleared data is exported all-zero so Rust's all-zero
    # row + bias check marks it absent → its sigmoid output is forced to 0.0
    # and it can never fire a moderation signal.
    for name, idx in (("gore", 0), ("nudity", 1), ("juvenile", 2)):
        if name not in heads_present:
            wh[idx::N_HEADS] = 0.0
            bh[idx] = 0.0
            print(f"head {name}: absent → exported zeroed (Rust treats as off)")
    with open(out, "wb") as f:
        f.write(b"SOSIMG1")
        f.write(struct.pack("<B", 2))  # v2 = concat(GAP, GMP) pooling
        for v in (INPUT, 3, c1, c2, c3, N_HEADS):
            f.write(struct.pack("<I", v))
        for arr in (w1, b1, w2, b2, w3, b3, wh, bh):
            f.write(arr.tobytes())
    print(f"wrote {out} ({out.stat().st_size} bytes)")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--juvenile-dir")
    ap.add_argument("--nudity-manifest")
    ap.add_argument("--gore-manifest")
    ap.add_argument("--epochs", type=int, default=12)
    ap.add_argument("--batch", type=int, default=64)
    ap.add_argument("--out", default=str(ASSET / "image_moderation_v1.nn"))
    ap.add_argument("--export", dest="export", action="store_true", default=True)
    args = ap.parse_args()

    rows: list[tuple[np.ndarray, np.ndarray]] = []
    if args.juvenile_dir:
        jr = utkface_rows(args.juvenile_dir)
        print(f"juvenile (UTKFace): {len(jr)} rows")
        rows.extend(jr)
    for manifest, head, label in (
        (args.nudity_manifest, 1, "nudity"),
        (args.gore_manifest, 0, "gore"),
    ):
        if manifest:
            mr = manifest_rows(manifest, head)
            print(f"{label}: {len(mr)} rows")
            rows.extend(mr)

    present = [args.juvenile_dir is not None,
               args.nudity_manifest is not None,
               args.gore_manifest is not None]
    heads_present = {n for n, ok in (("juvenile", present[0]),
                                     ("nudity", present[1]),
                                     ("gore", present[2])) if ok}
    print("heads with data:", sorted(heads_present) or "none")

    # The CSAM-risk composition is nudity x juvenile: both heads must have
    # real data before Rust ever emits a composed signal. Gore is optional —
    # when absent it is exported all-zero so Rust treats it as off and the
    # chrominance heuristic remains the sole gore block source.
    required = {"juvenile", "nudity"}
    if not required <= heads_present:
        print("missing heads:", sorted(required - heads_present))
        print("writing placeholder — image NN stays off until juvenile+nudity "
              "have cleared training data.")
        with open(args.out, "wb") as f:
            f.write(b"\x00")
        return

    if not rows:
        print("no usable rows; writing placeholder")
        with open(args.out, "wb") as f:
            f.write(b"\x00")
        return

    X = np.stack([r[0] for r in rows])
    Y = np.stack([r[1] for r in rows])
    print(f"total rows {X.shape[0]}")

    # Mask absent heads' columns so their BCE term is excluded (a head fed
    # only other labels' negatives would just learn to never fire). The
    # `yb >= 0` loss mask below skips these -1 columns.
    HEAD_IDX = {"gore": 0, "nudity": 1, "juvenile": 2}
    for hname in set(HEAD_IDX) - heads_present:
        Y[:, HEAD_IDX[hname]] = -1.0
        print(f"head {hname}: BCE masked out (no data) — exported zeroed")

    # sample weights emphasize rare positive labels present in the data
    # (x8 for positives — without per-sample weighting the rare juvenile
    # positives vanish under the adult-face majority and the head learns to
    # never fire).
    frac = Y.mean(axis=0)
    w = np.ones(Y.shape[0], dtype=np.float32)
    for h in range(N_HEADS):
        col = Y[:, h]
        if col.sum() > 0 and col.sum() < len(col):
            w[col == 1] *= 8.0
    print("head fractions:", np.round(frac, 3))

    dev = torch.device("cuda" if torch.cuda.is_available() else "cpu")
    print("device:", dev)
    torch.manual_seed(7)  # reproducible asset (Kaiming init + shuffles)
    model = Cnn().to(dev)
    Xt = torch.from_numpy(X).to(dev)
    Yt = torch.from_numpy(Y).to(dev)
    Wt = torch.from_numpy(w).to(dev)

    opt = torch.optim.Adam(model.parameters(), lr=1e-3)
    n = Xt.shape[0]
    perm = torch.randperm(n, device=dev)
    n_tr = int(n * 0.9)
    tr_idx, ev_idx = perm[:n_tr], perm[n_tr:]
    bs = args.batch
    for epoch in range(args.epochs):
        model.train()
        tot = 0.0
        cnt = 0
        order = torch.randperm(n_tr, device=dev)
        for s in range(0, n_tr, bs):
            idx = tr_idx[order[s : s + bs]]
            xb, yb = Xt[idx], Yt[idx]
            logits = model(xb)
            # masked BCE: missing heads (-1 equivalent => label 0 w/o signal);
            # per-sample weights emphasize rare positive labels (e.g. juvenile)
            mask = yb >= 0
            loss = torch.nn.functional.binary_cross_entropy_with_logits(
                logits, yb, weight=None, reduction="none")
            loss = ((loss * mask).sum(dim=1) * Wt[idx]).sum() / mask.sum().clamp(min=1)
            opt.zero_grad()
            loss.backward()
            opt.step()
            tot += loss.item()
            cnt += 1
        model.eval()
        with torch.no_grad():
            ev_logits = model(Xt[ev_idx])
            probs = torch.sigmoid(ev_logits)
            evy = Yt[ev_idx]
            accs = []
            for h in range(N_HEADS):
                m = evy[:, h] >= 0
                if m.sum() == 0:
                    accs.append(None)
                    continue
                acc = ((probs[:, h][m] > 0.5).float() == evy[:, h][m]).float().mean()
                accs.append(float(acc))
        print(f"epoch {epoch} loss {tot/cnt:.4f}  eval accs {accs}")

    if args.export:
        export(model, Path(args.out), heads_present)


if __name__ == "__main__":
    main()