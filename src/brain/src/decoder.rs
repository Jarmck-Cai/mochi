//! Beam-search decoder over the key-stream lattice (port of pipeline/decoder.py).
//!
//! Scoring follows DESIGN.md §3. The general LM (λ₁) and the personal layer
//! (λ₂, M2-3) combine in linear probability space, then log-space arc-type
//! priors and the personal-lexicon bonus are added:
//!
//!     P(w|h)  = μ_g · P_general(w|h) + μ_p · P_personal(w|h)
//!     score  += log10 P(w|h) + prior[kind] + λ_lex · lexicon_bonus(w)

use rustc_hash::FxHashMap;

use crate::lattice::{Arc, ArcKind, LatticeBuilder, PersonalMatchers};
use crate::lm::{BackoffTrigramLm, PersonalLayer};

/// Log10 prior per arc type (decoder.py `DEFAULT_ARC_PRIOR`): dictionary
/// words neutral; single chars mildly penalized (prefer longer words);
/// English arcs pay a mode-switch cost; fallback letters are last resort.
#[derive(Debug, Clone, Copy)]
pub struct ArcPriors {
    pub py_word: f64,
    pub py_char: f64,
    pub py_personal: f64,
    pub en_word: f64,
    pub en_personal: f64,
    /// An English word right after another English word: the mode switch
    /// was already paid, continuing the run is cheap ("timewindow" must
    /// not pay -3 twice and lose to 提么+window).
    pub en_continue: f64,
    pub fallback: f64,
    pub fallback_tail: f64,
    /// Added on top of the kind prior for typing-ahead completion arcs.
    pub completion: f64,
    /// Added for fuzzy-pinyin matches: exact spelling always outranks.
    pub fuzzy: f64,
}

impl Default for ArcPriors {
    fn default() -> Self {
        Self {
            py_word: 0.0,
            py_char: -0.6,
            py_personal: 0.0,
            en_word: -3.0,
            en_personal: -1.0, // user's own terms pay a smaller switch cost
            en_continue: -0.5,
            fallback: -12.0,
            // unfinished tail: keeping raw letters is the *expected* display
            // while a syllable is half-typed, so the cost is mild
            fallback_tail: -1.5,
            completion: -2.0,
            fuzzy: -1.5,
        }
    }
}

impl ArcPriors {
    #[inline]
    fn of(&self, kind: ArcKind) -> f64 {
        match kind {
            ArcKind::PyWord => self.py_word,
            ArcKind::PyChar => self.py_char,
            ArcKind::PyPersonal => self.py_personal,
            ArcKind::EnWord => self.en_word,
            ArcKind::EnPersonal => self.en_personal,
            ArcKind::Fallback => self.fallback,
            ArcKind::FallbackTail => self.fallback_tail,
        }
    }
}

pub struct Scorer<'a> {
    pub lm: &'a BackoffTrigramLm,
    pub personal: &'a dyn PersonalLayer,
    pub priors: ArcPriors,
    pub mu_general: f64,
    pub mu_personal: f64,
    pub lambda_lexicon: f64,
}

impl<'a> Scorer<'a> {
    /// Baseline scorer (tests/diagnostics); production uses `full`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn general_only(lm: &'a BackoffTrigramLm, personal: &'a dyn PersonalLayer) -> Self {
        Self {
            lm,
            personal,
            priors: ArcPriors::default(),
            mu_general: 1.0,
            mu_personal: 0.0,
            lambda_lexicon: 0.0,
        }
    }

    /// The winning configuration of experiment 001 ("full(+LM+lexicon)",
    /// run_experiment.py): the production scorer since M2-3.
    pub fn full(lm: &'a BackoffTrigramLm, personal: &'a dyn PersonalLayer) -> Self {
        Self {
            lm,
            personal,
            priors: ArcPriors::default(),
            mu_general: 0.65,
            mu_personal: 0.35,
            lambda_lexicon: 0.8,
        }
    }

    #[inline]
    fn arc_score(&self, arc: &Arc, h2: u32, h1: u32, prev_en: bool) -> f64 {
        let logp_g = self.lm.logp(arc.lm_token, h2, h1);
        let mut score = if self.mu_personal > 0.0 {
            // linear interpolation in probability space (decoder.py)
            let p = self.mu_general * 10f64.powf(logp_g)
                + self.mu_personal * self.personal.p(arc.lm_token, h2, h1);
            if p > 0.0 {
                p.log10()
            } else {
                -12.0
            }
        } else {
            // μ_g = 1, μ_p = 0: log10(1.0 · 10^logp) == logp, skip the pow
            logp_g
        };
        score += if prev_en && arc.kind.is_english() {
            self.priors.en_continue
        } else {
            self.priors.of(arc.kind)
        };
        if arc.completed {
            score += self.priors.completion;
        }
        if arc.fuzzy {
            score += self.priors.fuzzy;
        }
        if self.lambda_lexicon > 0.0 {
            score += self.lambda_lexicon * self.personal.lexicon_bonus(arc.lm_token);
        }
        score
    }
}

