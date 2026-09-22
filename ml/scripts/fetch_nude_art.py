#!/usr/bin/env python3
"""Fetch a cleared, permissively-licensed artistic-nudity corpus from
Wikimedia Commons for the image NN `nudity` head (train_image.py
`--nudity-manifest`).

Only fine-art categories are crawled (paintings, sculptures, statues,
drawings, reliefs, figurines — "nude art"), never photographic nude
categories, per the user's decision to use an artistic-nudity source.

License filter: allow CC0 / Public domain / CC BY / CC BY-SA / Attribution
(rejects BY-NC / BY-ND). Training a tiny on-device model from an allowed
license keeps the project's "cleared/legal" invariant.

Output (gitignored by ml/.gitignore's /corpus/ rules):
    ml/corpus/nudity_art/<hash>.jpg    (256px Commons thumbs)
    ml/corpus/nudity_art.csv           manifest rows:  <path>,1
"""

from __future__ import annotations

import argparse
import hashlib
import re
import sys
import time
from pathlib import Path

import requests

# Wikimedia requires a descriptive, contactable User-Agent (bare
# python-requests gets 403).
API = "https://commons.wikimedia.org/w/api.php"
HEADERS = {
    "User-Agent": (
        "SoshalModerationCorpus/0.1 (https://github.com/soshal/open-source; "
        "soshal ml training corpus fetcher)"
    ),
    "Accept": "application/json",
}
SEEDS = [
    "Nude art",
    "Nudes in art",
    "Nude paintings",
    "Nude drawings",
    "Nude sculptures",
    "Nude statues",
    "Nude reliefs",
    "Nude figurines",
]

ALLOWED = re.compile(
    r"^(Public domain|CC0|CC BY|CC BY-SA|Attribution|PD[- ])", re.IGNORECASE
)
FORBIDDEN = re.compile(r"(NC|ND)", re.IGNORECASE)
EXT_RE = re.compile(r"(\.jpe?g|\.png|\.webp|\.tiff?|\.gif)(?:\?|$)", re.IGNORECASE)


def license_ok(short: str) -> bool:
    if not short or not isinstance(short, str):
        return False
    s = short.strip()
    if not FORBIDDEN.search(s) and ALLOWED.search(s):
        return True
    # uncommon but clearly-free variants
    return s in {"No restrictions", "Public domain - USA", "FAL", "Free Art License"}


def api_get(params: dict, timeout: int = 30) -> dict:
    params.setdefault("format", "json")
    r = requests.get(API, params=params, timeout=timeout, headers=HEADERS)
    r.raise_for_status()
    return r.json()


def crawl(*, depth: int, cap: int, delay: float) -> tuple[dict[str, dict], list[str]]:
    """Returns (files: {title: info}, licenses: [title] errors)."""
    seen_cats: set[str] = set()
    files: dict[str, dict] = {}
    skip_missing: list[str] = []
    queue: list[tuple[str, int]] = [(c, 0) for c in SEEDS]
    while queue and len(files) < cap:
        cat, d = queue.pop(0)
        if d > depth or cat in seen_cats:
            continue
        seen_cats.add(cat)
        cont: dict | None = None
        while len(files) < cap:
            params = {
                "action": "query",
                "generator": "categorymembers",
                "gcmtitle": f"Category:{cat}",
                "gcmlimit": "500",
                "gcmtype": "file|subcat",
                "prop": "imageinfo",
                "iiprop": "url|extmetadata",
                "iiurlwidth": "256",
            }
            if cont:
                params.update(cont)
            try:
                data = api_get(params)
            except requests.RequestException as e:
                print(f"  api error on {cat}: {e}", file=sys.stderr)
                time.sleep(2.0)
                break
            pages = (data.get("query") or {}).get("pages") or {}
            for p in pages.values():
                if int(p.get("ns", 0)) == 14:  # subcategory
                    if d + 1 <= depth:
                        queue.append((p["title"][len("Category:"):], d + 1))
                    continue
                ii = (p.get("imageinfo") or [{}])[0]
                lic = (((ii.get("extmetadata") or {}).get("LicenseShortName") or {})
                       .get("value", ""))
                if not license_ok(lic):
                    continue
                title = p["title"]
                url = ii.get("thumburl") or ii.get("url")
                if not url:
                    continue
                files[title] = {"url": url}
            cont = data.get("continue")
            if not cont:
                break
            time.sleep(delay)
        if len(files) < cap:
            time.sleep(delay)
    return files, skip_missing


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out-dir", default="corpus/nudity_art")
    ap.add_argument("--manifest", default="corpus/nudity_art.csv")
    ap.add_argument("--cap", type=int, default=2500)
    ap.add_argument("--depth", type=int, default=2)
    ap.add_argument("--delay", type=float, default=0.12)
    args = ap.parse_args()

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    print(f"crawling {len(SEEDS)} seed categories (depth {args.depth}, cap {args.cap})")
    files, _ = crawl(depth=args.depth, cap=args.cap, delay=args.delay)
    print(f"collected {len(files)} unique allowed-license files")

    rows: list[str] = []
    seen_hashes: set[str] = set()
    skips = 0
    sess = requests.Session()
    sess.headers.update(HEADERS)
    for i, (title, info) in enumerate(files.items()):
        url = info["url"]
        m = EXT_RE.search(url)
        ext = (m.group(1).lower() if m else ".jpg").split("?")[0]
        if ext == ".tiff":
            ext = ".tif"
        try:
            r = sess.get(url, timeout=60)
            if r.status_code != 200 or not r.content:
                skips += 1
                continue
            h = hashlib.sha1(r.content).hexdigest()[:16]
            if h in seen_hashes:
                continue
            seen_hashes.add(h)
            dest = out_dir / f"{h}{ext}"
            dest.write_bytes(r.content)
            rows.append(f"{dest},{1}")
        except requests.RequestException:
            skips += 1
        if i and i % 250 == 0:
            print(f"  {i}/{len(files)} downloaded ({len(rows)} kept, {skips} skipped)")
        time.sleep(args.delay / 3)

    Path(args.manifest).write_text("\n".join(rows) + "\n")
    print(f"wrote {len(rows)} rows to {args.manifest} ({skips} fetch fails)")


if __name__ == "__main__":
    main()