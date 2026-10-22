//! lm-artifacts-v0 loading (docs/specs/lm-artifacts-v0.md): dict.tsv +
//! ngram.tsv + english.tsv -> interned LM tables and lattice matchers.
//!
//! The TSV text format is the cross-language contract; this module is the
//! "brain build-artifacts" private side. Measured load is well under the 3s
//! threshold (see README), so no binary cache layer is needed yet.

use std::path::Path;
use std::time::Instant;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::interner::Interner;
use crate::lattice::{Entry, LatticeBuilder, Matcher, SyllableSet};
use crate::lm::BackoffTrigramLm;

/// Top-N words kept per pinyin key (lattice.py `build_pinyin_matcher`):
/// the LM does the real ranking, the dictionary just proposes candidates.
const MAX_WORDS_PER_KEY: usize = 8;
const MAX_CHARS_PER_KEY: usize = MAX_WORDS_PER_KEY * 2; // chars need more variety

pub struct LoadStats {
    pub elapsed_ms: u128,
    pub est_bytes: usize,
    pub dict_entries: usize,
    pub ngrams: (usize, usize, usize),
    pub english_words: usize,
    pub syllables: usize,
}

pub struct Artifacts {
    pub lm: BackoffTrigramLm,
    pub builder: LatticeBuilder,
    /// Syllable inventory derived from dict keys (segmentation checks/tests;
    /// candidates derive preedit from their own arcs).
    #[allow(dead_code)]
    pub syllables: SyllableSet,
    pub bos: u32,
    pub interner: Interner,
    /// Tokens that already have a lattice arc via the static dictionary;
    /// the personal layer only adds arcs for words outside this set.
    pub dict_tokens: FxHashSet<u32>,
    /// hanzi -> known toneless syllables (longest first), built from all
    /// single-char dict entries; used to align commit text with raw keys.
    pub char_pinyin: FxHashMap<char, Vec<Box<str>>>,
    pub stats: LoadStats,
}

impl Artifacts {
    pub fn load(dir: &Path) -> Result<Artifacts, String> {
        let read = |name: &str| -> Result<String, String> {
            std::fs::read_to_string(dir.join(name))
                .map_err(|e| format!("read {}: {}", dir.join(name).display(), e))
        };
        let dict = read("dict.tsv")?;
        let ngram = read("ngram.tsv")?;
        let english = read("english.tsv")?;
        Self::from_strs(&dict, &ngram, &english)
    }

    pub fn from_strs(dict: &str, ngram: &str, english: &str) -> Result<Artifacts, String> {
        let t0 = Instant::now();
        let mut interner = Interner::new();
        let bos = interner.intern("<s>");

        let mut lm = parse_ngram(ngram, &mut interner)?;
        let DictTables {
            char_matcher,
            word_matcher,
            syllables,
            dict_entries,
            dict_tokens,
            char_pinyin,
        } = parse_dict(dict, &mut interner)?;
        let (en_matcher, english_words) = parse_english(english, &mut interner)?;

        let mut letter_tokens = [0u32; 26];
        for (i, t) in letter_tokens.iter_mut().enumerate() {
            let letter = (b'a' + i as u8) as char;
            *t = interner.intern(letter.encode_utf8(&mut [0u8; 4]));
        }

        // completion indexes: canonical (non-fuzzy) keys only — completing
        // through a fuzzy variant would stack two speculations
        let mut word_key_index: Vec<Box<str>> = word_matcher
            .keys()
            .filter(|k| {
                word_matcher
                    .get(k)
                    .is_some_and(|es| es.iter().any(|e| !e.fuzzy))
            })
            .map(Into::into)
            .collect();
        word_key_index.sort_unstable();
        let mut en_key_index: Vec<Box<str>> = en_matcher.keys().map(Into::into).collect();
        en_key_index.sort_unstable();
        let syllable_index = syllables.to_sorted();

        lm.finalize(interner.len());

        let ngrams = lm.stats();
        let est_bytes = lm.est_bytes()
            + interner.est_bytes()
            + char_matcher.est_bytes()
            + word_matcher.est_bytes()
            + en_matcher.est_bytes();
        let stats = LoadStats {
            elapsed_ms: t0.elapsed().as_millis(),
            est_bytes,
            dict_entries,
            ngrams,
            english_words,
            syllables: syllables.len(),
        };
        Ok(Artifacts {
            lm,
            builder: LatticeBuilder {
                char_matcher,
                word_matcher,
                en_matcher,
                letter_tokens,
                word_key_index,
                en_key_index,
                syllable_index,
            },
            syllables,
            bos,
            interner,
            dict_tokens,
            char_pinyin,
            stats,
        })
    }
}

