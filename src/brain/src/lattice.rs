//! Lattice construction over the key stream (port of pipeline/lattice.py).
//!
//! For the current keys we lay three kinds of arcs and let one scoring
//! function arbitrate in the decoder (DESIGN.md §3, the mixed-input core):
//!   - pinyin arcs: dictionary words whose concatenated toneless pinyin
//!     matches a substring (char arcs guarantee any pinyin stream decodes);
//!   - English word arcs (general vocabulary; the personal lexicon joins
//!     in M2-3 via the same matcher type);
//!   - fallback arcs: single raw letters with a large penalty, keeping the
//!     lattice connected for arbitrary letter sequences.

use rustc_hash::{FxHashMap, FxHashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcKind {
    PyWord,       // multi-char dictionary word via pinyin
    PyChar,       // single hanzi via pinyin
    PyPersonal,   // personal-lexicon Chinese word (OOV in the static dict)
    EnWord,       // general-vocabulary English word
    EnPersonal,   // personal-lexicon English word (user's own terms/casing)
    Fallback,     // single raw letter mid-stream, last resort
    FallbackTail, // raw letter in the unfinished tail (cheap: typing goes on)
}

impl ArcKind {
    /// English-word arcs form runs: "timewindow" -> "time window" (spaces
    /// between them, mode-switch cost paid once).
    #[inline]
    pub fn is_english(self) -> bool {
        matches!(self, ArcKind::EnWord | ArcKind::EnPersonal)
    }
}

/// One word stored under a matcher key.
pub struct Entry {
    /// Surface form emitted when the arc is taken.
    pub text: Box<str>,
    /// Space-separated pinyin syllables (for preedit); for English entries
    /// this is just the word itself. Always the CANONICAL reading — a fuzzy
    /// entry shows the correct pinyin, which is the correction itself.
    pub pinyin: Box<str>,
    /// Interned LM token (lowercase for English, surface for hanzi).
    pub lm_token: u32,
    /// Stored under a fuzzy-variant key (in/ing, z/zh...): scored with a
    /// penalty so the exact spelling always outranks the tolerated one.
    pub fuzzy: bool,
}

/// FxHash of a byte slice (prefix-set key). Storing 8-byte hashes instead
/// of owned strings keeps the 4.5M-prefix set (essay-sized dict) cheap to
/// build and query; a collision only costs one extra entries.get miss.
#[inline]
fn prefix_hash(bytes: &[u8]) -> u64 {
    use std::hash::Hasher;
    let mut h = rustc_hash::FxHasher::default();
    h.write(bytes);
    h.finish()
}

/// Exact-match dictionary with prefix-based early stopping
/// (lattice.py `StringMatcher`).
#[derive(Default)]
pub struct Matcher {
    entries: FxHashMap<Box<str>, Vec<Entry>>,
    prefixes: FxHashSet<u64>,
    max_len: usize,
}

impl Matcher {
    pub fn add(&mut self, key: &str, entry: Entry) {
        if key.is_empty() {
            return;
        }
        self.entries.entry(key.into()).or_default().push(entry);
        for i in 1..=key.len() {
            self.prefixes.insert(prefix_hash(&key.as_bytes()[..i]));
        }
        self.max_len = self.max_len.max(key.len());
    }

    /// Call `f(end, entries)` for every key matching `keys[start..end]`.
    /// Entries borrow `&'a self` so arcs can reference them directly.
    #[inline]
    pub fn for_matches<'a>(
        &'a self,
        keys: &str,
        start: usize,
        mut f: impl FnMut(usize, &'a [Entry]),
    ) {
        let limit = keys.len().min(start + self.max_len);
        for end in (start + 1)..=limit {
            let sub = &keys[start..end];
            if !self.prefixes.contains(&prefix_hash(sub.as_bytes())) {
                break;
            }
            if let Some(entries) = self.entries.get(sub) {
                f(end, entries);
            }
        }
    }

    /// Update the surface form of an existing entry (the personal layer
    /// tracks the user's preferred casing, which can change over time).
    pub fn set_text(&mut self, key: &str, lm_token: u32, text: &str) {
        if let Some(entries) = self.entries.get_mut(key) {
            for e in entries.iter_mut() {
                if e.lm_token == lm_token {
                    e.text = text.into();
                }
            }
        }
    }

    /// Entries stored under exactly `key`.
    pub fn get(&self, key: &str) -> Option<&[Entry]> {
        self.entries.get(key).map(|v| v.as_slice())
    }

    /// All keys (for building sorted completion indexes at load).
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(|k| k.as_ref())
    }

    /// Linear prefix scan for SMALL matchers (the personal layer): calls
    /// `f(key, entries)` for every key strictly extending `prefix`.
    pub fn for_completions<'a>(
        &'a self,
        prefix: &str,
        mut f: impl FnMut(&'a str, &'a [Entry]),
    ) {
        if prefix.is_empty() {
            return;
        }
        for (k, v) in &self.entries {
            if k.len() > prefix.len() && k.starts_with(prefix) {
                f(k, v);
            }
        }
    }

    pub fn est_bytes(&self) -> usize {
        let mut bytes = 0;
        for (k, v) in &self.entries {
            bytes += k.len() + 48; // key + map entry overhead
            for e in v {
                bytes += e.text.len() + e.pinyin.len() + 40;
            }
        }
        bytes += self.prefixes.len() * 10; // u64 hash + set overhead
        bytes
    }
}

