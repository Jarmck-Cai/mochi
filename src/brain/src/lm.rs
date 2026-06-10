//! General-layer word trigram LM with stupid backoff (λ₁ of DESIGN.md §3).
//!
//! Port of experiments/001 `pipeline/lm.py::BackoffTrigramLM` scoring:
//!
//!     P(w | h2 h1) = c(h2 h1 w) / c(h2 h1)            if trigram seen
//!                  = α  · c(h1 w) / c(h1)             elif bigram seen
//!                  = α² · c(w) / total                elif unigram seen
//!                  = α² · 10^oov_floor                otherwise
//!
//! Tokens are interned u32 ids; n-gram keys are packed into u64/u128.

use rustc_hash::FxHashMap;

/// log10 floor for unseen unigrams, matching lm.py `oov_logp=-7.5`.
pub const OOV_LOGP: f64 = -7.5;

#[inline]
fn bi_key(h1: u32, w: u32) -> u64 {
    ((h1 as u64) << 32) | w as u64
}

#[inline]
fn tri_key(h2: u32, h1: u32, w: u32) -> u128 {
    ((h2 as u128) << 64) | ((h1 as u128) << 32) | w as u128
}

pub struct BackoffTrigramLm {
    #[allow(dead_code)] // recorded from the ngram.tsv header; read in tests
    pub alpha: f64,
    log_alpha: f64,
    oov_logp: f64,
    pub total: f64,
    uni: Vec<f32>,                // token id -> count (0.0 = unseen)
    bi: FxHashMap<u64, f32>,      // (h1, w) -> count
    tri: FxHashMap<u128, f32>,    // (h2, h1, w) -> count
    bi_hist: FxHashMap<u64, f32>, // (h2, h1) -> Σ tri counts (trigram denominator)
    uni_hist: Vec<f32>,           // h1 -> Σ bi counts (bigram denominator)
}

impl BackoffTrigramLm {
    pub fn new(alpha: f64, total: f64) -> Self {
        Self {
            alpha,
            log_alpha: alpha.log10(),
            oov_logp: OOV_LOGP,
            total,
            uni: Vec::new(),
            bi: FxHashMap::default(),
            tri: FxHashMap::default(),
            bi_hist: FxHashMap::default(),
            uni_hist: Vec::new(),
        }
    }

    pub fn add_unigram(&mut self, w: u32, count: f32) {
        let idx = w as usize;
        if idx >= self.uni.len() {
            self.uni.resize(idx + 1, 0.0);
        }
        self.uni[idx] += count;
    }

    pub fn add_bigram(&mut self, h1: u32, w: u32, count: f32) {
        *self.bi.entry(bi_key(h1, w)).or_insert(0.0) += count;
    }

    pub fn add_trigram(&mut self, h2: u32, h1: u32, w: u32, count: f32) {
        *self.tri.entry(tri_key(h2, h1, w)).or_insert(0.0) += count;
    }

    /// Build denominator tables (lm.py `finalize`). `vocab_size` is the
    /// final interner size so the Vec-indexed tables cover every token id.
    pub fn finalize(&mut self, vocab_size: usize) {
        if self.uni.len() < vocab_size {
            self.uni.resize(vocab_size, 0.0);
        }
        self.bi_hist.clear();
        for (&k, &c) in &self.tri {
            *self.bi_hist.entry((k >> 32) as u64).or_insert(0.0) += c;
        }
        self.uni_hist = vec![0.0; vocab_size];
        for (&k, &c) in &self.bi {
            let h1 = (k >> 32) as usize;
            if h1 < self.uni_hist.len() {
                self.uni_hist[h1] += c;
            }
        }
    }

    /// log10 P(w | h2 h1), stupid backoff. UNK ids fall through to OOV.
    pub fn logp(&self, w: u32, h2: u32, h1: u32) -> f64 {
        if w != crate::interner::UNK {
            if h2 != crate::interner::UNK && h1 != crate::interner::UNK {
                if let Some(&c3) = self.tri.get(&tri_key(h2, h1, w)) {
                    if c3 > 0.0 {
                        if let Some(&denom) = self.bi_hist.get(&bi_key(h2, h1)) {
                            if denom > 0.0 {
                                return (c3 as f64 / denom as f64).log10();
                            }
                        }
                    }
                }
            }
            if h1 != crate::interner::UNK {
                if let Some(&c2) = self.bi.get(&bi_key(h1, w)) {
                    if c2 > 0.0 {
                        let denom = self.uni_hist[h1 as usize];
                        if denom > 0.0 {
                            return self.log_alpha + (c2 as f64 / denom as f64).log10();
                        }
                    }
                }
            }
            let c1 = self.uni[w as usize];
            if c1 > 0.0 && self.total > 0.0 {
                return 2.0 * self.log_alpha + (c1 as f64 / self.total).log10();
            }
        }
        2.0 * self.log_alpha + self.oov_logp
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        let uni_n = self.uni.iter().filter(|&&c| c > 0.0).count();
        (uni_n, self.bi.len(), self.tri.len())
    }