/// ngram.tsv: `order<TAB>token1[ token2[ token3]]<TAB>count`; header comments
/// carry `total_tokens=<N>` and `backoff=stupid:<alpha>`.
fn parse_ngram(text: &str, interner: &mut Interner) -> Result<BackoffTrigramLm, String> {
    let mut alpha: Option<f64> = None;
    let mut total: Option<f64> = None;
    for line in text.lines().take_while(|l| l.is_empty() || l.starts_with('#')) {
        if let Some(v) = line.split("total_tokens=").nth(1) {
            total = v.trim().parse().ok();
        }
        if let Some(v) = line.split("backoff=stupid:").nth(1) {
            alpha = v.trim().parse().ok();
        }
    }
    let alpha = alpha.ok_or("ngram.tsv: missing '# backoff=stupid:<alpha>' header")?;
    let total = total.ok_or("ngram.tsv: missing '# total_tokens=<N>' header")?;
    if !alpha.is_finite() {
        return Err("ngram.tsv: non-finite alpha".into());
    }
    if !total.is_finite() {
        return Err("ngram.tsv: non-finite total_tokens".into());
    }
    let mut lm = BackoffTrigramLm::new(alpha, total);
    for (no, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let (order, tokens, count) = match (cols.next(), cols.next(), cols.next(), cols.next()) {
            (Some(o), Some(t), Some(c), None) => (o, t, c),
            _ => return Err(format!("ngram.tsv line {}: bad column count", no + 1)),
        };
        let count: f32 = count
            .parse()
            .map_err(|_| format!("ngram.tsv line {}: bad count '{}'", no + 1, count))?;
        if !count.is_finite() {
            return Err(format!("ngram.tsv line {}: non-finite count", no + 1));
        }
        let expected = match order {
            "1" => 1,
            "2" => 2,
            "3" => 3,
            other => return Err(format!("ngram.tsv line {}: bad order '{}'", no + 1, other)),
        };
        // Keep empty fields: trimming or split_whitespace would hide malformed separators.
        let mut token_count = 0;
        for token in tokens.split(' ') {
            token_count += 1;
            if token.is_empty() || token_count > expected {
                return Err(format!(
                    "ngram.tsv line {}: bad tokens for order {}",
                    no + 1,
                    order
                ));
            }
        }
        if token_count != expected {
            return Err(format!(
                "ngram.tsv line {}: bad tokens for order {}",
                no + 1,
                order
            ));
        }
        let mut toks = tokens.split(' ');
        match order {
            "1" => {
                let w = interner.intern(toks.next().unwrap_or(""));
                lm.add_unigram(w, count);
            }
            "2" => {
                let h1 = interner.intern(toks.next().unwrap_or(""));
                let w = interner.intern(toks.next().unwrap_or(""));
                lm.add_bigram(h1, w, count);
            }
            "3" => {
                let h2 = interner.intern(toks.next().unwrap_or(""));
                let h1 = interner.intern(toks.next().unwrap_or(""));
                let w = interner.intern(toks.next().unwrap_or(""));
                lm.add_trigram(h2, h1, w, count);
            }
            other => return Err(format!("ngram.tsv line {}: bad order '{}'", no + 1, other)),
        }
    }
    Ok(lm)
}

struct DictTables {
    char_matcher: Matcher,
    word_matcher: Matcher,
    syllables: SyllableSet,
    dict_entries: usize,
    dict_tokens: FxHashSet<u32>,
    char_pinyin: FxHashMap<char, Vec<Box<str>>>,
}

/// Fuzzy-pinyin variants of one syllable (the common confusion pairs).
/// One rule application each — onset and rime variants are generated
/// separately, never combined.
fn fuzzy_syllable_variants(syl: &str) -> Vec<String> {
    let mut out = Vec::new();
    // onsets: zh/z, ch/c, sh/s
    for (a, b) in [("zh", "z"), ("ch", "c"), ("sh", "s")] {
        if let Some(rest) = syl.strip_prefix(a) {
            out.push(format!("{}{}", b, rest));
        } else if let Some(rest) = syl.strip_prefix(b) {
            // careful: "s" also prefixes "sh" — strip_prefix("zh") above
            // already consumed those, but "sh".strip_prefix("s") would hit
            // here; skip when the result would just undo the digraph
            if !rest.starts_with('h') {
                out.push(format!("{}{}", a, rest));
            }
        }
    }
    // rimes: ing/in, eng/en, ang/an
    for (long, short) in [("ing", "in"), ("eng", "en"), ("ang", "an")] {
        if let Some(stem) = syl.strip_suffix(long) {
            out.push(format!("{}{}", stem, short));
        } else if let Some(stem) = syl.strip_suffix(short) {
            out.push(format!("{}{}", stem, long));
        }
    }
    out
}

/// Variant CONCAT keys for a spaced pinyin key, changing one syllable per
/// variant (multi-syllable typos are almost always single-syllable slips;
/// full combination would explode the matcher).
fn fuzzy_variant_keys(spaced: &str) -> Vec<String> {
    let syls: Vec<&str> = spaced.split(' ').collect();
    let mut out = Vec::new();
    for (j, syl) in syls.iter().enumerate() {
        for variant in fuzzy_syllable_variants(syl) {
            let mut key = String::new();
            for (k, s) in syls.iter().enumerate() {
                if k == j {
                    key.push_str(&variant);
                } else {
                    key.push_str(s);
                }
            }
            out.push(key);
        }
    }
    out
}