#[derive(Clone, Copy)]
struct Hyp {
    score: f64,
    node: i32,     // index into the backpointer arena, -1 = path start
    last_en: bool, // last arc was an English word (run continuation)
}

struct Node<'a> {
    parent: i32,
    text: &'a str,
    pinyin: &'a str,
    en: bool, // English-word node: spaces go between adjacent ones
}

pub struct DecodeResult {
    pub text: String,
    pub preedit: String,
    pub score: f64,
}

/// N-best beam search. `bos` is the interned `<s>` id. Hypotheses ending at
/// the same position are deduped by trigram state (h2, h1), exactly like
/// decoder.py; the final beam yields up to `topn` surface-distinct results.
pub fn decode_topn(
    keys: &str,
    builder: &LatticeBuilder,
    personal_arcs: Option<PersonalMatchers>,
    scorer: &Scorer,
    bos: u32,
    beam_width: usize,
    topn: usize,
) -> Vec<DecodeResult> {
    let n = keys.len();
    if n == 0 || !keys.is_ascii() {
        return Vec::new();
    }
    let arcs = builder.build(keys, personal_arcs);
    let mut nodes: Vec<Node> = Vec::with_capacity(64);
    let mut beams: Vec<FxHashMap<(u32, u32), Hyp>> = (0..=n).map(|_| FxHashMap::default()).collect();
    beams[0].insert(
        (bos, bos),
        Hyp {
            score: 0.0,
            node: -1,
            last_en: false,
        },
    );
    let mut frontier: Vec<((u32, u32), Hyp)> = Vec::new();
    for pos in 0..n {
        if beams[pos].is_empty() {
            continue;
        }
        frontier.clear();
        frontier.extend(beams[pos].iter().map(|(&s, &h)| (s, h)));
        frontier.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
        frontier.truncate(beam_width);
        // the state key IS the trigram history (h2, h1); last_en needs no
        // extra state split — a given lm_token always has one arc kind
        for &((h2, h1), hyp) in frontier.iter() {
            for arc in &arcs[pos] {
                let s = hyp.score + scorer.arc_score(arc, h2, h1, hyp.last_en);
                let state = (h1, arc.lm_token);
                let bucket = &mut beams[arc.end];
                let insert = match bucket.get(&state) {
                    Some(cur) => s > cur.score,
                    None => true,
                };
                if insert {
                    nodes.push(Node {
                        parent: hyp.node,
                        text: arc.text,
                        pinyin: arc.pinyin,
                        en: arc.kind.is_english(),
                    });
                    bucket.insert(
                        state,
                        Hyp {
                            score: s,
                            node: (nodes.len() - 1) as i32,
                            last_en: arc.kind.is_english(),
                        },
                    );
                }
            }
        }
    }
    let mut finals: Vec<&Hyp> = beams[n].values().collect();
    finals.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut results: Vec<DecodeResult> = Vec::with_capacity(topn);
    for hyp in finals {
        let (text, preedit) = materialize(&nodes, hyp.node);
        if results.iter().any(|r| r.text == text) {
            continue;
        }
        results.push(DecodeResult {
            text,
            preedit,
            score: hyp.score,
        });
        if results.len() >= topn {
            break;
        }
    }
    results
}

