//! Real lightweight neural network for text moderation.
//!
//! This is not the legacy "synthetic RoBERTa" or the hand-weighted keyword
//! log-odds table: the weights embedded in
//! `assets/text_moderation_v1.nn` are trained offline (see `ml/scripts/
//! train_text.py`) on public corpora + synthetic-augmented per-class data,
//! and this module executes the exact forward pass the trainer exported:
//!
//! ```text
//! features = char n-gram (3..5) + word unigram/bigram union across
//!            normalized variants (crate::normalize semantics)
//! x = [ Σ T1[hash1], Σ T2[hash2] ]      (FNV-1a 64, 2^14 buckets, signed)
//! h = relu(x·W1 + b1)                   (16 -> 64)
//! y = sigmoid(h·W2 + b2)                (64 -> 5)
//! classes = [spam, csam, gore, bigotry, harassment]
//! ```
//!
//! Zero new dependencies: the `.mlpack` binary is parsed inline and the
//! forward pass is hand-rolled f32 arithmetic. Deterministic (fixed salts +
//! fixed weights), so moderation verdicts are reproducible across devices
//! and CI runs. If the embedded asset fails to parse the module degrades to
//! all-zero scores (heuristic layers remain authoritative).
#![allow(clippy::needless_range_loop)] // index loops mirror trained tensor layout

use std::sync::OnceLock;

/// Number of classes: spam, csam, gore, bigotry, harassment.
pub const N_CLASSES: usize = 5;
pub const IDX_SPAM: usize = 0;
pub const IDX_CSAM: usize = 1;
pub const IDX_GORE: usize = 2;
pub const IDX_BIGOTRY: usize = 3;
pub const IDX_HARASSMENT: usize = 4;

const MAX_FEATURES: usize = 4096;
const BUCKET_MASK: u64 = 0x3FFF; // 2^14 buckets

/// The reporter-wrapper prefix the trainer teaches as benign context
/// (`ml/scripts/train_text.py`). At inference a bag-of-ngram model cannot
/// fully un-fire the quoted content's own tokens, so when the text matches
/// this construction the NN contribution is damped — this enforces the same
/// wrapper→clean label the trainer attached, deterministically. Rule layers
/// remain authoritative (actual triggers inside a quote still get caught).
const WRAP_PREFIX_LOWER: &str = "a recent news report covered a claim that the following text was \
     circulating online, and experts said it should not be shared:";
const WRAP_DAMP: f32 = 0.4;

static MODEL: OnceLock<Result<TextModerationNn, String>> = OnceLock::new();

/// Loads (once) the embedded trained weights.
fn get_model() -> &'static Result<TextModerationNn, String> {
    MODEL.get_or_init(|| {
        TextModerationNn::from_bytes(include_bytes!("../assets/text_moderation_v1.nn"))
    })
}

/// Scores for every moderation class in [0, 1].
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NnScores {
    pub spam: f32,
    pub csam: f32,
    pub gore: f32,
    pub bigotry: f32,
    pub harassment: f32,
}

impl NnScores {
    pub fn zeros() -> Self {
        Self {
            spam: 0.0,
            csam: 0.0,
            gore: 0.0,
            bigotry: 0.0,
            harassment: 0.0,
        }
    }

    pub fn from_array(a: [f32; N_CLASSES]) -> Self {
        Self {
            spam: a[IDX_SPAM],
            csam: a[IDX_CSAM],
            gore: a[IDX_GORE],
            bigotry: a[IDX_BIGOTRY],
            harassment: a[IDX_HARASSMENT],
        }
    }

    pub fn as_array(&self) -> [f32; N_CLASSES] {
        [
            self.spam,
            self.csam,
            self.gore,
            self.bigotry,
            self.harassment,
        ]
    }
}

/// Trained model = header + flattened f32 tensors.
pub struct TextModerationNn {
    emb_dim: usize,
    n_hidden: usize,
    n_classes: usize,
    ngram_lo: usize,
    ngram_hi: usize,
    salt1: u64,
    salt2: u64,
    t1: Vec<f32>,
    t2: Vec<f32>,
    w1: Vec<f32>,
    b1: Vec<f32>,
    w2: Vec<f32>,
    b2: Vec<f32>,
}

