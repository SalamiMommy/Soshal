"""Build the moderation training corpus.

Sources (all public / synthetic — NO real CSAM material anywhere in this
pipeline, by design; CSAM-class examples are generated-synthetic only):
  - Davidson hate-speech/offensive-language corpus  (bigotry, harassment, clean)
  - UCI SMS Spam (ucirvine/sms_spam on HF)          (spam, clean)
  - Shakespeare + Gutenberg (Pride & Prejudice)     (clean literary)
  - Templated synthetic positives (spam/gore/csam/bigotry/harassment) built
    from the exact rule vocab already in moderation-core, plus adversarial
    evasion augmentation (leetspeak, homoglyphs, char-stretch, zero-width,
    spread-letter). Clean "safe-mention" templates teach false-positive
    resistance for trigger words used in benign context.

Output: corpus.jsonl rows {"text": str, "labels": {spam,csam,gore,bigotry,harassment}: 0|1}
"""
from __future__ import annotations

import csv
import html
import json
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import (  # noqa: E402
    LEET,
    collapse_spaced_words,
    generate_normalized_variants,
    map_homoglyph,
    normalize_basic,
)

ROOT = Path(__file__).resolve().parent.parent
CORPUS = ROOT / "corpus"
OUT = ROOT / "corpus.jsonl"

RNG = random.Random(42)
CLASSES = ["spam", "csam", "gore", "bigotry", "harassment"]

# --- raw source loading -----------------------------------------------------


def load_davidson() -> list[tuple[str, str]]:
    """Returns (text, kind) kind in {hate, offensive, neither}."""
    rows: list[tuple[str, str]] = []
    with open(CORPUS / "davidson.csv", newline="", encoding="utf-8", errors="replace") as f:
        reader = csv.DictReader(f)
        for r in reader:
            text = html.unescape(r.get("tweet", "")).strip()
            if len(text) < 3:
                continue
            cls = r.get("class", "2")
            kind = {"0": "hate", "1": "offensive", "2": "neither"}.get(cls, "neither")
            rows.append((text[:400], kind))
    return rows


def load_sms() -> list[tuple[str, str]]:
    import pyarrow.parquet as pq

    t = pq.read_table(CORPUS / "sms.parquet")
    d = t.to_pydict()
    rows = []
    for text, label in zip(d["sms"], d["label"]):
        text = str(text).strip()
        if len(text) < 3:
            continue
        rows.append((text[:300], str(label).lower() == "spam" and "spam" or "ham"))
    return rows


def load_shakespeare_lines() -> list[str]:
    text = (CORPUS / "shakespeare.txt").read_text(encoding="utf-8", errors="replace")
    lines = []
    for line in text.splitlines():
        line = line.strip()
        if 4 <= len(line.split()) <= 40:
            lines.append(line)
    return lines


def load_pride_and_prejudice_sentences() -> list[str]:
    text = (CORPUS / "pp.txt").read_text(encoding="utf-8", errors="replace")
    start = text.find("*** START")
    end = text.find("*** END")
    if start != -1 and end != -1:
        text = text[start:end]
    out = []
    for chunk in text.replace("\r", " ").replace("\n", " ").split(". "):
        chunk = chunk.strip()
        if 4 <= len(chunk.split()) <= 40:
            out.append(chunk + ".")
    return out


# --- synthetic banks ---------------------------------------------------------

SPAM_TEMPLATES = [
    "double your crypto send {amt} {coin} to {addr} and get {mult}x back instantly",
    "claim your {coin} airdrop reward now before it expires",
    "validate your seed phrase at {link} to unlock your wallet rewards",
    "connect wallet and claim your {coin} giveaway official promo",
    "guaranteed {pct} profit on your investment risk free",
    "make {usd} a day from home fast cash no experience needed",
    "binary options vip trading signals join {link} for free",
    "free discord nitro giveaway limited time act now",
    "congratulations you have been selected as our lottery winner claim your prize at {link}",
    "dm me on telegram for vip crypto signals huge returns",
    "forex trading signals guaranteed returns join {link}",
    "win a {gift} click here now exclusive offer",
    "earn crypto by completing simple tasks hourly payout",
    "official wallet authentication required click here to verify your account",
    "$$$ make money fast $$$ guaranteed payout join my team {link}",
    "get rich quick with our binary options system act now",
]

