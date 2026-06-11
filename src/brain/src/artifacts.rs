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
    for line in text.lines().take_while(|l| l.starts_with('#')) {
        if let Some(v) = line.split("total_tokens=").nth(1) {
            total = v.trim().parse().ok();
        }
        if let Some(v) = line.split("backoff=stupid:").nth(1) {
            alpha = v.trim().parse().ok();
        }
    }
    let alpha = alpha.ok_or("ngram.tsv: missing '# backoff=stupid:<alpha>' header")?;
    let total = total.ok_or("ngram.tsv: missing '# total_tokens=<N>' header")?;
    let mut lm = BackoffTrigramLm::new(alpha, total);
    for (no, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let (order, tokens, count) = match (cols.next(), cols.next(), cols.next()) {
            (Some(o), Some(t), Some(c)) => (o, t, c),
            _ => return Err(format!("ngram.tsv line {}: bad column count", no + 1)),
        };
        let count: f32 = count
            .parse()
            .map_err(|_| format!("ngram.tsv line {}: bad count '{}'", no + 1, count))?;
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
        let (key, word, weight) = match (cols.next(), cols.next(), cols.next()) {
            (Some(k), Some(w), Some(c)) => (k, w, c),
            _ => return Err(format!("dict.tsv line {}: bad column count", no + 1)),
        };
        let weight: i64 = weight.parse::<f64>().map(|f| f as i64).unwrap_or(0);
        for syl in key.split(' ') {
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
                matcher.add(
                    &key,
                    Entry {
                        text,
                        pinyin: spaced,
                        lm_token,
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
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let word = line.split('\t').next().unwrap_or("");
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
            },
        );
        n += 1;
    }
    Ok((m, n))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

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
}