/// One lattice arc; borrows surfaces from the engine ('a) so building a
/// lattice allocates only the per-position Vecs.
pub struct Arc<'a> {
    pub end: usize,
    pub text: &'a str,
    /// Space-separated syllables (pinyin arcs) or raw letters (en/fallback).
    pub pinyin: &'a str,
    pub lm_token: u32,
    pub kind: ArcKind,
    /// The key stream only PREFIXES this word — the arc completes the rest
    /// ("congrat" -> congratulations, "shangxiaw" -> 上下文). Penalized;
    /// surfaces in the candidate list as a typing-ahead suggestion.
    pub completed: bool,
    /// Matched through a fuzzy-pinyin variant (in/ing, z/zh...).
    pub fuzzy: bool,
}

/// Personal-layer matchers joining the lattice (borrowed from the
/// PersonalStore read guard for the duration of one decode).
#[derive(Clone, Copy)]
pub struct PersonalMatchers<'p> {
    pub zh: &'p Matcher,
    pub en: &'p Matcher,
}

/// Completion limits: a prefix must be constraining enough to complete
/// (an alphabetical range scan over thousands of keys would surface junk),
/// and each source contributes a bounded number of arcs — the LM ranks.
const COMPLETE_MIN_PREFIX: usize = 3; // static word/english completion
const COMPLETE_MIN_PREFIX_PERSONAL: usize = 2; // personal words are few and precious
const COMPLETE_RANGE_CAP: usize = 300; // skip unconstrained prefixes entirely
const COMPLETE_WORDS_CAP: usize = 6;
const COMPLETE_EN_CAP: usize = 12;
const COMPLETE_CHARS_PER_SYL: usize = 4;
const COMPLETE_PERSONAL_CAP: usize = 4;

pub struct LatticeBuilder {
    pub char_matcher: Matcher,
    pub word_matcher: Matcher,
    pub en_matcher: Matcher,
    /// Interned ids for single letters a-z (fallback arcs), index = letter - b'a'.
    pub letter_tokens: [u32; 26],
    /// Sorted canonical keys for completion lookups (binary search + scan).
    pub word_key_index: Vec<Box<str>>,
    pub en_key_index: Vec<Box<str>>,
    pub syllable_index: Vec<Box<str>>,
}

/// Iterate keys in `index` that strictly extend `prefix`; gives up (calls
/// nothing) when the range exceeds COMPLETE_RANGE_CAP — too unconstrained.
fn for_prefix_range<'a>(
    index: &'a [Box<str>],
    prefix: &str,
    mut f: impl FnMut(&'a str) -> bool, // return false to stop early
) {
    let start = index.partition_point(|k| k.as_ref() < prefix);
    let mut end = start;
    while end < index.len() && end - start <= COMPLETE_RANGE_CAP && index[end].starts_with(prefix) {
        end += 1;
    }
    if end - start > COMPLETE_RANGE_CAP {
        return;
    }
    for k in &index[start..end] {
        if k.len() > prefix.len() && !f(k) {
            break;
        }
    }
}