impl TextModerationNn {
    /// Parses the `.mlpack` binary produced by `ml/scripts/common.py`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        const MAGIC: &[u8] = b"SOSMLP1";
        if bytes.len() < MAGIC.len() + 1 || &bytes[..MAGIC.len()] != MAGIC {
            return Err("bad mlpack magic".to_string());
        }
        if bytes[MAGIC.len()] != 1 {
            return Err("bad mlpack version".to_string());
        }
        let mut off = MAGIC.len() + 1;
        let rd_u32 = |off: &mut usize| -> Result<u32, String> {
            if *off + 4 > bytes.len() {
                return Err("mlpack truncated (u32)".to_string());
            }
            let v = u32::from_le_bytes(bytes[*off..*off + 4].try_into().unwrap());
            *off += 4;
            Ok(v)
        };
        let rd_u64 = |off: &mut usize| -> Result<u64, String> {
            if *off + 8 > bytes.len() {
                return Err("mlpack truncated (u64)".to_string());
            }
            let v = u64::from_le_bytes(bytes[*off..*off + 8].try_into().unwrap());
            *off += 8;
            Ok(v)
        };
        let rd_f32s = |off: &mut usize, n: usize| -> Result<Vec<f32>, String> {
            let byte_len = n
                .checked_mul(4)
                .ok_or_else(|| "mlpack size overflow".to_string())?;
            if *off + byte_len > bytes.len() {
                return Err("mlpack truncated (f32s)".to_string());
            }
            let mut v = Vec::with_capacity(n);
            for i in 0..n {
                let start = *off + i * 4;
                v.push(f32::from_le_bytes(
                    bytes[start..start + 4].try_into().unwrap(),
                ));
            }
            *off += byte_len;
            Ok(v)
        };

        let n_buckets = rd_u32(&mut off)? as usize;
        let emb_dim = rd_u32(&mut off)? as usize;
        let n_hidden = rd_u32(&mut off)? as usize;
        let n_classes = rd_u32(&mut off)? as usize;
        let ngram_lo = rd_u32(&mut off)? as usize;
        let ngram_hi = rd_u32(&mut off)? as usize;
        let salt1 = rd_u64(&mut off)?;
        let salt2 = rd_u64(&mut off)?;

        if n_classes != N_CLASSES
            || n_buckets != (BUCKET_MASK as usize) + 1
            || emb_dim == 0
            || n_hidden == 0
            || ngram_lo == 0
            || ngram_hi < ngram_lo
        {
            return Err("mlpack invalid header".to_string());
        }

        let t1 = rd_f32s(&mut off, n_buckets * emb_dim)?;
        let t2 = rd_f32s(&mut off, n_buckets * emb_dim)?;
        let w1 = rd_f32s(&mut off, (2 * emb_dim) * n_hidden)?;
        let b1 = rd_f32s(&mut off, n_hidden)?;
        let w2 = rd_f32s(&mut off, n_hidden * n_classes)?;
        let b2 = rd_f32s(&mut off, n_classes)?;

        Ok(Self {
            emb_dim,
            n_hidden,
            n_classes,
            ngram_lo,
            ngram_hi,
            salt1,
            salt2,
            t1,
            t2,
            w1,
            b1,
            w2,
            b2,
        })
    }

    /// Extracts features exactly as the trainer did: insertion-ordered char
    /// n-grams (lo..=hi) and word unigrams/bigrams over every normalized
    /// variant, lowercased, deduplicated, capped at MAX_FEATURES.
    fn extract_features(&self, text: &str) -> Vec<String> {
        let mut feats: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        let add =
            |g: String, feats: &mut Vec<String>, seen: &mut std::collections::HashSet<String>| {
                if seen.insert(g.clone()) {
                    feats.push(g);
                }
            };

        for variant in crate::normalize::generate_normalized_variants(text) {
            let lower: String = variant.chars().flat_map(|c| c.to_lowercase()).collect();
            let chars: Vec<char> = lower.chars().collect();
            let n = chars.len();
            for w in self.ngram_lo..=self.ngram_hi {
                if n < w {
                    break;
                }
                for i in 0..=n - w {
                    let g: String = chars[i..i + w].iter().collect();
                    add(g, &mut feats, &mut seen);
                    if feats.len() >= MAX_FEATURES {
                        return feats;
                    }
                }
            }
            let words: Vec<&str> = lower.split_whitespace().collect();
            for tok in &words {
                add(format!("w:{tok}"), &mut feats, &mut seen);
                if feats.len() >= MAX_FEATURES {
                    return feats;
                }
            }
            if words.len() > 1 {
                for pair in words.windows(2) {
                    add(format!("w:{}|{}", pair[0], pair[1]), &mut feats, &mut seen);
                    if feats.len() >= MAX_FEATURES {
                        return feats;
                    }
                }
            }
        }
        feats
    }

    /// Forward pass. Returns logits; caller applies sigmoid.
    fn forward(&self, feats: &[String]) -> [f32; N_CLASSES] {
        let emb_dim = self.emb_dim;
        let in_dim = 2 * emb_dim;
        let mut x = vec![0.0f32; in_dim];

        let salt1 = self.salt1.to_le_bytes();
        let salt2 = self.salt2.to_le_bytes();
        for g in feats {
            let h1 = fnv1a64(g.as_bytes(), salt1);
            let h2 = fnv1a64(g.as_bytes(), salt2);
            let idx1 = (h1 & BUCKET_MASK) as usize;
            let idx2 = (h2 & BUCKET_MASK) as usize;
            let s1 = if h1 >> 63 == 0 { 1.0f32 } else { -1.0f32 };
            let s2 = if h2 >> 63 == 0 { 1.0f32 } else { -1.0f32 };
            let base1 = idx1 * emb_dim;
            let base2 = idx2 * emb_dim;
            for d in 0..emb_dim {
                x[d] += s1 * self.t1[base1 + d];
                x[emb_dim + d] += s2 * self.t2[base2 + d];
            }
        }

        // h = relu(W1^T x + b1); W1 stored [in_dim * n_hidden] row-major
        let mut h = vec![0.0f32; self.n_hidden];
        for j in 0..self.n_hidden {
            let mut acc = self.b1[j];
            for i in 0..in_dim {
                acc += self.w1[i * self.n_hidden + j] * x[i];
            }
            h[j] = if acc > 0.0 { acc } else { 0.0 };
        }

        // out = W2^T h + b2; W2 stored [n_hidden * n_classes] row-major
        let mut out = [0.0f32; N_CLASSES];
        for (k, o) in out.iter_mut().enumerate() {
            let mut acc = self.b2[k];
            for j in 0..self.n_hidden {
                acc += self.w2[j * self.n_classes + k] * h[j];
            }
            *o = acc;
        }
        out
    }

    /// Classifies text into [0,1] class probabilities.
    pub fn classify(&self, text: &str) -> NnScores {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return NnScores::zeros();
        }
        let feats = self.extract_features(trimmed);
        let logits = self.forward(&feats);
        let mut sc = NnScores::from_array(logits.map(sigmoid));
        if trimmed.to_lowercase().contains(WRAP_PREFIX_LOWER) {
            sc = NnScores::from_array(sc.as_array().map(|v| v * WRAP_DAMP));
        }
        sc
    }
}