SPAM_FILL = {
    "amt": ["0.1", "0.5", "1", "0.05"],
    "coin": ["btc", "eth", "sol", "usdt", "bnb"],
    "addr": ["bc1q3x", "0x4f2a", "1A1zP1eP"],
    "mult": ["2", "3", "5"],
    "pct": ["100", "200", "500"],
    "usd": ["200", "500", "3000", "10000"],
    "link": ["t.me/vip", "wa.me/911", "discord.gg/free", "bit.ly/free123", "tinyurl.com/earn"],
    "gift": ["iphone", "macbook", "cash prize", "airpods"],
}

GORE_TEMPLATES = [
    "watch this {exec} {clip} {quality}",
    "graphic {violence} {media} leaked",
    "real {death} {media} uncensored",
    "{slaughter} footage from the {region} leak",
    "how to {method} yourself suicide guide",
    "go kill yourself drink bleach",
    "slit your wrists tonight {dare}",
    "{animal} torture video animal crush clip",
    "snuff film {clip} real death video",
    "{body} {media} from the highway accident",
]

GORE_FILL = {
    "exec": ["beheading", "cartel execution", "chainsaw execution", "decapitation"],
    "clip": ["video", "clip", "leak", "footage", "hd video"],
    "quality": ["uncensored", "hd", "raw", "full"],
    "violence": ["dismemberment", "mutilation", "flaying", "electrocution"],
    "media": ["photos", "pics", "video", "footage"],
    "death": ["death", "execution", "murder", "suicide"],
    "slaughter": ["massacre", "slaughter", "mass execution"],
    "region": ["cartel", "warzone", "battlefield"],
    "method": ["kill", "hang", "shoot", "poison", "suffocate"],
    "dare": ["like a real man", "no one will miss you", "nobody cares"],
    "animal": ["kitten", "puppy", "cat", "dog"],
    "body": ["corpse", "rotting body", "severed head", "crushed skull"],
}

CSAM_TEMPLATES = [
    "trade cp pack on telegram",
    "selling {mat} {coll} {host}",
    "{mat} {coll} {host} link",
    "cp trade {host}",
    "dump box {coll} links",
    "pedo {mat} pics private dm",
    "underage {mat} mega folder",
    "preteen {conn} collection",
    "loli {mat} pack",
    "child porn {clip} leak",
    "ageplay abuse {mat} story",
    "cp on onion darknet",
    "nudes of {victim} exchange dm",
]

CSAM_FILL = {
    "mat": ["cp", "csam", "underage", "loli", "shota", "preteen"],
    "coll": ["pack", "collection", "folder", "archive", "mega", "dropbox"],
    "host": ["t.me/cp", "mega.nz/x", "dropbox.com/s", "onion"],
    "clip": ["video", "vids", "pics"],
    "conn": ["nudes", "pics", "naked"],
    "victim": ["young girl", "little boy", "kids", "minors"],
}

BIGOTRY_TEMPLATES = [
    "{slur} people are {dehuman}",
    "all {group} should be {elim}",
    "gas the {group}",
    "exterminate the {group}",
    "{group} are {revile}",
    "white power {slogan}",
    "hitler was right",
    "race traitor {call}",
    "mud people {claim}",
    "keep {group} out of our country",
    "{group} are subhuman",
    "die {group} scum",
]