fn materialize(nodes: &[Node], mut idx: i32) -> (String, String) {
    let mut chain: Vec<&Node> = Vec::new();
    while idx >= 0 {
        let node = &nodes[idx as usize];
        chain.push(node);
        idx = node.parent;
    }
    chain.reverse();
    let mut text = String::new();
    let mut preedit = String::new();
    let mut prev_en = false;
    for node in chain {
        if prev_en && node.en {
            text.push(' '); // English run: "timewindow" -> "time window"
        }
        text.push_str(node.text);
        prev_en = node.en;
        if !preedit.is_empty() {
            preedit.push(' ');
        }
        preedit.push_str(node.pinyin);
    }
    (text, preedit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::tests::mini_artifacts;
    use crate::lm::NoPersonalLayer;

    static NO_PERSONAL: NoPersonalLayer = NoPersonalLayer;

    fn top(keys: &str, n: usize) -> Vec<DecodeResult> {
        let arts = mini_artifacts();
        let scorer = Scorer::general_only(&arts.lm, &NO_PERSONAL);
        decode_topn(keys, &arts.builder, None, &scorer, arts.bos, 12, n)
    }

    #[test]
    fn decodes_nihao_top1() {
        let r = top("nihao", 5);
        assert_eq!(r[0].text, "你好");
        assert_eq!(r[0].preedit, "ni hao");
        assert!(r.len() >= 2, "n-best should offer alternatives");
        // scores strictly ordered
        assert!(r.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[test]
    fn decodes_sentence_with_lm_context() {
        let r = top("woyaoceshi", 3);
        assert_eq!(r[0].text, "我要测试");
        assert_eq!(r[0].preedit, "wo yao ce shi");
    }

    #[test]
    fn english_arc_competes_in_lattice() {
        // "woyaoyonggan": 勇敢 (word) should beat ...yong + gan-the-English-word
        let r = top("woyaoyonggan", 3);
        assert_eq!(r[0].text, "我要勇敢");
        // standalone "test" has no pinyin reading in the mini dict -> English arc wins
        let r = top("test", 3);
        assert_eq!(r[0].text, "test");
    }

    #[test]
    fn english_run_gets_spaces_and_single_switch_cost() {
        // two consecutive English words: spaces in the surface, the second
        // word pays the cheap continuation prior, not the -3 switch cost
        let r = top("thetest", 3);
        assert_eq!(r[0].text, "the test");
        assert_eq!(r[0].preedit, "the test");
        // single English word: behavior unchanged, no stray spaces
        let r = top("test", 1);
        assert_eq!(r[0].text, "test");
    }

    #[test]
    fn unfinished_tail_completes_and_keeps_raw_variant() {
        // half-typed syllable: typing-ahead completion ranks first ("c" is
        // a prefix of ce -> 测), the literal raw-tail variant stays available
        let r = top("woyaoc", 5);
        assert_eq!(r[0].text, "我要测");
        assert_eq!(r[0].preedit, "wo yao ce");
        assert!(r.iter().any(|c| c.text == "我要c"), "raw tail must survive");
    }

    #[test]
    fn completion_finishes_words_and_english() {
        // dict word-key completion: "cesh" is a strict prefix of "ceshi"
        let r = top("woyaocesh", 1);
        assert_eq!(r[0].text, "我要测试");
        // english completion: "tes" -> test
        let r = top("tes", 5);
        assert!(r.iter().any(|c| c.text == "test"));
    }

    #[test]
    fn fuzzy_pinyin_matches_with_penalty() {
        // z/zh: typing "zongwen" still reaches 中文, preedit shows the
        // canonical reading (the correction itself)
        let r = top("zongwen", 3);
        assert_eq!(r[0].text, "中文");
        assert_eq!(r[0].preedit, "zhong wen");
        // exact spelling is unaffected and scores strictly better
        let exact = top("zhongwen", 1);
        assert_eq!(exact[0].text, "中文");
        assert!(exact[0].score > r[0].score);
    }

    #[test]
    fn fallback_keeps_lattice_connected() {
        let r = top("nivvv", 1);
        assert_eq!(r.len(), 1);
        assert!(r[0].text.starts_with("你") || r[0].text.contains('v'));
        assert!(r[0].text.ends_with("vvv"));
    }

    #[test]
    fn empty_and_non_ascii_inputs_yield_no_candidates() {
        assert!(top("", 5).is_empty());
        assert!(top("你好", 5).is_empty());
    }
}