impl LatticeBuilder {
    /// Arcs grouped by start byte position; every position is covered.
    /// `keys` must be ASCII (the IME segment is always a-z in practice).
    /// `personal` adds the user's own words (lattice.py: personal English
    /// arcs shadow same-span general ones; personal Chinese words are
    /// dict-OOV by construction, so no dedup is needed there).
    pub fn build<'a>(
        &'a self,
        keys: &'a str,
        personal: Option<PersonalMatchers<'a>>,
    ) -> Vec<Vec<Arc<'a>>> {
        let n = keys.len();
        let mut arcs: Vec<Vec<Arc<'a>>> = (0..n).map(|_| Vec::new()).collect();
        let mut seen_en: Vec<(usize, u32)> = Vec::new();
        for i in 0..n {
            let out = &mut arcs[i];
            self.char_matcher.for_matches(keys, i, |end, entries| {
                for e in entries {
                    out.push(Arc {
                        end,
                        text: &e.text,
                        pinyin: &e.pinyin,
                        lm_token: e.lm_token,
                        kind: ArcKind::PyChar,
                        completed: false,
                        fuzzy: e.fuzzy,
                    });
                }
            });
            self.word_matcher.for_matches(keys, i, |end, entries| {
                for e in entries {
                    out.push(Arc {
                        end,
                        text: &e.text,
                        pinyin: &e.pinyin,
                        lm_token: e.lm_token,
                        kind: ArcKind::PyWord,
                        completed: false,
                        fuzzy: e.fuzzy,
                    });
                }
            });
            seen_en.clear();
            if let Some(p) = personal {
                p.zh.for_matches(keys, i, |end, entries| {
                    for e in entries {
                        out.push(Arc {
                            end,
                            text: &e.text,
                            pinyin: &e.pinyin,
                            lm_token: e.lm_token,
                            kind: ArcKind::PyPersonal,
                            completed: false,
                            fuzzy: e.fuzzy,
                        });
                    }
                });
                p.en.for_matches(keys, i, |end, entries| {
                    for e in entries {
                        out.push(Arc {
                            end,
                            text: &e.text,
                            pinyin: &keys[i..end],
                            lm_token: e.lm_token,
                            kind: ArcKind::EnPersonal,
                            completed: false,
                            fuzzy: e.fuzzy,
                        });
                        seen_en.push((end, e.lm_token));
                    }
                });
            }
            self.en_matcher.for_matches(keys, i, |end, entries| {
                for e in entries {
                    if seen_en.contains(&(end, e.lm_token)) {
                        continue;
                    }
                    out.push(Arc {
                        end,
                        text: &e.text,
                        pinyin: &keys[i..end],
                        lm_token: e.lm_token,
                        kind: ArcKind::EnWord,
                        completed: false,
                        fuzzy: e.fuzzy,
                    });
                }
            });
            self.push_completions(keys, i, personal, out);
            // completion arcs are speculative: the position still needs its
            // literal fallback so the raw-tail candidate (我要c) survives
            if out.iter().all(|a| a.completed) {
                let b = keys.as_bytes()[i];
                let lm_token = if b.is_ascii_lowercase() {
                    self.letter_tokens[(b - b'a') as usize]
                } else {
                    crate::interner::UNK
                };
                out.push(Arc {
                    end: i + 1,
                    text: &keys[i..i + 1],
                    pinyin: &keys[i..i + 1],
                    lm_token,
                    kind: ArcKind::Fallback,
                    completed: false,
                    fuzzy: false,
                });
            }
        }
        // The maximal suffix where nothing matched is an *unfinished* pinyin
        // tail (the user is still typing): those letters stay raw cheaply
        // instead of being force-read as junk short English words or odd
        // char splits ("woyaoc" must show 我要c, not 我压oc).
        for pos in (0..n).rev() {
            if !arcs[pos]
                .iter()
                .filter(|a| !a.completed)
                .all(|a| a.kind == ArcKind::Fallback)
            {
                break;
            }
            for arc in arcs[pos].iter_mut() {
                if arc.kind == ArcKind::Fallback {
                    arc.kind = ArcKind::FallbackTail;
                }
            }
        }
        arcs
    }

    /// Typing-ahead arcs: when the remaining keys `keys[i..]` are a strict
    /// prefix of a syllable / word key / English word / personal word, lay
    /// a penalized arc completing it to the end of input. This is the
    /// candidate-window stage of prediction: "congrat" proposes
    /// congratulations, "shangxiaw" proposes 上下文.
    fn push_completions<'a>(
        &'a self,
        keys: &'a str,
        i: usize,
        personal: Option<PersonalMatchers<'a>>,
        out: &mut Vec<Arc<'a>>,
    ) {
        let n = keys.len();
        let remainder = &keys[i..];
        // trailing partial syllable -> its top chars ("w" -> 文/我/万...)
        if remainder.len() <= 5 {
            for_prefix_range(&self.syllable_index, remainder, |syl| {
                if let Some(entries) = self.char_matcher.get(syl) {
                    for e in entries.iter().filter(|e| !e.fuzzy).take(COMPLETE_CHARS_PER_SYL) {
                        out.push(Arc {
                            end: n,
                            text: &e.text,
                            pinyin: &e.pinyin,
                            lm_token: e.lm_token,
                            kind: ArcKind::PyChar,
                            completed: true,
                            fuzzy: false,
                        });
                    }
                }
                true
            });
        }
        if remainder.len() >= COMPLETE_MIN_PREFIX {
            // dictionary word keys ("shangxiaw" -> 上下文)
            let mut budget = COMPLETE_WORDS_CAP;
            for_prefix_range(&self.word_key_index, remainder, |key| {
                if let Some(entries) = self.word_matcher.get(key) {
                    if let Some(e) = entries.iter().find(|e| !e.fuzzy) {
                        out.push(Arc {
                            end: n,
                            text: &e.text,
                            pinyin: &e.pinyin,
                            lm_token: e.lm_token,
                            kind: ArcKind::PyWord,
                            completed: true,
                            fuzzy: false,
                        });
                        budget -= 1;
                    }
                }
                budget > 0
            });
            // English vocabulary ("congrat" -> congratulations)
            let mut budget = COMPLETE_EN_CAP;
            for_prefix_range(&self.en_key_index, remainder, |key| {
                if let Some(entries) = self.en_matcher.get(key) {
                    if let Some(e) = entries.iter().find(|e| !e.fuzzy) {
                        out.push(Arc {
                            end: n,
                            text: &e.text,
                            pinyin: &e.pinyin,
                            lm_token: e.lm_token,
                            kind: ArcKind::EnWord,
                            completed: true,
                            fuzzy: false,
                        });
                        budget -= 1;
                    }
                }
                budget > 0
            });
        }
        // personal words: few, high-value, allowed at shorter prefixes
        if remainder.len() >= COMPLETE_MIN_PREFIX_PERSONAL {
            if let Some(p) = personal {
                let mut left = COMPLETE_PERSONAL_CAP;
                p.zh.for_completions(remainder, |_k, entries| {
                    for e in entries.iter().take(1) {
                        if left > 0 {
                            out.push(Arc {
                                end: n,
                                text: &e.text,
                                pinyin: &e.pinyin,
                                lm_token: e.lm_token,
                                kind: ArcKind::PyPersonal,
                                completed: true,
                                fuzzy: false,
                            });
                            left -= 1;
                        }
                    }
                });
                let mut left = COMPLETE_PERSONAL_CAP;
                p.en.for_completions(remainder, |_k, entries| {
                    for e in entries.iter().take(1) {
                        if left > 0 {
                            out.push(Arc {
                                end: n,
                                text: &e.text,
                                pinyin: &e.pinyin,
                                lm_token: e.lm_token,
                                kind: ArcKind::EnPersonal,
                                completed: true,
                                fuzzy: false,
                            });
                            left -= 1;
                        }
                    }
                });
            }
        }
    }
}