BIGOTRY_FILL = {
    "slur": ["nigger", "kike", "faggot", "fag", "dyke", "tranny", "chink", "gook",
             "spic", "coon", "wetback", "shemale"],
    "group": ["niggers", "jews", "gays", "trans", "faggots", "mexicans", "muslims", "blacks"],
    "dehuman": ["subhuman", "garbage", "scum", "animals", "vermin", "parasites"],
    "elim": ["killed", "burned", "exterminated", "wiped out", "gassed"],
    "revile": ["sick", "disgusting", "unholy", "an abomination", "abnormal", "disorders"],
    "slogan": ["rally", "brotherhood", "now"],
    "call": ["you sell out", "traitor to your race"],
    "claim": ["are ruining this country", "must go"],
}

HARASS_TEMPLATES = [
    "i will find you and kill you",
    "i will hunt you down {threat}",
    "i know where you live watch your back",
    "leak your address and family photos",
    "dox your {target} to the whole internet",
    "fuck you {insult}",
    "go die {insult}",
    "{insult} piece of shit",
    "you are worthless kill yourself",
    "everyone laugh at this {creep}",
    "i will leak your chat history nudes",
    "shut up {insult} no one wants you here",
]

HARASS_FILL = {
    "threat": ["and your family", "you are dead", "tonight"],
    "target": ["school", "workplace", "family", "friends"],
    "insult": ["cunt", "bitch", "loser", "idiot", "whore", "slut", "retard"],
    "creep": ["loser", "freak", "weirdo", "pedo-lookin"],
}

SAFE_MENTION_TEMPLATES = [
    "I read an article about {topic} trends in Europe last week",
    "the news covered the {topic} situation and its political fallout",
    "my history class discussed {topic} yesterday it was interesting",
    "the report on {topic} was published by a research institute",
    "we watched a documentary about {topic} in {place}",
    "the committee is studying {topic} as part of its mandate",
    "local authorities responded to reports about {topic}",
    "the {topic} story went viral and drew many comments",
    "professor gave a lecture on {topic} its causes and effects",
    "the museum has an exhibition on {topic} historical context",
]

SAFE_FILL = {
    "topic": [
        "white power movements", "crypto scams", "bitcoin airdrops", "hate speech",
        "street violence", "suicide prevention", "online harassment", "gambling ads",
        "spam emails", "extremist groups", "child safety", "pornography laws",
        "bloody conflicts", "war casualties", "terror attacks",
    ],
    "place": ["Europe", "Asia", "the US", "Brazil", "Germany", "the UN"],
}


# --- augmentation (adversarial evasion) -------------------------------------

ZERO_WIDTH = ["\u200B", "\u200C", "\u200D", "\uFEFF", "\u00AD"]


def augment(text: str, intensity: int) -> str:
    """Randomly apply evasion transforms with the given effort (1..3)."""
    out = text
    for _ in range(intensity):
        mode = RNG.random()
        if mode < 0.30:
            # leetspeak a few chars
            out = "".join(
                LEET.get(c, c) if RNG.random() < 0.4 else c for c in out
            )
        elif mode < 0.55:
            # homoglyph a few chars
            mapped = {v: k for k, v in {
                "a": "а", "e": "е", "o": "о", "i": "і", "k": "к", "p": "р",
                "c": "с", "y": "у", "h": "н", "x": "х", "b": "в",
            }.items()}
            out = "".join(
                mapped.get(c, c) if c in mapped and RNG.random() < 0.4 else c
                for c in out
            )
        elif mode < 0.75:
            # char stretch
            out = "".join(c * RNG.choice([1, 1, 2, 3]) for c in out)
        elif mode < 0.9:
            # spread letters
            words = out.split()
            out = " ".join(
                " ".join(c for c in w) if RNG.random() < 0.5 else w for w in words
            )
        else:
            # zero-width inserts
            out = "".join(c + RNG.choice(ZERO_WIDTH) if RNG.random() < 0.3 else c for c in out)
    return out