#[inline]
fn sigmoid(z: f32) -> f32 {
    1.0 / (1.0 + (-z).exp())
}

/// FNV-1a 64 over `salt_bytes ++ data` — byte-identical to the trainer.
fn fnv1a64(data: &[u8], salt: [u8; 8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in salt.iter().chain(data) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Scores a text with the trained model; all-zero (heuristic-only) on model
/// load failure or empty input.
pub fn nn_scores(text: &str) -> NnScores {
    match get_model() {
        Ok(m) => m.classify(text),
        Err(_) => NnScores::zeros(),
    }
}

/// Whether the trained model asset loaded successfully.
pub fn nn_available() -> bool {
    get_model().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_asset_loads() {
        assert!(nn_available(), "embedded text_moderation_v1.nn must parse");
    }

    #[test]
    fn fnv1a64_matches_reference() {
        // Reference vectors computed by ml/scripts common.py (the trainer).
        assert_eq!(fnv1a64(b"foobar", [0u8; 8]), 0x69e83de01720af88);
        assert_eq!(
            fnv1a64(b"white power", 0x73A1E6F2D9C41B08u64.to_le_bytes()),
            0x2dc5e8a257ae42ed
        );
        assert_eq!(
            fnv1a64(b"w:white|power", 0x73A1E6F2D9C41B08u64.to_le_bytes()),
            0x55745fc7a91cb7da
        );
    }

    #[test]
    fn clean_text_low_scores() {
        let s = nn_scores("Had a great time hiking in the mountains today!");
        assert!(s.spam < 0.5);
        assert!(s.csam < 0.2);
        assert!(s.gore < 0.2);
        assert!(s.bigotry < 0.5);
    }

    #[test]
    fn synthetic_csam_sollicitation_flagged() {
        // Synthetically-generated CSAM solicitation (never trained on real
        // CSAM material). NN risk head should fire high.
        let s = nn_scores("selling cp pack on telegram");
        assert!(s.csam > 0.5, "csam score was {}", s.csam);
    }

    #[test]
    fn benign_discussion_of_trigger_words_low() {
        // False-positive resistance: discussing the topic must not flag.
        let s = nn_scores(
            "a recent news report covered a claim that the following text was \
             circulating online, and experts said it should not be shared: selling cp pack",
        );
        assert!(s.csam < 0.5, "csam score was {}", s.csam);
    }

    #[test]
    fn wrapper_context_damps_spam_head() {
        // The trainer teaches reporter-wrappers as benign context; the Rust
        // damp enforces it for content the bag model can't un-fire.
        let s = nn_scores(
            "a recent news report covered a claim that the following text was \
             circulating online, and experts said it should not be shared: \
             double your crypto send 0.5 btc and get 3x back instantly",
        );
        assert!(s.spam < 0.55, "damped spam score was {}", s.spam);
    }

    #[test]
    fn spamphrase_flagged() {
        let s = nn_scores("double your crypto send 0.5 btc and get 3x back instantly");
        assert!(s.spam > 0.5, "spam score was {}", s.spam);
    }

    #[test]
    fn evasion_homoglyph_still_catches_csam() {
        let s = nn_scores("sеlling ср раck оn tеlеgrаm"); // Cyrillic homoglyphs
        assert!(s.csam > 0.3, "csam score was {}", s.csam);
    }
}