    /// Rough heap usage in bytes for load-time reporting.
    pub fn est_bytes(&self) -> usize {
        // FxHashMap entry ≈ (key + val) / load_factor(~0.85) + control byte
        let bi_entry = (8 + 4 + 1) * 100 / 85;
        let tri_entry = (16 + 4 + 1) * 100 / 85;
        self.uni.len() * 4
            + self.uni_hist.len() * 4
            + self.bi.len() * bi_entry
            + self.tri.len() * tri_entry
            + self.bi_hist.len() * bi_entry
    }
}

/// λ₂ personal layer slot (DESIGN.md §3). M2-3 will implement the real
/// time-decayed per-scene personal LM + lexicon; the decoder already
/// combines it in linear probability space exactly like decoder.py:
///
///     P(w|h) = μ_g·P_general + μ_p·P_personal,  + λ_lex·lexicon_bonus(w)
pub trait PersonalLayer: Send + Sync {
    /// Linear-space probability mass P_personal(w | h2 h1); 0.0 when silent.
    fn p(&self, _w: u32, _h2: u32, _h1: u32) -> f64 {
        0.0
    }
    /// log10 personal-lexicon bonus for token w; 0.0 when none.
    fn lexicon_bonus(&self, _w: u32) -> f64 {
        0.0
    }
}

/// Null object until M2-3 (decoder.py `EmptyLM` + empty lexicon).
pub struct NoPersonalLayer;

impl PersonalLayer for NoPersonalLayer {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mini LM mirroring lm.py semantics:
    /// total=1000; uni: A=100 B=50; bi: (A,B)=20 (A,C missing); tri: (X,A,B)=8
    fn mini() -> (BackoffTrigramLm, u32, u32, u32, u32) {
        let (a, b, x, d) = (0u32, 1u32, 2u32, 3u32);
        let mut lm = BackoffTrigramLm::new(0.4, 1000.0);
        lm.add_unigram(a, 100.0);
        lm.add_unigram(b, 50.0);
        lm.add_bigram(a, b, 20.0);
        lm.add_trigram(x, a, b, 8.0);
        lm.finalize(4);
        (lm, a, b, x, d)
    }

    #[test]
    fn trigram_path_uses_trigram_denominator() {
        let (lm, a, b, x, _) = mini();
        // bi_hist[(X,A)] = 8 (sum of trigram counts), so P = 8/8 = 1.0
        assert!((lm.logp(b, x, a) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn bigram_backoff_applies_alpha() {
        let (lm, a, b, x, _) = mini();
        // history (A=h2? no): w=B, h2=B(no tri), h1=A: bi(A,B)=20, uni_hist[A]=20
        let expect = 0.4f64.log10() + (20.0f64 / 20.0).log10();
        assert!((lm.logp(b, b, a) - expect).abs() < 1e-9);
        // and differs from the trigram path
        assert!((lm.logp(b, x, a) - lm.logp(b, b, a)).abs() > 1e-6);
    }

    #[test]
    fn unigram_backoff_applies_alpha_squared() {
        let (lm, a, _, x, _) = mini();
        // w=A with unseen history: α² · 100/1000
        let expect = 2.0 * 0.4f64.log10() + (100.0f64 / 1000.0).log10();
        assert!((lm.logp(a, x, x) - expect).abs() < 1e-9);
    }

    #[test]
    fn oov_hits_floor() {
        let (lm, _, _, x, d) = mini();
        let expect = 2.0 * 0.4f64.log10() + OOV_LOGP;
        assert!((lm.logp(d, x, x) - expect).abs() < 1e-9);
        assert!((lm.logp(crate::interner::UNK, x, x) - expect).abs() < 1e-9);
    }
}
