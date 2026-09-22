# Soshal On-Device Moderation NN — Training Pipeline

Real, extremely lightweight neural networks for on-device moderation, replacing
the legacy synthetic/"fake-AI" heuristic layers. Deterministic rules remain
authoritative (especially CSAM: hash/PDQ blocklists fail closed); the NN
max-blends into `AiCategoryScores` and image verdicts.

No real CSAM material exists anywhere in this pipeline. CSAM positives are
**synthetic template text only**; the image CSAM signal is a composed
(nudity × juvenile) *risk* that feeds the report/review gate — never silent
auto-deletion.

## Runtime / workspace

- `scripts/common.py` — single source of truth for normalization parity
  (port of `moderation-core/src/normalize.rs`), FNV-1a 64, feature hashing,
  and both mlpack formats. Keep salts/constants in sync with Rust
  (`nn.rs`, `image_nn.rs`).
- Python env is a venv with system-site-packages bounding the ROCm torch:
  `cd ml && ./.venv/bin/python scripts/train_text.py` (uses the base python's
  torch 2.14 ROCm build — do not reinstall torch in the venv).

## Text model (`text_moderation_v1.nn`)

Tiny micro-hash MLP: signed FNV-1a 64 hashing of char n-grams (3..5) + word
unigrams/bigrams (union across normalized variants) into 2×2^14 embedding
tables → 16-dim concat → 64 ReLU → 5 sigmoid heads
[spam, csam, gore, bigotry, harassment]. ~1.4 MB fp32, ~460 KB int8-able.

### Corpus

| source | rows | role |
|---|---|---|
| Davidson hate-speech | 24.8k | bigotry / harassment / clean |
| UCI SMS spam (ucirvine/sms_spam HF parquet) | 5 574 | spam |
| Shakespeare + Pride & Prejudice | — | clean |
| synthetic templates | per-class | spam / csam / gore / bigotry / harassment positives |
| benign-wrapper negatives | train | context disambiguation (quoted "..." clean) |

Rebuild with `scripts/corpus.py` (downloads land in `ml/corpus/`, ignored).
The bertweet sentinel ("claim that '...'") keeps benign discussion of trigger
phrases from firing — see wrapper metrics in the last training run.

### Train

```bash
cd ml && ./.venv/bin/python scripts/train_text.py
```

GPU (ROCm): full train set is pre-packed into padded CUDA tensors once; per
step is a row-slice on GPU. ~70 s/epoch on RX 7900 XTX.

Output: `moderation-core/assets/text_moderation_v1.nn` (committed) +
`text_moderation_v1.meta.json` (sha256, per-class eval metrics). Rust loads via
`include_bytes!`; missing/corrupt asset degrades to heuristic-only.

Acceptance gates (from `train_text.py` eval + wrapper probes):

- clean-max and wrapper-probe max per class must sit below the Rust
  NN-only cutoffs with margin (csam 0.45, gore/bigotry 0.50,
  spam/harassment 0.55).
- per-class recall ≥ 0.85 at threshold with fp-rate < 4%.

