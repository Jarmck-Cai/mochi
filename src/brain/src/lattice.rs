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

/// One word stored under a matcher key.
pub struct Entry {
    /// Surface form emitted when the arc is taken.
    pub text: Box<str>,
    /// Space-separated pinyin syllables (for preedit); for English entries
    /// this is just the word itself.
    pub pinyin: Box<str>,
    /// Interned LM token (lowercase for English, surface for hanzi).
    pub lm_token: u32,
}

/// Exact-match dictionary with prefix-based early stopping
/// (lattice.py `StringMatcher`).
#[derive(Default)]
pub struct Matcher {
    entries: FxHashMap<Box<str>, Vec<Entry>>,
    prefixes: FxHashSet<Box<str>>,
    max_len: usize,
}

impl Matcher {
    pub fn add(&mut self, key: &str, entry: Entry) {
        if key.is_empty() {
            return;
        }
        self.entries.entry(key.into()).or_default().push(entry);
        for i in 1..=key.len() {
            if !self.prefixes.contains(&key[..i]) {
                self.prefixes.insert(key[..i].into());
            }
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
            if !self.prefixes.contains(sub) {
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

    pub fn est_bytes(&self) -> usize {
        let mut bytes = 0;
        for (k, v) in &self.entries {
            bytes += k.len() + 48; // key + map entry overhead
            for e in v {
                bytes += e.text.len() + e.pinyin.len() + 40;
            }
        }
        for p in &self.prefixes {
            bytes += p.len() + 40;
        }
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
}

/// Personal-layer matchers joining the lattice (borrowed from the
/// PersonalStore read guard for the duration of one decode).
#[derive(Clone, Copy)]
pub struct PersonalMatchers<'p> {
    pub zh: &'p Matcher,
    pub en: &'p Matcher,
}

pub struct LatticeBuilder {
    pub char_matcher: Matcher,
    pub word_matcher: Matcher,
    pub en_matcher: Matcher,
    /// Interned ids for single letters a-z (fallback arcs), index = letter - b'a'.
    pub letter_tokens: [u32; 26],
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
                    });
                }
            });
            if out.is_empty() {
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
                });
            }
        }
        // The maximal all-fallback suffix is an *unfinished* pinyin tail (the
        // user is still typing): those letters stay raw cheaply instead of
        // being force-read as junk short English words or odd char splits
        // ("woyaoc" must show 我要c, not 我压oc).
        for pos in (0..n).rev() {
            if !arcs[pos].iter().all(|a| a.kind == ArcKind::Fallback) {
                break;
            }
            for arc in arcs[pos].iter_mut() {
                arc.kind = ArcKind::FallbackTail;
            }
        }
        arcs
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