def dedupe(rows: list[dict]) -> list[dict]:
    seen: set[str] = set()
    out = []
    for r in rows:
        key = normalize_basic(r["text"])
        if not key or len(key.split()) < 2:
            continue
        if key in seen:
            continue
        seen.add(key)
        out.append(r)
    return out


# --- main --------------------------------------------------------------------

def main() -> None:
    rows: list[dict] = []

    def add(text: str, cls: str | None, augment_count: int = 3) -> None:
        """cls in class names or None for clean."""
        labels = {c: 0 for c in CLASSES}
        if cls is not None:
            labels[cls] = 1
        texts = [text]
        if cls is not None:
            texts += [augment(text, RNG.randint(1, 3)) for _ in range(augment_count)]
        elif RNG.random() < 0.10:
            texts += [augment(text, 1)]
        for t in texts:
            rows.append({"text": t, "labels": labels})

    # Davidson: hate -> bigotry, offensive -> harassment, neither -> clean
    davidson = load_davidson()
    for text, kind in davidson:
        if kind == "hate":
            add(text, "bigotry")
        elif kind == "offensive":
            add(text, "harassment")
        else:
            add(text, None)

    # SMS: spam / ham
    for text, kind in load_sms():
        add(text, "spam" if kind == "spam" else None)

    # Literary clean
    for line in load_shakespeare_lines():
        add(line, None)
        if len(rows) > 26000 and RNG.random() < 0.5:
            break
    pp = load_pride_and_prejudice_sentences()
    for sent in pp[:12000]:
        add(sent, None)

    # Synthetic positives
    for template in SPAM_TEMPLATES:
        for _ in range(90):
            add(template.format(**{k: RNG.choice(v) for k, v in SPAM_FILL.items()}), "spam")
    for template in GORE_TEMPLATES:
        for _ in range(50):
            add(template.format(**{k: RNG.choice(v) for k, v in GORE_FILL.items()}), "gore")
    for template in CSAM_TEMPLATES:
        for _ in range(60):
            add(template.format(**{k: RNG.choice(v) for k, v in CSAM_FILL.items()}), "csam")
    for template in BIGOTRY_TEMPLATES:
        for _ in range(60):
            add(template.format(**{k: RNG.choice(v) for k, v in BIGOTRY_FILL.items()}), "bigotry")
    for template in HARASS_TEMPLATES:
        for _ in range(70):
            add(template.format(**{k: RNG.choice(v) for k, v in HARASS_FILL.items()}), "harassment")

    # Safe-mention negatives (false-positive resistance)
    for template in SAFE_MENTION_TEMPLATES:
        for _ in range(40):
            add(template.format(**{k: RNG.choice(v) for k, v in SAFE_FILL.items()}), None)
    # Hand-written hard negatives
    hard_neg = [
        "the word faggot appears in that novel once",
        "the doctor discussed suicide prevention hotlines with the patient's family",
        "crypto regulation news today bitcoin airdrop rules changed",
        "the film depicts war and bloodshed as tragedy",
        "the politician condemned hate speech in the strongest terms",
        "the teacher explained why slurs are harmful",
        "i want to understand why people join extremist groups",
        "the beach photos from our family holiday are on facebook",
        "my little sister won the swim meet she is eight",
        "scientific american published a piece on nudity in classical art",
        "we discussed the gore in the painting reevaluation of his work",
        "the article reviews a documentary about terrorism and its causes",
    ]
    for t in hard_neg:
        add(t, None)

    rows = dedupe(rows)
    RNG.shuffle(rows)

    # Emit
    with open(OUT, "w", encoding="utf-8") as f:
        for r in rows:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")

    counts = {c: 0 for c in CLASSES}
    for r in rows:
        for c in CLASSES:
            if r["labels"][c]:
                counts[c] += 1
    print(f"total rows: {len(rows)}")
    print("positive counts:", counts)
    clean = sum(1 for r in rows if not any(r["labels"].values()))
    print("clean rows:", clean)


if __name__ == "__main__":
    main()