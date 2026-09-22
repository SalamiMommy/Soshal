"""Shared helpers for the Soshal lightweight moderation NN pipeline.

Ports the exact normalization semantics of moderation-core/src/normalize.rs
so training-time augmentation matches inference-time variant generation, and
implements the feature-hash + embedding-bag feature extraction that the Rust
forward pass (moderation-core/src/nn.rs) reproduces exactly.
"""
from __future__ import annotations

import struct

MASK64 = (1 << 64) - 1
FNV_OFFSET = 0xCBF29CE484222325
FNV_PRIME = 0x100000001B3

# --- exact parity with moderation-core/src/normalize.rs --------------------

INVISIBLE_OR_CONTROL = set(
    [
        0x200B, 0x200C, 0x200D, 0x200E, 0x200F, 0x202A, 0x202B, 0x202C, 0x202D,
        0x202E, 0x2060, 0xFEFF, 0x00AD,
    ]
)


def is_invisible_or_control(c: str) -> bool:
    cp = ord(c)
    if cp in INVISIBLE_OR_CONTROL:
        return True
    ranges = [
        (0x0300, 0x036F), (0x1AB0, 0x1AFF), (0x1DC0, 0x1DFF), (0x20D0, 0x20FF),
        (0xFE20, 0xFE2F), (0x0000, 0x0008), (0x000E, 0x001F), (0x007F, 0x009F),
    ]
    return any(lo <= cp <= hi for lo, hi in ranges)


HOMOGLYPH: dict[str, str] = {}

# Cyrillic lookalikes
for _src, _dst in [
    ("аА", "a"), ("бБ", "b"), ("вВ", "b"), ("гГ", "r"), ("дД", "d"),
    ("еЕёЁєЄ", "e"), ("жЖ", "z"), ("зЗ", "z"), ("иИіІїЇ", "i"), ("йЙ", "i"),
    ("кК", "k"), ("лЛ", "l"), ("мМ", "m"), ("нН", "h"), ("оО", "o"),
    ("пП", "n"), ("рР", "p"), ("сС", "c"), ("тТ", "t"), ("уУ", "y"),
    ("фФ", "f"), ("хХ", "x"), ("цЦ", "u"), ("чЧ", "h"), ("шШщЩ", "w"),
    ("ъЪьЬ", "b"), ("ыЫ", "y"), ("эЭ", "e"), ("юЮ", "u"), ("яЯ", "r"),
]:
    for ch in _src:
        HOMOGLYPH[ch] = _dst

# Greek lookalikes
for _src, _dst in [
    ("αΑ", "a"), ("βΒ", "b"), ("γΓ", "y"), ("δΔ", "d"), ("εΕ", "e"),
    ("ζΖ", "z"), ("ηΗ", "h"), ("θΘ", "o"), ("ιΙ", "i"), ("κΚ", "k"),
    ("λΛ", "l"), ("μΜ", "m"), ("νΝ", "n"), ("ξΞ", "x"), ("οΟ", "o"),
    ("πΠ", "n"), ("ρΡ", "p"), ("σΣς", "s"), ("τΤ", "t"), ("υΥ", "u"),
    ("φΦ", "f"), ("χΧ", "x"), ("ψΨ", "y"), ("ωΩ", "w"),
]:
    for ch in _src:
        HOMOGLYPH[ch] = _dst

# Latin letters with diacritics / accents
for _src, _dst in [
    ("àáâãäåāăąǎÀÁÂÃÄÅĀĂĄǍ", "a"),
    ("çćĉċčÇĆĈĊČ", "c"),
    ("èéêëēĕėęěÈÉÊËĒĔĖĘĚ", "e"),
    ("ìíîïĩīĭįǐÌÍÎÏĨĪĬĮǏ", "i"),
    ("ñńņňÑŃŅŇ", "n"),
    ("òóôõöōŏőǒøÒÓÔÕÖŌŎŐǑØ", "o"),
    ("ùúûüũūŭůűǔÙÚÛÜŨŪŬŮŰǓ", "u"),
    ("ýÿŷÝŸŶ", "y"),
    ("śŝşšŚŜŞŠ", "s"),
]:
    for ch in _src:
        HOMOGLYPH[ch] = _dst


def map_homoglyph(c: str) -> str:
    if "\uFF01" <= c <= "\uFF5E":
        code = ord(c) - 0xFEE0
        return chr(code)
    return HOMOGLYPH.get(c, c)


LEET: dict[str, str] = {
    "0": "o", "1": "i", "!": "i", "|": "i", "3": "e", "4": "a", "@": "a",
    "5": "s", "$": "s", "7": "t", "+": "t", "8": "b",
}


def normalize_basic(text: str) -> str:
    return "".join(
        map_homoglyph(c).lower() for c in text if not is_invisible_or_control(c)
    )


def normalize_leetspeak(text: str) -> str:
    out = []
    for c in text:
        if is_invisible_or_control(c):
            continue
        h = map_homoglyph(c)
        l = LEET.get(h, h)
        out.append(l.lower())
    return "".join(out)


def collapse_repeats(text: str) -> str:
    kept: list[str] = []
    last = None
    for c in text:
        if c != last:
            last = c
            kept.append(c)
    return "".join(kept)