/// dict.tsv: `key<TAB>text<TAB>weight`, key = space-separated syllables.
/// Splits entries into char/word matchers keeping the top-N per pinyin key
/// by (weight desc, text desc), mirroring lattice.py.
fn parse_dict(text: &str, interner: &mut Interner) -> Result<DictTables, String> {
    type Bucket = FxHashMap<String, Vec<(i64, Box<str>, Box<str>)>>; // concat key -> (weight, text, spaced)
    let mut chars: Bucket = FxHashMap::default();
    let mut words: Bucket = FxHashMap::default();
    let mut syllables = SyllableSet::default();
    let mut char_pinyin: FxHashMap<char, Vec<Box<str>>> = FxHashMap::default();
    let mut n = 0usize;
    for (no, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let (key, word, weight) = match (cols.next(), cols.next(), cols.next(), cols.next()) {
            (Some(k), Some(w), Some(c), None) => (k, w, c),
            _ => return Err(format!("dict.tsv line {}: bad column count", no + 1)),
        };
        if word.is_empty() {
            return Err(format!("dict.tsv line {}: empty text", no + 1));
        }
        let weight: i64 = weight.parse::<f64>().map(|f| f as i64).unwrap_or(0);
        for syl in key.split(' ') {
            if syl.is_empty() {
                return Err(format!("dict.tsv line {}: empty pinyin syllable", no + 1));
            }
            syllables.add(syl);
        }
        let concat: String = key.split(' ').collect();
        let target = if word.chars().count() == 1 {
            // reading inventory uses ALL single-char entries (no top-N cap):
            // alignment must recognize rare readings the lattice may not offer
            let c = word.chars().next().unwrap();
            let readings = char_pinyin.entry(c).or_default();
            if !readings.iter().any(|s| s.as_ref() == concat.as_str()) {
                readings.push(concat.as_str().into());
            }
            &mut chars
        } else {
            &mut words
        };
        target
            .entry(concat)
            .or_default()
            .push((weight, word.into(), key.into()));
        n += 1;
    }
    // longest-first so greedy alignment prefers maximal syllables
    for readings in char_pinyin.values_mut() {
        readings.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    }
    let mut char_m = Matcher::default();
    let mut word_m = Matcher::default();
    let mut dict_tokens = FxHashSet::default();
    for (bucket, matcher, cap) in [
        (chars, &mut char_m, MAX_CHARS_PER_KEY),
        (words, &mut word_m, MAX_WORDS_PER_KEY),
    ] {
        for (key, mut cands) in bucket {
            cands.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
            cands.truncate(cap);
            for (_, text, spaced) in cands {
                let lm_token = interner.intern(&text);
                dict_tokens.insert(lm_token);
                // fuzzy-pinyin variant keys (one syllable changed each);
                // the entry keeps the canonical reading — showing the right
                // pinyin in the preedit IS the correction
                for vkey in fuzzy_variant_keys(&spaced) {
                    matcher.add(
                        &vkey,
                        Entry {
                            text: text.clone(),
                            pinyin: spaced.clone(),
                            lm_token,
                            fuzzy: true,
                        },
                    );
                }
                matcher.add(
                    &key,
                    Entry {
                        text,
                        pinyin: spaced,
                        lm_token,
                        fuzzy: false,
                    },
                );
            }
        }
    }
    Ok(DictTables {
        char_matcher: char_m,
        word_matcher: word_m,
        syllables,
        dict_entries: n,
        dict_tokens,
        char_pinyin,
    })
}

/// english.tsv: `word<TAB>rank`. Match keys are lowercase, len >= 2,
/// alphabetic (lattice.py `build_english_matcher`); rank is unused for now
/// (English arcs get a flat prior, the LM arbitrates).
fn parse_english(text: &str, interner: &mut Interner) -> Result<(Matcher, usize), String> {
    let mut m = Matcher::default();
    let mut n = 0usize;
    for (no, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let word = match (cols.next(), cols.next(), cols.next()) {
            (Some(word), Some(_rank), None) => word,
            _ => return Err(format!("english.tsv line {}: bad column count", no + 1)),
        };
        let lower = word.to_ascii_lowercase();
        if lower.len() < 2 || !lower.bytes().all(|b| b.is_ascii_lowercase()) {
            continue;
        }
        let lm_token = interner.intern(&lower);
        m.add(
            &lower.clone(),
            Entry {
                text: word.into(), // surface keeps source casing
                pinyin: lower.into(),
                lm_token,
                fuzzy: false,
            },
        );
        n += 1;
    }
    Ok((m, n))
}

