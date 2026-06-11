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
    en: bool,        // English-word node: spaces go between adjacent ones
    completed: bool, // typing-ahead arc
    typed_len: u16,  // for completed arcs: key bytes the user actually typed
    fuzzy: bool,     // fuzzy-pinyin arc
    raw: bool,       // fallback letter arc (incomplete syllable)
}

pub struct DecodeResult {
    pub text: String,
    pub preedit: String,
    pub score: f64,
    /// Key bytes this candidate consumes. Less than the input length for
    /// PREFIX candidates (局部候选): selecting one commits just that span
    /// and the rest of the input keeps composing — fix one segment of a
    /// long sentence without retyping it all.
    pub len: usize,
    /// Path ends in a typing-ahead completion: the candidate contains
    /// text the user did NOT type — the UI must say so.
    pub completed: bool,
    /// The not-yet-typed part of the candidate, in TEXT space (the chars/
    /// letters the engine assumed): "cesh"→测试 gives "试", "congrat"→
    /// congratulations gives "ulations". Empty for non-completed paths.
    pub ghost: String,
    /// Path crossed a fuzzy-pinyin arc.
    pub fuzzy: bool,
    /// Path contains raw fallback letters (incomplete syllable somewhere) —
    /// fine for full candidates (unfinished tail), disqualifying for prefix
    /// candidates (a mid-syllable cut is not a meaningful repair boundary).
    pub raw: bool,
}

/// How many prefix candidates join the list, and where: the first
/// FULL_BEFORE_PREFIX full-span candidates stay on top (the whole-sentence
/// reading is the headline), prefixes follow (longest first — the natural
/// repair order), then the remaining full-span variants.
const PREFIX_CAP: usize = 6;
const FULL_BEFORE_PREFIX: usize = 3;

/// Which part of a completed arc's text the user has NOT typed yet, judged
/// by reading coverage: a hanzi whose syllable was fully typed is real,
/// the rest are ghost ("cesh" over 测试: ce complete → 测 real, shi cut at
/// sh → 试 ghost). English words are a plain byte suffix.
fn ghost_of(text: &str, spaced: &str, typed: usize) -> String {
    if text.is_ascii() {
        return text.get(typed..).unwrap_or("").to_string();
    }
    let mut budget = typed;
    let mut ghost = String::new();
    for (ch, syl) in text.chars().zip(spaced.split(' ')) {
        if budget >= syl.len() {
            budget -= syl.len();
        } else {
            ghost.push(ch);
            budget = 0;
        }
    }
    ghost
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
                        completed: arc.completed,
                        typed_len: arc.typed_len,
                        fuzzy: arc.fuzzy,
                        raw: matches!(arc.kind, ArcKind::Fallback | ArcKind::FallbackTail),
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
    let mut full: Vec<DecodeResult> = Vec::with_capacity(topn);
    for hyp in finals {
        let r = materialize(&nodes, hyp.node, hyp.score, n);
        if full.iter().any(|prev| prev.text == r.text) {
            continue;
        }
        full.push(r);
        if full.len() >= topn {
            break;
        }
    }

    // Prefix candidates (局部候选): the best hypothesis ending at each
    // earlier position, longest first. Selecting one commits only that
    // span; the remaining keys keep composing (rime re-queries them) —
    // the repair path for long sentences with one bad segment.
    //
    // Mid-syllable cuts are weeded out two ways: raw fallback letters in
    // the path, and a per-key score gate — a cut that forces a junk parse
    // ("常用词很zh on") pays visibly more per key than the top reading.
    let top_per_key = full.first().map(|f| f.score / n as f64);
    let mut prefixes: Vec<DecodeResult> = Vec::new();
    for p in (1..n).rev() {
        if prefixes.len() >= PREFIX_CAP {
            break;
        }
        let best = beams[p]
            .values()
            .max_by(|a, b| a.score.total_cmp(&b.score));
        if let Some(hyp) = best {
            let r = materialize(&nodes, hyp.node, hyp.score, p);
            let junk = match top_per_key {
                Some(t) => (r.score / p as f64) < t - 0.5,
                None => false,
            };
            if r.completed
                || r.raw
                || junk
                || full.iter().any(|f| f.text == r.text)
                || prefixes.iter().any(|q| q.text == r.text)
            {
                continue;
            }
            prefixes.push(r);
        }
    }

    // headline full readings, then repairs, then the long tail of variants
    let mut results = Vec::with_capacity(full.len() + prefixes.len());
    let rest = full.split_off(full.len().min(FULL_BEFORE_PREFIX));
    results.extend(full);
    results.extend(prefixes);
    results.extend(rest);
    results
}

fn materialize(nodes: &[Node], mut idx: i32, score: f64, len: usize) -> DecodeResult {
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
    let mut completed = false;
    let mut ghost = String::new();
    let mut fuzzy = false;
    let mut raw = false;
    for node in chain {
        if prev_en && node.en {
            text.push(' '); // English run: "timewindow" -> "time window"
        }
        text.push_str(node.text);
        prev_en = node.en;
        fuzzy |= node.fuzzy;
        raw |= node.raw;
        if !preedit.is_empty() {
            preedit.push(' ');
        }
        if node.completed {
            completed = true;
            ghost = ghost_of(node.text, node.pinyin, node.typed_len as usize);
            // boundary marker in reading space: "shang xia w‥en" — the
            // preedit line (candidate window AND inline composition) shows
            // exactly where the user's own typing ended. Display-only;
            // committed text comes from `text`.
            let mut budget = node.typed_len as usize;
            for c in node.pinyin.chars() {
                if budget == 0 {
                    preedit.push('‥');
                    budget = usize::MAX; // marker placed once
                }
                if c != ' ' && budget != usize::MAX {
                    budget -= 1;
                }
                preedit.push(c);
            }
            if budget == 0 {
                preedit.push('‥'); // typed exactly consumed (defensive)
            }
        } else {
            preedit.push_str(node.pinyin);
        }
    }
    DecodeResult {
        text,
        preedit,
        score,
        len,
        completed,
        ghost,
        fuzzy,
        raw,
    }
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
        // scores ordered among full-span candidates (prefix candidates are
        // shorter paths — their scores live on a different scale)
        let full: Vec<&DecodeResult> = r.iter().filter(|c| c.len == 5).collect();
        assert!(full.windows(2).all(|w| w[0].score >= w[1].score));
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
        // ‥ marks where the user's own typing ended (display-only)
        assert_eq!(r[0].preedit, "wo yao c‥e");
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
        // topn=1 full candidate, prefix repairs may follow
        assert!(r[0].text.starts_with("你") || r[0].text.contains('v'));
        assert!(r[0].text.ends_with("vvv"));
        assert_eq!(r[0].len, 5);
    }

    #[test]
    fn empty_and_non_ascii_inputs_yield_no_candidates() {
        assert!(top("", 5).is_empty());
        assert!(top("你好", 5).is_empty());
    }
}