def collapse_spaced_words(text: str) -> str:
    if " " not in text:
        words = text.split()
        return " ".join(words)
    chars = list(text)
    n = len(chars)
    i = 0
    inter = []
    while i < n:
        c = chars[i]
        inter.append(c)
        if (
            c.isalnum()
            and i + 2 < n
            and chars[i + 1] == " "
            and chars[i + 2].isalnum()
            and (i + 3 >= n or chars[i + 3] == " " or not chars[i + 3].isalnum())
        ):
            i += 2
            continue
        i += 1
    return " ".join("".join(inter).split())


def generate_normalized_variants(text: str) -> list[str]:
    trimmed = text.strip()
    if not trimmed:
        return []
    basic = normalize_basic(trimmed)
    leet = normalize_leetspeak(trimmed)
    collapsed = collapse_repeats(leet)
    unspaced = collapse_spaced_words(trimmed)
    unspaced_leet = collapse_spaced_words(collapsed)
    variants = [trimmed]
    for v in (basic, leet, collapsed, unspaced, unspaced_leet):
        if v not in variants:
            variants.append(v)
    return variants


# --- feature hashing --------------------------------------------------------

def fnv1a64(data: bytes, salt: int) -> int:
    h = FNV_OFFSET
    salts = struct.pack("<Q", salt & MASK64)
    for b in salts + data:
        h ^= b
        h = (h * FNV_PRIME) & MASK64
    return h


def char_ngrams(s: str, lo: int = 3, hi: int = 5, max_features: int = 4096) -> list[str]:
    n = len(s)
    feats: list[str] = []
    seen: set[str] = set()
    for w in range(lo, hi + 1):
        for i in range(n - w + 1):
            g = s[i : i + w]
            if g not in seen:
                seen.add(g)
                feats.append(g)
                if len(feats) >= max_features:
                    return feats
    return feats


def extract_features(variants: list[str], lo: int = 3, hi: int = 5,
                     max_features: int = 4096) -> list[str]:
    """Union of char n-grams across normalized variants, insertion-ordered."""
    feats: list[str] = []
    seen: set[str] = set()
    for v in variants:
        for g in char_ngrams(v.lower(), lo, hi, max_features):
            if g not in seen:
                seen.add(g)
                feats.append(g)
                if len(feats) >= max_features:
                    return feats
    return feats


def hash_ngram(ng: str, salt: int) -> tuple[int, int]:
    """Returns (bucket_index, sign) for one embedding table."""
    h = fnv1a64(ng.encode("utf-8"), salt)
    idx = h & 0x3FFF  # 2^14 buckets
    sign = 1 if (h >> 63) == 0 else -1
    return idx, sign


# --- mlpack binary format ---------------------------------------------------
# magic b"SOSMLP1" + u8 version
# u32 n_buckets, u32 emb_dim, u32 n_hidden, u32 n_classes,
# u32 ngram_lo, u32 ngram_hi,
# u64 salt1, u64 salt2,
# then f32 LE tensors:
#   T1[n_buckets*emb_dim] T2[n_buckets*emb_dim]
#   W1[(2*emb_dim)*n_hidden] B1[n_hidden]
#   W2[n_hidden*n_classes] B2[n_classes]


def write_mlpack(path: str, tensors: dict, meta: dict) -> None:
    with open(path, "wb") as f:
        f.write(b"SOSMLP1")
        f.write(struct.pack("<B", 1))
        for k in ("n_buckets", "emb_dim", "n_hidden", "n_classes", "ngram_lo", "ngram_hi"):
            f.write(struct.pack("<I", meta[k]))
        f.write(struct.pack("<Q", meta["salt1"]))
        f.write(struct.pack("<Q", meta["salt2"]))
        for name in ("T1", "T2", "W1", "B1", "W2", "B2"):
            arr = tensors[name].reshape(-1).astype("<f4")
            f.write(arr.tobytes())


def load_mlpack(path: str) -> tuple[dict, dict]:
    with open(path, "rb") as f:
        data = f.read()
    assert data[:7] == b"SOSMLP1", "bad magic"
    version = data[7]
    assert version == 1, f"bad version {version}"
    off = 8

    def u32() -> int:
        nonlocal off
        v = struct.unpack_from("<I", data, off)[0]
        off += 4
        return v

    def u64() -> int:
        nonlocal off
        v = struct.unpack_from("<Q", data, off)[0]
        off += 8
        return v

    meta = {
        "n_buckets": u32(), "emb_dim": u32(), "n_hidden": u32(),
        "n_classes": u32(), "ngram_lo": u32(), "ngram_hi": u32(),
        "salt1": u64(), "salt2": u64(),
    }
    tensors = {}

    def f32s(count: int) -> "numpy.ndarray":
        nonlocal off
        import numpy as np
        arr = np.frombuffer(data, dtype="<f4", count=count, offset=off)
        off += count * 4
        return arr.copy()

    n, d, h, k = meta["n_buckets"], meta["emb_dim"], meta["n_hidden"], meta["n_classes"]
    tensors["T1"] = f32s(n * d)
    tensors["T2"] = f32s(n * d)
    tensors["W1"] = f32s((2 * d) * h)
    tensors["B1"] = f32s(h)
    tensors["W2"] = f32s(h * k)
    tensors["B2"] = f32s(k)
    return tensors, meta