#[cfg(test)]
pub(crate) mod tests {
    fn assert_dict_tone_digit_rejected(tone: u8) {
        let dict = format!("ni{tone}\t你\t1\n");
        let ngram = "# total_tokens=1\n# backoff=stupid:0.4\n";
        assert!(
            Artifacts::from_strs(&dict, ngram, "hello\t1\n").is_err(),
            "accepted tone-bearing dictionary key ni{tone}"
        );
    }

    #[test]
    fn dict_tone_digit_1_is_rejected() {
        assert_dict_tone_digit_rejected(1);
    }

    #[test]
    fn dict_tone_digit_2_is_rejected() {
        assert_dict_tone_digit_rejected(2);
    }

    #[test]
    fn dict_tone_digit_3_is_rejected() {
        assert_dict_tone_digit_rejected(3);
    }

    #[test]
    fn dict_tone_digit_4_is_rejected() {
        assert_dict_tone_digit_rejected(4);
    }

    #[test]
    fn dict_tone_digit_5_is_rejected() {
        assert_dict_tone_digit_rejected(5);
    }

    #[test]
    fn dict_tone_digits_report_actual_line_in_multisyllable_keys() {
        let ngram = "# total_tokens=1\n# backoff=stupid:0.4\n";
        let mut failures = Vec::new();
        for tone in 1..=5 {
            for key in [format!("ni{tone} hao"), format!("ni hao{tone}")] {
                // Physical line numbers include comments, blanks, and valid data.
                let dict = format!("# fixture\n\nni\t你\t1\n# comment\n\n{key}\t你好\t1\n");
                match Artifacts::from_strs(&dict, ngram, "hello\t1\n") {
                    Ok(_) => failures.push(format!("accepted tone-bearing key {key:?}")),
                    Err(error) => {
                        if !error.contains("dict.tsv line 6:") {
                            failures.push(format!("wrong location for {key:?}: {error}"));
                        }
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn dict_toneless_keys_preserve_loading_and_matchers() {
        let ngram = "# total_tokens=1\n# backoff=stupid:0.4\n";
        let dict = "# fixture\n\nni\t你\t1\nni hao\t你好\t1\nlv\t绿\t1\n";
        let artifacts = Artifacts::from_strs(dict, ngram, "hello\t1\n")
            .expect("toneless dictionary keys must load");
        assert_eq!(artifacts.stats.dict_entries, 3);
        for (key, text) in [("ni", "你"), ("lv", "绿")] {
            let entries = artifacts.builder.char_matcher.get(key).expect("char key");
            assert_eq!(entries[0].text.as_ref(), text);
            assert_eq!(entries[0].pinyin.as_ref(), key);
        }
        let entries = artifacts.builder.word_matcher.get("nihao").expect("word key");
        assert_eq!(entries[0].text.as_ref(), "你好");
        assert_eq!(entries[0].pinyin.as_ref(), "ni hao");
    }

    use super::*;

    #[test]
    fn dict_empty_text_is_rejected_at_actual_line() {
        let dict = "# fixture\n\nni\t\t1\n";
        let ngram = "# total_tokens=100\n# backoff=stupid:0.4\n1\thello\t2.5\n";
        let result = Artifacts::from_strs(dict, ngram, "hello\t1\n");
        assert!(result.is_err(), "accepted empty text in dict.tsv line 3");
        let error = result.err().unwrap();
        assert!(error.contains("dict.tsv line 3:"), "wrong location: {error}");
    }

    fn assert_dict_empty_syllable_rejected(key: &str) {
        let ngram = "# total_tokens=100\n# backoff=stupid:0.4\n1\thello\t2.5\n";
        let dict = format!("# fixture\n\nni\t你\t3\n# comment\n\n{key}\t你好\t1\n");
        let result = Artifacts::from_strs(&dict, ngram, "hello\t1\n");
        assert!(result.is_err(), "accepted empty syllable in dict.tsv line 6: {key:?}");
        let error = result.err().unwrap();
        assert!(error.contains("dict.tsv line 6:"), "wrong location for {key:?}: {error}");
    }

    #[test]
    fn dict_empty_key_is_rejected() {
        assert_dict_empty_syllable_rejected("");
    }

    #[test]
    fn dict_leading_empty_syllable_is_rejected() {
        assert_dict_empty_syllable_rejected(" ni");
    }

    #[test]
    fn dict_trailing_empty_syllable_is_rejected() {
        assert_dict_empty_syllable_rejected("ni ");
    }

    #[test]
    fn dict_internal_empty_syllable_is_rejected() {
        assert_dict_empty_syllable_rejected("ni  hao");
    }

    #[test]
    fn dict_valid_syllable_keys_preserve_loading() {
        let ngram = "# total_tokens=100\n# backoff=stupid:0.4\n1\thello\t2.5\n";
        for weight in ["3", "2.5", "-1.5", "NaN", "inf", "invalid", ""] {
            let dict = format!("# fixture\n\nni\t你\t{weight}\nni hao\t你好\t{weight}\n\n# end\n");
            let artifacts = Artifacts::from_strs(&dict, ngram, "hello\t1\n")
                .unwrap_or_else(|error| panic!("valid keys with weight {weight:?}: {error}"));
            assert_eq!(artifacts.stats.dict_entries, 2);
            assert_eq!(artifacts.builder.char_matcher.get("ni").unwrap()[0].pinyin.as_ref(), "ni");
            assert_eq!(artifacts.builder.word_matcher.get("nihao").unwrap()[0].pinyin.as_ref(), "ni hao");
        }
    }

    #[test]
    fn ngram_effective_nonfinite_alpha_is_rejected() {
        let mut failures = Vec::new();
        for alpha in ["NaN", "inf", "-inf", "1e309", "-1e309"] {
            for header in [
                format!("# total_tokens=100\n# backoff=stupid:{alpha}\n"),
                format!("\r\n# fixture\r\n# total_tokens=100\r\n\r\n# backoff=stupid:0.4\r\n# backoff=stupid:{alpha}\r\n\r\n"),
            ] {
                let ngram = format!("{header}1\thello\t2.5\n# backoff=stupid:0.4\n");
                match Artifacts::from_strs("ni\t你\t1\n", &ngram, "hello\t1\n") {
                    Ok(_) => failures.push(format!("accepted effective alpha {alpha:?}: {header:?}")),
                    Err(error) => {
                        if !error.contains("ngram.tsv") || !error.contains("alpha") {
                            failures.push(format!("wrong error for alpha {alpha:?}: {error}"));
                        }
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn ngram_finite_alpha_preserves_scores_and_final_header_override() {
        for alpha in ["0", "-0.4", "0.25", "2", "1e308"] {
            for earlier in ["NaN", "inf", "-inf", "1e309", "invalid"] {
                let ngram = format!(
                    "\r\n# total_tokens=100\r\n# backoff=stupid:{earlier}\r\n\r\n# fixture\r\n# backoff=stupid:{alpha}\r\n\r\n1\thello\t2.5\r\n# backoff=stupid:inf\r\n# total_tokens=999\r\n"
                );
                let artifacts = Artifacts::from_strs("ni\t你\t1\n", &ngram, "hello\t1\n")
                    .unwrap_or_else(|error| panic!("final finite alpha {alpha}: {error}"));
                let alpha: f64 = alpha.parse().unwrap();
                assert_eq!(artifacts.lm.alpha, alpha);
                assert_eq!(artifacts.lm.total, 100.0);
                assert_eq!(artifacts.stats.ngrams, (1, 0, 0));
                let hello = artifacts.interner.get("hello").unwrap();
                let expected = 2.0 * alpha.log10() + (2.5_f64 / 100.0).log10();
                let actual = artifacts.lm.logp(hello, crate::interner::UNK, crate::interner::UNK);
                if expected.is_nan() {
                    assert!(actual.is_nan());
                } else {
                    assert_eq!(actual, expected);
                }
            }
        }
    }

    #[test]
    fn ngram_nonfinite_total_is_rejected() {
        let mut failures = Vec::new();
        for total in ["NaN", "inf", "-inf", "1e309", "-1e309"] {
            for header in [
                format!("# total_tokens={total}\n# backoff=stupid:0.4\n"),
                format!("\r\n# fixture\r\n# total_tokens=100\r\n\r\n# backoff=stupid:0.4\r\n# total_tokens={total}\r\n\r\n"),
            ] {
                let ngram = format!("{header}1\t你\t1\n# total_tokens=100\n");
                let result = Artifacts::from_strs("ni\t你\t1\n", &ngram, "hello\t1\n");
                if !result.is_err() {
                    failures.push(format!("accepted nonfinite total_tokens {total:?}: {header:?}"));
                } else {
                    let error = result.err().unwrap();
                    if !error.contains("ngram.tsv") || !error.contains("total_tokens") {
                        failures.push(format!("wrong error for total_tokens {total:?}: {error}"));
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn ngram_finite_total_preserves_loading_and_final_header_override() {
        // Finite zero and negative values retain their existing accepted range.
        for total in ["1", "2.5", "100", "1e308", "0", "-1", "-1e308"] {
            for earlier in ["NaN", "inf", "-inf", "1e309", "-1e309", "invalid"] {
                let ngram = format!(
                    "\r\n# total_tokens={earlier}\r\n\r\n# fixture\r\n# backoff=stupid:0.4\r\n# total_tokens={total}\r\n\r\n1\t你\t1\r\n# total_tokens=NaN\r\n"
                );
                let artifacts = Artifacts::from_strs("ni\t你\t1\n", &ngram, "hello\t1\n")
                    .unwrap_or_else(|error| panic!("final finite total_tokens {total}: {error}"));
                assert_eq!(artifacts.lm.total, total.parse::<f64>().unwrap());
                assert_eq!(artifacts.lm.alpha, 0.4);
                assert_eq!(artifacts.stats.ngrams, (1, 0, 0));
            }
        }
        let artifacts = mini_artifacts();
        assert_eq!(artifacts.lm.total, 100000.0);
    }

    #[test]
    fn ngram_invalid_final_alpha_retains_missing_header_error() {
        for earlier in ["", "# backoff=stupid:0.4\n", "# backoff=stupid:inf\n"] {
            let ngram = format!("# total_tokens=100\n{earlier}# backoff=stupid:invalid\n1\thello\t2.5\n");
            match Artifacts::from_strs("ni\t你\t1\n", &ngram, "hello\t1\n") {
                Ok(_) => panic!("accepted invalid final alpha"),
                Err(error) => assert_eq!(error, "ngram.tsv: missing '# backoff=stupid:<alpha>' header"),
            }
        }
    }

    #[test]
    fn ngram_nonfinite_counts_report_file_and_actual_line() {
        let mut failures = Vec::new();
        for (order, tokens) in [("1", "你"), ("2", "<s> 你"), ("3", "<s> <s> 你")] {
            for count in ["NaN", "inf", "-inf", "1e39"] {
                // Physical line numbers include comments, blanks, and valid data.
                let ngram = format!(
                    "# total_tokens=100\n# backoff=stupid:0.4\n\n1\thello\t2.5\n# fixture\n\n{order}\t{tokens}\t{count}\n"
                );
                match Artifacts::from_strs("ni\t你\t1\n", &ngram, "hello\t1\n") {
                    Ok(_) => failures.push(format!("accepted nonfinite count {count:?} for order {order}")),
                    Err(error) => {
                        if !error.contains("ngram.tsv line 7:") {
                            failures.push(format!("wrong location for order {order}, {count:?}: {error}"));
                        }
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn ngram_finite_float_counts_preserve_backoff_ratios() {
        let ngram = "# total_tokens=100\n# backoff=stupid:0.4\n\n\
                     1\t你\t2.5\n1\t<s>\t1\n\
                     2\t<s> 你\t1.5\n2\t<s> hello\t0.5\n\
                     3\t<s> <s> 你\t0.5\n3\t<s> <s> hello\t1.5\n";
        let artifacts = Artifacts::from_strs("ni\t你\t1\n", ngram, "hello\t1\n")
            .expect("finite fractional counts must load");
        let ni = artifacts.interner.get("你").unwrap();
        let unknown = crate::interner::UNK;
        assert_eq!(artifacts.stats.ngrams, (2, 2, 2));
        assert_eq!(artifacts.lm.total, 100.0);
        assert_eq!(artifacts.lm.alpha, 0.4);
        for (h2, h1, probability) in [
            (unknown, unknown, 0.4_f64.powi(2) * 2.5 / 100.0),
            (unknown, artifacts.bos, 0.4 * 1.5 / 2.0),
            (artifacts.bos, artifacts.bos, 0.5 / 2.0),
        ] {
            let score = artifacts.lm.logp(ni, h2, h1);
            assert!(score.is_finite());
            assert!((score - probability.log10()).abs() < 1e-12);
        }
    }

    #[test]
    fn ngram_header_blank_line_preserves_required_hyperparameters() {
        let ngram = "# total_tokens=100\n\n# backoff=stupid:0.4\n1\thello\t2.5\n";
        let result = Artifacts::from_strs("ni\t你\t1\n", ngram, "hello\t1\n");
        assert!(result.is_ok(), "blank header line must load: {:?}", result.as_ref().err());
        let artifacts = result
            .unwrap_or_else(|error| panic!("blank header line must load: {error}"));
        assert_eq!(artifacts.lm.total, 100.0);
        assert_eq!(artifacts.lm.alpha, 0.4);
    }

    #[test]
    fn ngram_header_layouts_preserve_hyperparameters_and_data() {
        for header in [
            "# total_tokens=100\n# backoff=stupid:0.4\n",
            "\n# total_tokens=100\n\n# backoff=stupid:0.4\n\n",
            "# total_tokens=100\r\n\r\n# backoff=stupid:0.4\r\n",
        ] {
            let ngram = format!("{header}\n1\thello\t2.5\n\n# total_tokens=999\n# backoff=stupid:0.9\n\n2\t<s> hello\t1.5\n");
            let artifacts = Artifacts::from_strs("ni\t你\t1\n", &ngram, "hello\t1\n")
                .unwrap_or_else(|error| panic!("header {header:?} must load: {error}"));
            assert_eq!(artifacts.lm.total, 100.0);
            assert_eq!(artifacts.lm.alpha, 0.4);
            assert_eq!(artifacts.stats.ngrams, (1, 1, 0));
        }
    }

    #[test]
    fn ngram_header_requires_both_hyperparameters_before_first_data() {
        for (header, late_header, missing) in [
            ("# total_tokens=100\n", "# backoff=stupid:0.4\n", "backoff=stupid:<alpha>"),
            ("# backoff=stupid:0.4\n", "# total_tokens=100\n", "total_tokens=<N>"),
        ] {
            for suffix in ["", late_header] {
                let ngram = format!("{header}\n1\thello\t2.5\n\n{suffix}");
                match Artifacts::from_strs("ni\t你\t1\n", &ngram, "hello\t1\n") {
                    Ok(_) => panic!("accepted missing header {missing}"),
                    Err(error) => assert!(error.contains(missing), "wrong error: {error}"),
                }
            }
        }
    }

    #[test]
    fn english_missing_rank_is_rejected() {
        assert!(Artifacts::from_strs(MINI_DICT, MINI_NGRAM, "test").is_err());
    }

    #[test]
    fn english_extra_column_is_rejected() {
        assert!(Artifacts::from_strs(MINI_DICT, MINI_NGRAM, "test\t1\textra").is_err());
    }

    #[test]
    fn english_empty_trailing_column_is_rejected() {
        assert!(Artifacts::from_strs(MINI_DICT, MINI_NGRAM, "test\t1\t").is_err());
    }

    #[test]
    fn english_bad_column_count_reports_actual_line() {
        for row in ["test", "test\t1\textra", "test\t1\t", "a", "123\t1\textra"] {
            let english = format!("# fixture\n\nValid\t1\n# comment\n\n{row}\n");
            match Artifacts::from_strs(MINI_DICT, MINI_NGRAM, &english) {
                Ok(_) => panic!("accepted malformed english.tsv line 6: {row:?}"),
                Err(error) => assert!(
                    error.contains("english.tsv line 6:"),
                    "wrong location for {row:?}: {error}"
                ),
            }
        }
    }

    #[test]
    fn english_two_columns_preserve_casing_filtering_and_unused_rank() {
        for rank in ["1", "invalid", "-2.5", ""] {
            let english = format!(
                "# word<TAB>rank\n\nTeSt\t{rank}\na\t1\nabc123\t2\nnon-ascii\t3\n\n# end\n"
            );
            let artifacts = Artifacts::from_strs(MINI_DICT, MINI_NGRAM, &english)
                .unwrap_or_else(|error| panic!("valid two columns with rank {rank:?}: {error}"));
            assert_eq!(artifacts.stats.english_words, 1);
            let entries = artifacts.builder.en_matcher.get("test").expect("lowercase key");
            assert_eq!(entries[0].text.as_ref(), "TeSt");
            for filtered in ["a", "abc123", "non-ascii"] {
                assert!(artifacts.builder.en_matcher.get(filtered).is_none());
            }
        }
    }

    fn assert_dict_bad_column_count(row: &str) {
        // Count comments, blank lines, and valid data in the physical line number.
        let dict = format!("# fixture\n\nni\t你\t3\n# another comment\n\n{row}\n");
        match Artifacts::from_strs(&dict, MINI_NGRAM, MINI_ENGLISH) {
            Ok(_) => panic!("accepted malformed dict.tsv line 6: {row:?}"),
            Err(error) => assert!(
                error.contains("dict.tsv line 6: bad column count"),
                "wrong error for {row:?}: {error}"
            ),
        }
    }

    #[test]
    fn dict_missing_columns_report_actual_line() {
        for row in ["ni", "ni\t你"] {
            assert_dict_bad_column_count(row);
        }
    }

    #[test]
    fn dict_extra_columns_report_actual_line() {
        for row in ["ni\t你\t3\textra", "ni\t你\t3\textra\tmore"] {
            assert_dict_bad_column_count(row);
        }
    }

    #[test]
    fn dict_empty_trailing_fourth_column_reports_actual_line() {
        assert_dict_bad_column_count("ni\t你\t3\t");
    }

    #[test]
    fn dict_three_columns_preserve_comments_blanks_and_weight_parsing() {
        for weight in ["3", "2.5", "-1.5", "NaN", "inf", "invalid", ""] {
            let dict = format!("# fixture\n\nni hao\t你好\t{weight}\n\n# end\n");
            let artifacts = Artifacts::from_strs(&dict, MINI_NGRAM, MINI_ENGLISH)
                .unwrap_or_else(|error| panic!("three columns with weight {weight:?}: {error}"));
            assert_eq!(artifacts.stats.dict_entries, 1);
            assert!(artifacts.syllables.segment("nihao").is_some());
        }
    }

    const MINI_DICT: &str = "\
# key<TAB>text<TAB>weight
ni hao\t你好\t5000
ni\t你\t3000
ni\t尼\t300
hao\t好\t2000
hao\t号\t100
wo\t我\t3000
yao\t要\t2000
yao\t药\t200
ce shi\t测试\t1500
ce\t测\t100
shi\t是\t2000
shi\t试\t300
yong gan\t勇敢\t800
yong\t用\t900
gan\t敢\t300
gan\t干\t500
te\t特\t400
zhong wen\t中文\t1000
zhong\t中\t1500
wen\t文\t500
shu ru\t输入\t900
shu\t书\t400
ru\t入\t200
sui ji\t随即\t900
sui ji\t随机\t800
ti du\t梯度\t200
sui\t随\t500
ji\t机\t400
ji\t即\t350
ti\t梯\t100
du\t度\t300
";

    const MINI_NGRAM: &str = "\
# order<TAB>token1[ token2[ token3]]<TAB>count
# total_tokens=100000
# backoff=stupid:0.4
1\t你好\t500
1\t你\t800
1\t好\t600
1\t我\t900
1\t要\t500
1\t测试\t300
1\t勇敢\t200
1\t用\t300
1\t敢\t50
1\t干\t100
1\t中文\t250
1\t输入\t240
1\t是\t700
1\t试\t60
1\t测\t40
1\t尼\t10
1\t号\t20
1\t药\t30
1\t特\t80
1\t书\t60
1\t入\t40
1\t文\t90
1\t中\t400
1\t随即\t200
1\t随机\t50
1\t梯度\t30
1\t随\t20
1\t机\t30
1\t即\t25
1\t梯\t5
1\t度\t40
2\t<s> 你好\t200
2\t<s> 我\t300
2\t我 要\t200
2\t要 测试\t80
2\t要 勇敢\t50
3\t<s> <s> 你好\t150
3\t<s> 我 要\t120
3\t我 要 测试\t60
3\t我 要 勇敢\t30
";

    const MINI_ENGLISH: &str = "\
# word<TAB>rank
test\t1
gan\t2
the\t3
a\t4
";

    /// Small self-contained artifacts for unit tests (no full-size files).
    pub(crate) fn mini_artifacts() -> Artifacts {
        Artifacts::from_strs(MINI_DICT, MINI_NGRAM, MINI_ENGLISH).expect("mini artifacts parse")
    }

    #[test]
    fn parses_mini_artifacts_counts_and_headers() {
        let a = mini_artifacts();
        assert_eq!(a.stats.dict_entries, 31);
        assert_eq!(a.stats.ngrams, (31, 5, 4));
        // "a" is filtered (len < 2), so 3 English entries
        assert_eq!(a.stats.english_words, 3);
        assert!((a.lm.alpha - 0.4).abs() < 1e-12);
        assert!((a.lm.total - 100000.0).abs() < 1e-9);
        // syllable inventory derived from dict keys
        assert!(a.syllables.segment("nihao").is_some());
        assert!(a.syllables.segment("zhongwenshuru").is_some());
    }

    #[test]
    fn missing_headers_are_rejected() {
        let bad = "# no headers here\n1\tx\t1\n";
        assert!(Artifacts::from_strs(MINI_DICT, bad, MINI_ENGLISH).is_err());
    }

    #[test]
    fn malformed_ngram_rows_report_file_and_actual_line() {
        let mut failures = Vec::new();
        for (order, valid) in [("1", "hello"), ("2", "<s> hello"), ("3", "<s> hello world")] {
            let mut rows = vec![
                format!("{order}\t\t1"),
                format!("{order}\t{valid} extra\t1"),
                format!("{order}\t {valid}\t1"),
                format!("{order}\t{valid} \t1"),
                format!("{order}\t{valid}\t1\textra"),
                format!("{order}\t{valid}\t1\t"),
                format!("{order}\t{valid}"),
            ];
            if order != "1" {
                rows.push(format!("{order}\thello\t1"));
                rows.push(format!("{order}\t{}\t1", valid.replace(' ', "  ")));
            }
            if order == "3" {
                rows.push("3\t<s> hello\t1".into());
                rows.push("3\t<s>  world\t1".into());
            }
            for row in rows {
                // Comments, a blank line, and a valid row precede the bad row.
                let text = format!(
                    "# total_tokens=100\n# backoff=stupid:0.4\n\n# fixture\n1\thello\t2\n{row}\n"
                );
                match Artifacts::from_strs(MINI_DICT, &text, MINI_ENGLISH) {
                    Ok(_) => failures.push(format!("accepted malformed row: {row:?}")),
                    Err(error) => {
                        if !error.contains("ngram.tsv line 6:") {
                            failures.push(format!("wrong location for {row:?}: {error}"));
                        }
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn valid_ngram_orders_and_bos_continue_loading() {
        let text = "# total_tokens=100\n# backoff=stupid:0.4\n\n\
                    1\thello\t2.5\n1\t<s>\t1\n\
                    2\t<s> hello\t1.5\n3\t<s> <s> hello\t0.5\n";
        let artifacts = Artifacts::from_strs(MINI_DICT, text, MINI_ENGLISH)
            .expect("valid orders, fractional counts, and <s> must load");
        assert_eq!(artifacts.stats.ngrams, (2, 1, 1));
    }
}