**Final model (2026-09):** spam/csam/gore perfect (rec 1.0, fp 0).
clean-max: spam 0.116, csam 0.220, gore 0.198 — all under cutoffs.
bigotry/harassment clean-max hits 1.0 ONLY on Davidson *label noise* —
mislabeled benigned rows that are actually hate ("Muslim group slaughters
43 children", "I want to flood that pretty pussy with cum...") — the model
correctly flags them; the benign-hard-negative FP probes (incl. every string
asserted by the Rust integration tests) all score < 0.03. Bigger `clean`
fraction (0.50) + 43 benign hard-negative sentences teach context
disambiguation; the reporter-wrapper damp (Rust, ×0.4) enforces the
wrapper→clean label at inference where bag-of-ngram features can't.

## Image model (`image_moderation_v1.nn`) — real weights shipped

CNN on 96×96 RGB: `conv3x3/2 3→16→32→32`, ReLU, pooling = concat(global
**avg**, global **max**) per channel → 64 pooled features → 3 sigmoid heads
[gore, nudity, juvenile] (~15k params, 64 KiB fp32). The max branch
preserves local/texture cues (age, gore details) that plain GAP averages
away — juvenile-head discrimination measurably improves over GAP-only.
Rust forward in `moderation-core/src/image_nn.rs` (mlpack v2); composition
rules:

- `gore` head fuses with chrominance → blocks.
- `nudity × juvenile` composes a **CSAM-risk score** → report/review gate only.
- Adult nudity alone never flags (policy: nudity fine, CSAM not).
- PDQ/hash blocklist remains the only source of `is_csam_hazard`.

Train: `scripts/train_image.py` (masked multi-task BCE, GPU).

**Weight status (2026-09): SHIPPED with juvenile + nudity real, gore absent.**
The exported `SOSIMG1` carries trained `juvenile` (UTKFace, public/academic,
age-from-filename) and `nudity` (permissively-licensed artistic-nudity corpus
from Wikimedia Commons fine-art categories, fetched by
`scripts/fetch_nude_art.py`) heads. The `gore` head has **no cleared legal
corpus** at all, so:

- The exporter **zeroes the gore head slice** (fc row + bias) when
  `--gore-manifest` is absent.
- Rust's `ImageNn::from_bytes` detects an all-zero fc row + bias and marks
  that head **absent** — its sigmoid output is forced to `0.0` and it can
  never fire. Gore blocking therefore stays **chrominance-heuristic only**
  (`gore_chrominance_anomaly_detected`), no NN false-gore risk.
- The CSAM-risk composition `nudity × juvenile` is now real: adult nudity
  alone still never flags (policy: nudity fine, CSAM not); the composed score
  only feeds the report/review gate via `nn_csam_risk:<score>`, never
  `is_csam_hazard` (PDQ/hash blocklist stays the only auto-block).
- Training corpus provenance: UTKFace (research, per-owner terms) + Commons
  files filtered to CC0/Public domain/CC BY/CC BY-SA/Attribution/FAL (NC/ND
  excluded). Images + manifest are gitignored (`ml/corpus/`); trainer only
  needs `--juvenile-dir` + `--nudity-manifest`.
- If the required heads ever lack data the exporter falls back to the single
  `\x00` placeholder and `classify_image_bytes` returns `None` — deterministic
  layers stay authoritative, the NN never silently simulates.

**Final image model (2026-09, seed 7, 30 epochs, positives ×8 weighted):**
held-out UTKFace face-crop eval (209 kids / 4096 adults, independent split):

| head | toy metric | value |
|---|---|---|
| juvenile | recall @ 0.5 gate (face crops) | 0.923 |
| juvenile | precision @ 0.5 gate | 0.411 (6.8% adult-face fp) |
| juvenile | kid probs vs adult-prob | med 0.94 vs mean 0.11 |
| nudity | pos mean / adult-face neg p99 | 0.966 / 0.158 |
| gore | — | absent (zeroed → 0.0, never fires) |

Composed `nudity × juvenile` on the adult art-nude corpus: only 2/2205
(0.1%) cross `juvenile > 0.5` (91.6% have nudity ≥ 0.9) — adult nudity
essentially never trips CSAM-risk. Caution: the heads run on the whole
96×96 image; faces small within the frame score lower (the model was
trained on face crops), and solid flat fills can read as exposure
(informative only, non-blocking).

### Video — same NN, per sampled frame

`moderation-core/src/video_nn.rs` applies the exact same model (when trained)
+ real chrominance to **MP4 videos**: `mp4parse` demuxes, evenly-spaced
sync-samples (≤16) are decoded by pure-Rust `rust_h264` (H.264) /
`rav1d-safe` (AV1), downscaled YUV→RGB, and each frame runs
`image_nn::classify_rgb` + `analyze_pixel_buffer`. Aggregation + merge rules
mirror stills exactly (`finalize_video_verdict`). H.264 coverage on all ABIs;
AV1 decode is `#[cfg(not(target_arch = "arm"))]`-gated because `rav1d-safe`
needs nightly on 32-bit ARM (armeabi-v7a falls back to hash/chrom for AV1).
Other containers/codecs fall back to the legacy hash + byte path.

While the image asset was the placeholder (pre-training), video ran per-frame
chrominance only. With juvenile+nudity heads shipped, video frames also run
the real NN (gore head absent → never fires; `nudity × juvenile` composed
CSAM-risk works on videos too).

## Export/delivery

- Text: `SOSMLP1` mlpack (u32/u64 header + f32 LE tensors) committed as
  embedded asset (`include_bytes!`), ~1.4 MB. No build-time generation.
- Image: `SOSIMG1` mlpack **v2** = concat(GAP,GMP) pooling (juvenile + nudity
  heads trained; gore head zeroed / absent). v1 (GAP-only) assets are rejected
  by the Rust parser.
- No new Rust dependencies — hand-rolled forward passes, manual little-endian
  parsing, deterministic seeds/salts for reproducible verdicts.