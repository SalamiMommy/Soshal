#!/usr/bin/env python3
"""Train a zstd dictionary over Soshal network payloads (posts, feed JSON).

Offline dev tool only; not shipped. Produces content-core/assets/feed.dict.

Usage:
  ./scripts/train-zstd-dict.py                  # generate synthetic corpus, train
  ./scripts/train-zstd-dict.py --corpus DIR     # train over real JSON samples in DIR
  ./scripts/train-zstd-dict.py --samples 5000   # corpus size (default 2000)
"""
import argparse
import json
import os
import random
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DICT_OUT = os.path.join(ROOT, "content-core", "assets", "feed.dict")
DICT_MAX = 64 * 1024  # 64 KiB wall

TOPICS = [
    "nostr", "design", "code", "music", "photography", "travel", "food",
    "security", "privacy", "meshnet", "sats", "events", "dating", "groups",
]
OPENERS = [
    "just shipped", "why I think", "hot take:", "unpopular opinion:",
    "quick thread on", "PSA:", "big week for", "let's talk about",
]
CLOSERS = [
    "what do you think?", "thoughts?", "thread below",
    "links in replies", "feel free to disagree",
]
NAMES = ["alice", "bob", "carol", "dave", "erin", "frank", "grace"]


def rand_post(rng: random.Random) -> dict:
    words = ["alpha", "beta", "gamma", "delta", "relay", "keypair", "nsec",
             "relay pool", "event", "signature", "zap", "invoice", "touch",
             "polish", "battery", "frame rate", "scroll", "async", "worker"]
    body = " ".join(rng.choice(words) for _ in range(rng.randint(6, 40)))
    text = f"{rng.choice(OPENERS)} {rng.choice(TOPICS)}. {body} {rng.choice(CLOSERS)}"
    return {
        "id": "note1" + "".join(rng.choice("abcdef0123456789") for _ in range(56)),
        "pubkey": "npub1" + "".join(rng.choice("abcdef0123456789") for _ in range(56)),
        "created_at": rng.randint(1_700_000_000, 1_780_000_000),
        "kind": 1,
        "tags": [["t", rng.choice(TOPICS)]],
        "content": text,
        "sig": "".join(rng.choice("abcdef0123456789") for _ in range(128)),
    }


def rand_feed_entry(rng: random.Random, post: dict) -> dict:
    return {
        "id": post["id"],
        "author": rng.choice(NAMES),
        "author_pk": post["pubkey"],
        "content": post["content"],
        "created_at": post["created_at"],
        "likes": rng.randint(0, 400),
        "replies": rng.randint(0, 80),
        "zaps_msat": rng.randint(0, 2_000_000),
        "media": [{"kind": "image", "w": 1200, "h": 800,
                   "url": "https://cdn.example.com/" + post["id"]}]
        if rng.random() < 0.3 else [],
    }


def gen_corpus(count: int, outdir: str) -> None:
    rng = random.Random(7)
    os.makedirs(outdir, exist_ok=True)
    for i in range(count // 2):
        with open(os.path.join(outdir, f"post_{i:05d}.json"), "w") as f:
            json.dump(rand_post(rng), f, separators=(",", ":"))
    for i in range(count // 2):
        post = rand_post(rng)
        with open(os.path.join(outdir, f"feed_{i:05d}.json"), "w") as f:
            json.dump(rand_feed_entry(rng, post), f, separators=(",", ":"))


def train(corpus_dir: str) -> None:
    samples = [os.path.join(corpus_dir, f) for f in sorted(os.listdir(corpus_dir))]
    os.makedirs(os.path.dirname(DICT_OUT), exist_ok=True)
    cmd = ["zstd", "--train", f"--maxdict={DICT_MAX}", "-o", DICT_OUT] + samples
    subprocess.run(cmd, check=True, capture_output=True)
    size = os.path.getsize(DICT_OUT)
    print(f"dict written: {DICT_OUT} ({size} bytes)")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", default=None)
    ap.add_argument("--samples", type=int, default=2000)
    args = ap.parse_args()

    if args.corpus:
        train(args.corpus)
        return
    with tempfile.TemporaryDirectory() as tmp:
        gen_corpus(args.samples, tmp)
        train(tmp)


if __name__ == "__main__":
    sys.exit(main())