/// Pinyin syllable inventory derived from the dictionary keys; gives the
/// canonical syllable segmentation used for preedit sanity checks/tests.
#[derive(Default)]
pub struct SyllableSet {
    syllables: FxHashSet<Box<str>>,
    max_len: usize,
}

impl SyllableSet {
    pub fn add(&mut self, syl: &str) {
        if syl.is_empty() {
            return;
        }
        self.max_len = self.max_len.max(syl.len());
        if !self.syllables.contains(syl) {
            self.syllables.insert(syl.into());
        }
    }

    pub fn len(&self) -> usize {
        self.syllables.len()
    }

    /// Sorted copy for the completion index.
    pub fn to_sorted(&self) -> Vec<Box<str>> {
        let mut v: Vec<Box<str>> = self.syllables.iter().cloned().collect();
        v.sort_unstable();
        v
    }

    /// Longest-match-first segmentation with backtracking; None when the
    /// stream is not a pure pinyin syllable sequence.
    #[allow(dead_code)] // exercised in tests; production preedit comes from arcs
    pub fn segment<'a>(&self, keys: &'a str) -> Option<Vec<&'a str>> {
        fn go<'a>(set: &SyllableSet, keys: &'a str, pos: usize, acc: &mut Vec<&'a str>) -> bool {
            if pos == keys.len() {
                return true;
            }
            let limit = keys.len().min(pos + set.max_len);
            for end in ((pos + 1)..=limit).rev() {
                let sub = &keys[pos..end];
                if set.syllables.contains(sub) {
                    acc.push(sub);
                    if go(set, keys, end, acc) {
                        return true;
                    }
                    acc.pop();
                }
            }
            false
        }
        if !keys.is_ascii() {
            return None;
        }
        let mut acc = Vec::new();
        go(self, keys, 0, &mut acc).then_some(acc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::tests::mini_artifacts;

    #[test]
    fn syllable_segmentation_longest_match_with_backtracking() {
        let mut s = SyllableSet::default();
        for syl in ["ni", "hao", "zhong", "wen", "wo", "yao", "ce", "shi", "xi", "an", "xian"] {
            s.add(syl);
        }
        assert_eq!(s.segment("nihao").unwrap(), vec!["ni", "hao"]);
        assert_eq!(s.segment("zhongwen").unwrap(), vec!["zhong", "wen"]);
        // longest-first picks "xian" over "xi an"
        assert_eq!(s.segment("xian").unwrap(), vec!["xian"]);
        // backtracking: "xiani" = xi-an-... no; xian+i fails -> xi an i? "i" missing -> None
        assert!(s.segment("xiani").is_none());
        assert!(s.segment("nih").is_none());
        assert!(s.segment("gan").is_none()); // not in this mini inventory
    }

    #[test]
    fn lattice_lays_char_word_english_arcs() {
        let arts = mini_artifacts();
        let arcs = arts.builder.build("nihao", None);
        let at0: Vec<(&str, ArcKind, usize)> =
            arcs[0].iter().map(|a| (a.text, a.kind, a.end)).collect();
        // char arc 你 over "ni" and word arc 你好 over "nihao"
        assert!(at0.contains(&("你", ArcKind::PyChar, 2)));
        assert!(at0.contains(&("你好", ArcKind::PyWord, 5)));
        // pinyin field carries the spaced split for preedit
        let nihao = arcs[0].iter().find(|a| a.text == "你好").unwrap();
        assert_eq!(nihao.pinyin, "ni hao");

        // English arc: "gan" is both pinyin (敢) and an English word
        let arcs = arts.builder.build("gan", None);
        let kinds: Vec<(&str, ArcKind)> = arcs[0].iter().map(|a| (a.text, a.kind)).collect();
        assert!(kinds.contains(&("敢", ArcKind::PyChar)));
        assert!(kinds.contains(&("gan", ArcKind::EnWord)));
    }

    #[test]
    fn personal_arcs_join_and_shadow_general_english() {
        let arts = mini_artifacts();
        let mut zh = Matcher::default();
        zh.add(
            "suiji",
            Entry {
                text: "随机".into(),
                pinyin: "sui ji".into(),
                lm_token: 900,
                fuzzy: false,
            },
        );
        let mut en = Matcher::default();
        // same lm_token as the general "test" entry -> general arc shadowed
        let test_token = arts
            .builder
            .build("test", None)[0]
            .iter()
            .find(|a| a.kind == ArcKind::EnWord && a.text == "test")
            .expect("general english arc for 'test'")
            .lm_token;
        en.add(
            "test",
            Entry {
                text: "TEST".into(),
                pinyin: "test".into(),
                lm_token: test_token,
                fuzzy: false,
            },
        );
        let personal = PersonalMatchers { zh: &zh, en: &en };
        let arcs = arts.builder.build("test", Some(personal));
        let en_arcs: Vec<(&str, ArcKind)> = arcs[0]
            .iter()
            .filter(|a| a.end == 4)
            .map(|a| (a.text, a.kind))
            .collect();
        assert!(en_arcs.contains(&("TEST", ArcKind::EnPersonal)));
        assert!(!en_arcs.contains(&("test", ArcKind::EnWord)));

        let arcs = arts.builder.build("suiji", Some(personal));
        assert!(arcs[0]
            .iter()
            .any(|a| a.text == "随机" && a.kind == ArcKind::PyPersonal && a.end == 5));
    }

    #[test]
    fn lattice_fallback_covers_unmatched_positions() {
        let arts = mini_artifacts();
        let arcs = arts.builder.build("nivx", None); // v/x start no entry in mini dict
        assert_eq!(arcs[2].len(), 1);
        // trailing all-fallback run counts as the unfinished tail
        assert_eq!(arcs[2][0].kind, ArcKind::FallbackTail);
        assert_eq!(arcs[2][0].text, "v");
        assert_eq!(arcs[2][0].end, 3);
        // every position has at least one arc (lattice always connected)
        assert!(arcs.iter().all(|a| !a.is_empty()));
    }

    #[test]
    fn mid_stream_fallback_stays_expensive_tail_is_cheap() {
        let arts = mini_artifacts();
        // "v" mid-stream (before valid pinyin) is a real gap, not a tail
        let arcs = arts.builder.build("vni", None);
        assert_eq!(arcs[0][0].kind, ArcKind::Fallback);
        // half-typed syllable at the end is a tail
        let arcs = arts.builder.build("nihaoc", None);
        let last = arcs[5].iter().find(|a| a.text == "c").unwrap();
        assert_eq!(last.kind, ArcKind::FallbackTail);
    }
}
