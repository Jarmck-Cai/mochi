//! Personal memory layer (λ₂ of DESIGN.md §3, M2-3).
//!
//! What it learns from each committed text (ipc-v0 `commit`):
//!   - a personal word-trigram LM with time-decayed counts, kept twice:
//!     globally and per scene bucket (ADR-004: 微信里的你和写论文的你是两套
//!     习惯), queried with hierarchical backoff scene -> global;
//!   - a personal lexicon (word -> decayed count) feeding the λ_lex bonus;
//!   - personal lattice arcs: the user's English terms with their preferred
//!     casing, and dict-OOV Chinese words whose pinyin is recovered by
//!     aligning the committed text against the raw key stream;
//!   - the user's casing habits ("resnet" -> "ResNet").
//!
//! Persistence is an append-only `commits.jsonl` (one JSON event per line)
//! replayed at startup with exponential day-level decay, plus an optional
//! `personal.tsv` cold-start file exported from a document corpus
//! (experiments/001 export_personal.py). A plain text log instead of an
//! embedded DB keeps the memory user-inspectable and user-editable — delete
//! a line and the brain forgets it on next start.

use std::io::Write;
use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use crate::artifacts::Artifacts;
use crate::interner::UNK;
use crate::lattice::{Entry, Matcher, PersonalMatchers};
use crate::lm::PersonalLayer;

/// Day-level exponential decay of event weights (half-life ~34 days).
/// The Python experiment used 0.95 per *document*; per day is the online
/// equivalent. Counts are fixed at load time relative to "today"; same-day
/// commits weigh 1.0 (multi-day uptime re-normalizes on next start).
pub const DECAY_PER_DAY: f64 = 0.98;

/// Personal-LM stupid backoff matches pipeline/lm.py exactly.
const ALPHA: f64 = 0.4;
const OOV_LOGP: f64 = -7.5;

/// Lexicon thresholds (personal.py: min_en_count / min_zh_count).
const MIN_EN_COUNT: f64 = 2.0;
const MIN_ZH_COUNT: f64 = 3.0;
/// Decayed occurrences before a whole committed phrase (dict-OOV, 2-6 hanzi)
/// becomes a personal word with its own lattice arc.
const MIN_PHRASE_COUNT: f64 = 2.0;
/// Re-selecting a different word for the SAME raw input is a correction —
/// the strongest signal the memory gets. It learns at this multiple, and
/// the contradicted earlier commit is retracted (negative learning).
/// Real case: quoting a wrong word (桔子) repeatedly flipped the ranking;
/// with corrections weighted, the last deliberate choice wins.
const CORRECTION_BOOST: f64 = 3.0;
/// How many recent commits are scanned for a same-input contradiction.
const RECENT_WINDOW: usize = 16;

#[inline]
fn bi_key(h1: u32, w: u32) -> u64 {
    ((h1 as u64) << 32) | w as u64
}

#[inline]
fn tri_key(h2: u32, h1: u32, w: u32) -> u128 {
    ((h2 as u128) << 64) | ((h1 as u128) << 32) | w as u128
}

fn is_hanzi(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

/// Hashmap-backed weighted trigram LM with stupid backoff. Unlike the
/// general `BackoffTrigramLm` (Vec-indexed, immutable after finalize) this
/// one mutates per commit and stays sparse, so denominator tables are
/// maintained incrementally: every trigram add feeds `bi_hist`, every
/// bigram add feeds `uni_hist` — same totals lm.py derives in finalize().
#[derive(Default)]
pub struct PersonalLm {
    uni: FxHashMap<u32, f64>,
    bi: FxHashMap<u64, f64>,
    tri: FxHashMap<u128, f64>,
    bi_hist: FxHashMap<u64, f64>,
    uni_hist: FxHashMap<u32, f64>,
    total: f64,
}

impl PersonalLm {
    pub fn add_unigram(&mut self, w: u32, weight: f64) {
        *self.uni.entry(w).or_insert(0.0) += weight;
        self.total += weight;
    }

    pub fn add_bigram(&mut self, h1: u32, w: u32, weight: f64) {
        *self.bi.entry(bi_key(h1, w)).or_insert(0.0) += weight;
        *self.uni_hist.entry(h1).or_insert(0.0) += weight;
    }

    pub fn add_trigram(&mut self, h2: u32, h1: u32, w: u32, weight: f64) {
        *self.tri.entry(tri_key(h2, h1, w)).or_insert(0.0) += weight;
        *self.bi_hist.entry(bi_key(h2, h1)).or_insert(0.0) += weight;
    }

    /// lm.py `add_sentence`: BOS BOS padding, every token counted once.
    pub fn add_sentence(&mut self, tokens: &[u32], bos: u32, weight: f64) {
        if tokens.is_empty() {
            return;
        }
        for &w in tokens {
            self.add_unigram(w, weight);
        }
        let padded: Vec<u32> = [bos, bos].iter().chain(tokens.iter()).copied().collect();
        for i in 2..padded.len() {
            self.add_bigram(padded[i - 1], padded[i], weight);
            self.add_trigram(padded[i - 2], padded[i - 1], padded[i], weight);
        }
    }

    fn knows(&self, w: u32) -> bool {
        self.uni.contains_key(&w)
    }

    /// log10 P(w | h2 h1), identical ladder to lm.py.
    pub fn logp(&self, w: u32, h2: u32, h1: u32) -> f64 {
        let log_alpha = ALPHA.log10();
        if w != UNK {
            if let Some(&c3) = self.tri.get(&tri_key(h2, h1, w)) {
                if c3 > 0.0 {
                    if let Some(&denom) = self.bi_hist.get(&bi_key(h2, h1)) {
                        if denom > 0.0 {
                            return (c3 / denom).log10();
                        }
                    }
                }
            }
            if let Some(&c2) = self.bi.get(&bi_key(h1, w)) {
                if c2 > 0.0 {
                    if let Some(&denom) = self.uni_hist.get(&h1) {
                        if denom > 0.0 {
                            return log_alpha + (c2 / denom).log10();
                        }
                    }
                }
            }
            if let Some(&c1) = self.uni.get(&w) {
                if c1 > 0.0 && self.total > 0.0 {
                    return 2.0 * log_alpha + (c1 / self.total).log10();
                }
            }
        }
        2.0 * log_alpha + OOV_LOGP
    }

    #[inline]
    pub fn p(&self, w: u32, h2: u32, h1: u32) -> f64 {
        10f64.powf(self.logp(w, h2, h1))
    }
}

/// One learning event as persisted in commits.jsonl.
#[derive(Debug, Serialize, Deserialize)]
struct CommitEvent {
    day: u32,
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    app: Option<String>,
}

fn today() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as u32)
        .unwrap_or(0)
}

pub struct PersonalStore {
    bos: u32,
    /// Token ids above the load-time vocab live here (commit-time words).
    ext_map: FxHashMap<Box<str>, u32>,
    base_vocab: u32,
    global: PersonalLm,
    scenes: FxHashMap<Box<str>, PersonalLm>,
    /// word token -> decayed count, split by script so thresholds differ.
    lex_en: FxHashMap<u32, f64>,
    lex_zh: FxHashMap<u32, f64>,
    /// English surface-form votes: token -> (surface -> decayed count).
    case_counts: FxHashMap<u32, FxHashMap<Box<str>, f64>>,
    /// Whole-commit phrase votes for dict-OOV multi-char words.
    phrase_counts: FxHashMap<u32, f64>,
    zh_matcher: Matcher,
    en_matcher: Matcher,
    zh_added: FxHashSet<u32>,
    en_added: FxHashSet<u32>,
    /// Current arc surface per English token (to detect casing flips).
    en_surface: FxHashMap<u32, Box<str>>,
    today: u32,
    log_path: Option<PathBuf>,
    /// Recent commits, scanned for same-input corrections (newest last).
    recent: std::collections::VecDeque<RecentCommit>,
    pub n_events: u64,
}

struct RecentCommit {
    input: Box<str>,
    text: Box<str>,
    /// What the event actually added (taken on retraction). Storing the
    /// applied deltas — not the text — makes retraction exact even when
    /// tokenization has drifted since (e.g. a phrase got promoted).
    record: Option<LearnRecord>,
}

/// The ranking-relevant mass one learn() call added: LM sentences (token
/// ids as counted), lexicon increments, the phrase vote. Structural
/// side effects (arc promotion, casing votes, back-fill) are one-time and
/// stay — only the mutable mass is retractable.
struct LearnRecord {
    weight: f64,
    scene: Option<String>,
    sentences: Vec<Vec<u32>>,
    lex_zh: Vec<(u32, f64)>,
    lex_en: Vec<(u32, f64)>,
    phrase: Option<(u32, f64)>,
}

/// Scene-resolved read view; what the decoder scores against.
pub struct PersonalView<'a> {
    global: &'a PersonalLm,
    scene: Option<&'a PersonalLm>,
    store: &'a PersonalStore,
}

impl PersonalLayer for PersonalView<'_> {
    /// ADR-004 ladder: P(w|ctx,scene) when the scene bucket knows the word,
    /// else P(w|ctx) global. (The general-LM rung is the μ_g term.)
    fn p(&self, w: u32, h2: u32, h1: u32) -> f64 {
        if let Some(scene) = self.scene {
            if scene.knows(w) {
                return scene.p(w, h2, h1);
            }
        }
        self.global.p(w, h2, h1)
    }

    /// decoder.py: min(3.0, log10(1+c)+0.5) for words past the lexicon
    /// thresholds; 0.0 otherwise.
    fn lexicon_bonus(&self, w: u32) -> f64 {
        let c = match self.store.lex_en.get(&w) {
            Some(&c) if c >= MIN_EN_COUNT => c,
            _ => match self.store.lex_zh.get(&w) {
                Some(&c) if c >= MIN_ZH_COUNT => c,
                _ => return 0.0,
            },
        };
        ((1.0 + c).log10() + 0.5).min(3.0)
    }
}

impl<'a> PersonalView<'a> {
    pub fn matchers(&self) -> PersonalMatchers<'a> {
        PersonalMatchers {
            zh: &self.store.zh_matcher,
            en: &self.store.en_matcher,
        }
    }
}

/// Text units for alignment/tokenization.
enum Unit {
    Hanzi(char),
    Ascii(String), // original casing kept for the case map
    Sep,           // punctuation etc: sentence boundary, consumes no keys
}

impl PersonalStore {
    pub fn new(arts: &Artifacts) -> Self {
        PersonalStore {
            bos: arts.bos,
            ext_map: FxHashMap::default(),
            base_vocab: arts.interner.len() as u32,
            global: PersonalLm::default(),
            scenes: FxHashMap::default(),
            lex_en: FxHashMap::default(),
            lex_zh: FxHashMap::default(),
            case_counts: FxHashMap::default(),
            phrase_counts: FxHashMap::default(),
            zh_matcher: Matcher::default(),
            en_matcher: Matcher::default(),
            zh_added: FxHashSet::default(),
            en_added: FxHashSet::default(),
            en_surface: FxHashMap::default(),
            today: today(),
            log_path: None,
            recent: std::collections::VecDeque::new(),
            n_events: 0,
        }
    }

    /// Load the durable memory: optional personal.tsv (cold start), then
    /// replay commits.jsonl with day-level decay. Subsequent commits append.
    pub fn open(arts: &Artifacts, dir: &Path) -> Self {
        let mut store = Self::new(arts);
        let _ = std::fs::create_dir_all(dir);
        let tsv_path = dir.join("personal.tsv");
        if let Ok(tsv) = std::fs::read_to_string(&tsv_path) {
            match store.import_tsv(arts, &tsv) {
                Ok(n) => eprintln!("[brain] personal.tsv: {} entries", n),
                Err(e) => eprintln!("[brain] personal.tsv ignored: {}", e),
            }
        }
        let log_path = dir.join("commits.jsonl");
        if let Ok(log) = std::fs::read_to_string(&log_path) {
            let mut n = 0u64;
            for line in log.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<CommitEvent>(line) {
                    Ok(ev) => {
                        let age = store.today.saturating_sub(ev.day);
                        let weight = DECAY_PER_DAY.powi(age as i32);
                        // same path as live commits: the journal stores raw
                        // facts, correction detection re-runs on replay
                        store.ingest(arts, &ev.text, ev.input.as_deref(), ev.app.as_deref(), weight);
                        n += 1;
                    }
                    Err(e) => eprintln!("[brain] commits.jsonl: skipping bad line: {}", e),
                }
            }
            eprintln!("[brain] commits.jsonl: {} events replayed", n);
        }
        store.log_path = Some(log_path);
        store
    }

    /// Live commit: learn (with correction detection) and journal the raw
    /// event — interpretation happens at learn time, the log stays facts.
    pub fn commit(&mut self, arts: &Artifacts, text: &str, input: Option<&str>, app: Option<&str>) {
        self.ingest(arts, text, input, app, 1.0);
        if let Some(path) = self.log_path.clone() {
            let ev = CommitEvent {
                day: self.today,
                text: text.to_string(),
                input: input.map(str::to_string),
                app: app.map(str::to_string),
            };
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                let _ = serde_json::to_writer(&mut f, &ev);
                let _ = f.write_all(b"\n");
            }
        }
    }

    pub fn view(&self, app: Option<&str>) -> PersonalView<'_> {
        let scene = app
            .map(|a| a.to_ascii_lowercase())
            .and_then(|a| self.scenes.get(a.as_str()));
        PersonalView {
            global: &self.global,
            scene,
            store: self,
        }
    }

    /// Resolve a word to its token id, extending past the general vocab.
    fn token(&mut self, arts: &Artifacts, s: &str) -> u32 {
        if let Some(id) = arts.interner.get(s) {
            return id;
        }
        if let Some(&id) = self.ext_map.get(s) {
            return id;
        }
        let id = self.base_vocab + self.ext_map.len() as u32;
        self.ext_map.insert(s.into(), id);
        id
    }

    fn known_word(&self, arts: &Artifacts, s: &str) -> bool {
        arts.interner.get(s).is_some() || self.ext_map.contains_key(s)
    }

    // ------------------------------------------------------------- learning

    /// One commit event: correction detection, then learning. A recent
    /// commit with the SAME raw input but a different text means the user
    /// re-chose — the new pick learns boosted, the contradicted event is
    /// retracted at exactly the weight it was learned with.
    fn ingest(&mut self, arts: &Artifacts, text: &str, input: Option<&str>, app: Option<&str>, base_weight: f64) {
        let mut weight = base_weight;
        if let Some(keys) = input {
            let contradicted = self
                .recent
                .iter()
                .rposition(|r| r.record.is_some() && &*r.input == keys && &*r.text != text);
            if let Some(idx) = contradicted {
                if let Some(record) = self.recent[idx].record.take() {
                    self.retract(&record);
                }
                weight = base_weight * CORRECTION_BOOST;
            }
        }
        self.n_events += 1;
        let record = self.learn(arts, text, input, app, weight);
        if let Some(keys) = input {
            self.recent.push_back(RecentCommit {
                input: keys.into(),
                text: text.into(),
                record,
            });
            if self.recent.len() > RECENT_WINDOW {
                self.recent.pop_front();
            }
        }
    }

    /// Inverse of one learn() call, applied to the exact deltas it recorded.
    fn retract(&mut self, rec: &LearnRecord) {
        for sent in &rec.sentences {
            self.global.add_sentence(sent, self.bos, -rec.weight);
            if let Some(scene) = &rec.scene {
                if let Some(lm) = self.scenes.get_mut(scene.as_str()) {
                    lm.add_sentence(sent, self.bos, -rec.weight);
                }
            }
        }
        for &(tok, d) in &rec.lex_zh {
            *self.lex_zh.entry(tok).or_insert(0.0) -= d;
        }
        for &(tok, d) in &rec.lex_en {
            *self.lex_en.entry(tok).or_insert(0.0) -= d;
        }
        if let Some((tok, d)) = rec.phrase {
            *self.phrase_counts.entry(tok).or_insert(0.0) -= d;
        }
    }

    /// One learning pass over a committed text; returns the retractable
    /// deltas. The heart of "学习必须即时生效": everything updated here is
    /// visible to the very next query.
    fn learn(
        &mut self,
        arts: &Artifacts,
        text: &str,
        input: Option<&str>,
        app: Option<&str>,
        weight: f64,
    ) -> Option<LearnRecord> {
        let units = parse_units(text);
        if units.is_empty() {
            return None;
        }
        let mut rec = LearnRecord {
            weight,
            scene: None,
            sentences: Vec::new(),
            lex_zh: Vec::new(),
            lex_en: Vec::new(),
            phrase: None,
        };
        // Pinyin per hanzi unit, recovered from the raw keys when possible.
        let syllables = input.and_then(|keys| align(&units, keys, arts));

        // --- tokenize into sentences (greedy longest match over known words)
        let mut sentences: Vec<Vec<u32>> = Vec::new();
        let mut current: Vec<u32> = Vec::new();
        let mut zh_spans: Vec<(u32, String, Option<String>)> = Vec::new(); // token, text, pinyin key
        let mut i = 0;
        while i < units.len() {
            match &units[i] {
                Unit::Sep => {
                    if !current.is_empty() {
                        sentences.push(std::mem::take(&mut current));
                    }
                    i += 1;
                }
                Unit::Ascii(surface) => {
                    let lower = surface.to_ascii_lowercase();
                    if (2..=20).contains(&lower.len()) {
                        let tok = self.token(arts, &lower);
                        current.push(tok);
                        *self.lex_en.entry(tok).or_insert(0.0) += weight;
                        rec.lex_en.push((tok, weight));
                        *self
                            .case_counts
                            .entry(tok)
                            .or_default()
                            .entry(surface.as_str().into())
                            .or_insert(0.0) += weight;
                        self.refresh_en_arc(tok, &lower);
                    }
                    i += 1;
                }
                Unit::Hanzi(_) => {
                    // collect the full hanzi run, then greedy-match words
                    let start = i;
                    while i < units.len() && matches!(units[i], Unit::Hanzi(_)) {
                        i += 1;
                    }
                    let run: Vec<char> = units[start..i]
                        .iter()
                        .map(|u| match u {
                            Unit::Hanzi(c) => *c,
                            _ => unreachable!(),
                        })
                        .collect();
                    let mut j = 0;
                    while j < run.len() {
                        let max_l = (run.len() - j).min(6);
                        let mut taken = 1;
                        let mut word: String = run[j].to_string();
                        for l in (2..=max_l).rev() {
                            let cand: String = run[j..j + l].iter().collect();
                            if self.known_word(arts, &cand) {
                                taken = l;
                                word = cand;
                                break;
                            }
                        }
                        let tok = self.token(arts, &word);
                        current.push(tok);
                        if taken >= 2 {
                            *self.lex_zh.entry(tok).or_insert(0.0) += weight;
                            rec.lex_zh.push((tok, weight));
                            let key = syllables.as_ref().map(|syls| {
                                syls[start + j..start + j + taken].join(" ")
                            });
                            zh_spans.push((tok, word, key));
                        }
                        j += taken;
                    }
                }
            }
        }
        if !current.is_empty() {
            sentences.push(current);
        }

        // --- n-gram counts: global + scene bucket (ADR-004 场景分桶)
        let scene_key = app.map(|a| a.to_ascii_lowercase());
        for sent in &sentences {
            self.global.add_sentence(sent, self.bos, weight);
            if let Some(key) = &scene_key {
                self.scenes
                    .entry(key.as_str().into())
                    .or_default()
                    .add_sentence(sent, self.bos, weight);
            }
        }
        rec.scene = scene_key.clone();

        // --- personal zh word arcs for dict-OOV words past the threshold
        for (tok, word, key) in zh_spans {
            self.maybe_add_zh_arc(arts, tok, &word, key.as_deref(), MIN_ZH_COUNT, false);
        }

        // --- whole-phrase learning: an all-hanzi commit of 2-6 chars that
        // the dict cannot propose as one word is a strong "new word" signal
        // (greedy matching can never discover it on its own, unlike jieba).
        let all_hanzi: Vec<char> = text.chars().collect();
        if (2..=6).contains(&all_hanzi.len()) && all_hanzi.iter().all(|&c| is_hanzi(c)) {
            let tok = self.token(arts, text);
            if !arts.dict_tokens.contains(&tok) && !self.zh_added.contains(&tok) {
                let count = {
                    let e = self.phrase_counts.entry(tok).or_insert(0.0);
                    *e += weight;
                    *e
                };
                rec.phrase = Some((tok, weight));
                if count >= MIN_PHRASE_COUNT {
                    let key = syllables.as_ref().map(|syls| syls.join(" "));
                    let promoted = self.maybe_add_zh_arc(
                        arts,
                        tok,
                        text,
                        key.as_deref(),
                        0.0, // phrase threshold already passed
                        true,
                    );
                    if promoted {
                        // back-fill the LM/lexicon: earlier events counted
                        // this phrase as its pieces, the merged token starts
                        // from the accumulated phrase count
                        self.global.add_unigram(tok, count);
                        *self.lex_zh.entry(tok).or_insert(0.0) += count;
                    }
                }
            }
        }

        rec.sentences = sentences;
        Some(rec)
    }

    /// Add a personal lattice arc for a Chinese word if it is dict-OOV, past
    /// `min_count` in the lexicon, and its pinyin key is known. Returns
    /// whether the arc was (newly) added.
    fn maybe_add_zh_arc(
        &mut self,
        arts: &Artifacts,
        tok: u32,
        word: &str,
        key: Option<&str>,
        min_count: f64,
        skip_lexicon_check: bool,
    ) -> bool {
        if self.zh_added.contains(&tok) || arts.dict_tokens.contains(&tok) {
            return false;
        }
        if !skip_lexicon_check {
            match self.lex_zh.get(&tok) {
                Some(&c) if c >= min_count => {}
                _ => return false,
            }
        }
        let spaced = match key {
            Some(k) if !k.is_empty() => k.to_string(),
            // no alignment this time: fall back to an arbitrary known reading
            // per char; ambiguous chars may err until a typed commit fixes it
            _ => {
                let mut parts: Vec<&str> = Vec::new();
                for c in word.chars() {
                    match arts.char_pinyin.get(&c).and_then(|v| v.first()) {
                        Some(s) => parts.push(s),
                        None => return false, // unreadable char: no arc
                    }
                }
                parts.join(" ")
            }
        };
        let concat: String = spaced.split(' ').collect();
        self.zh_matcher.add(
            &concat,
            Entry {
                text: word.into(),
                pinyin: spaced.into(),
                lm_token: tok,
            },
        );
        self.zh_added.insert(tok);
        true
    }

    /// Create/refresh the personal English arc once past the threshold,
    /// keeping the surface at the user's currently preferred casing.
    fn refresh_en_arc(&mut self, tok: u32, lower: &str) {
        match self.lex_en.get(&tok) {
            Some(&c) if c >= MIN_EN_COUNT => {}
            _ => return,
        }
        let best = self
            .case_counts
            .get(&tok)
            .and_then(|m| {
                m.iter()
                    .max_by(|a, b| a.1.total_cmp(b.1).then_with(|| a.0.cmp(b.0)))
            })
            .map(|(s, _)| s.clone());
        let best = match best {
            Some(s) => s,
            None => return,
        };
        if self.en_added.contains(&tok) {
            if self.en_surface.get(&tok) != Some(&best) {
                self.en_matcher.set_text(lower, tok, &best);
                self.en_surface.insert(tok, best);
            }
            return;
        }
        self.en_matcher.add(
            lower,
            Entry {
                text: best.clone(),
                pinyin: lower.into(),
                lm_token: tok,
            },
        );
        self.en_added.insert(tok);
        self.en_surface.insert(tok, best);
    }

    // ------------------------------------------------------------ cold start

    /// personal-artifacts-v0 (docs/specs/lm-artifacts-v0.md): lex / case /
    /// zhword / 1|2|3 n-gram lines. Counts are used as-is (the exporter
    /// already applied document decay). Returns the number of data lines.
    pub fn import_tsv(&mut self, arts: &Artifacts, content: &str) -> Result<usize, String> {
        let mut n = 0usize;
        let mut en_words: Vec<(u32, String)> = Vec::new();
        for (no, line) in content.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cols = line.split('\t');
            let (kind, a, b) = match (cols.next(), cols.next(), cols.next()) {
                (Some(k), Some(a), Some(b)) => (k, a, b),
                _ => return Err(format!("personal.tsv line {}: bad column count", no + 1)),
            };
            let badnum = |v: &str| format!("personal.tsv line {}: bad count '{}'", no + 1, v);
            match kind {
                "lex" => {
                    let count: f64 = b.parse().map_err(|_| badnum(b))?;
                    let tok = self.token(arts, a);
                    if a.is_ascii() {
                        *self.lex_en.entry(tok).or_insert(0.0) += count;
                        en_words.push((tok, a.to_string()));
                    } else {
                        *self.lex_zh.entry(tok).or_insert(0.0) += count;
                    }
                }
                "case" => {
                    let tok = self.token(arts, a);
                    *self
                        .case_counts
                        .entry(tok)
                        .or_default()
                        .entry(b.into())
                        .or_insert(0.0) += 1.0;
                }
                "zhword" => {
                    let tok = self.token(arts, a);
                    let word = a.to_string();
                    self.maybe_add_zh_arc(arts, tok, &word, Some(b), 0.0, true);
                }
                "1" | "2" | "3" => {
                    let count: f64 = b.parse().map_err(|_| badnum(b))?;
                    let mut toks = a.split(' ').map(|s| self.token(arts, s));
                    match kind {
                        "1" => {
                            let w = toks.next().ok_or_else(|| badnum(a))?;
                            self.global.add_unigram(w, count);
                        }
                        "2" => {
                            let (h1, w) = (toks.next(), toks.next());
                            let (h1, w) = h1.zip(w).ok_or_else(|| badnum(a))?;
                            self.global.add_bigram(h1, w, count);
                        }
                        _ => {
                            let (h2, h1, w) = (toks.next(), toks.next(), toks.next());
                            let ((h2, h1), w) = h2.zip(h1).zip(w).ok_or_else(|| badnum(a))?;
                            self.global.add_trigram(h2, h1, w, count);
                        }
                    }
                }
                other => {
                    return Err(format!("personal.tsv line {}: unknown kind '{}'", no + 1, other))
                }
            }
            n += 1;
        }
        for (tok, lower) in en_words {
            self.refresh_en_arc(tok, &lower);
        }
        Ok(n)
    }

    pub fn stats(&self) -> String {
        format!(
            "events={} lex_en={} lex_zh={} zh_arcs={} en_arcs={} scenes={} total={:.1}",
            self.n_events,
            self.lex_en.len(),
            self.lex_zh.len(),
            self.zh_added.len(),
            self.en_added.len(),
            self.scenes.len(),
            self.global.total,
        )
    }
}

/// Split a commit text into alignment/tokenization units.
fn parse_units(text: &str) -> Vec<Unit> {
    let mut units = Vec::new();
    let mut ascii_run = String::new();
    for c in text.chars() {
        if c.is_ascii_alphabetic() {
            ascii_run.push(c);
            continue;
        }
        if !ascii_run.is_empty() {
            units.push(Unit::Ascii(std::mem::take(&mut ascii_run)));
        }
        if is_hanzi(c) {
            units.push(Unit::Hanzi(c));
        } else {
            units.push(Unit::Sep);
        }
    }
    if !ascii_run.is_empty() {
        units.push(Unit::Ascii(ascii_run));
    }
    units
}

/// Align text units against the raw key stream: each hanzi consumes one of
/// its known syllables, each ASCII run consumes itself (lowercase), Seps
/// consume nothing. Returns one syllable per unit (empty for non-hanzi) only
/// if the whole stream is consumed exactly — partial/edited commits yield
/// None and simply skip pinyin-dependent learning.
fn align(units: &[Unit], keys: &str, arts: &Artifacts) -> Option<Vec<String>> {
    let keys = keys.to_ascii_lowercase();
    fn go(
        units: &[Unit],
        ui: usize,
        keys: &str,
        pos: usize,
        arts: &Artifacts,
        out: &mut Vec<String>,
    ) -> bool {
        if ui == units.len() {
            return pos == keys.len();
        }
        match &units[ui] {
            Unit::Sep => {
                out.push(String::new());
                if go(units, ui + 1, keys, pos, arts, out) {
                    return true;
                }
                out.pop();
                false
            }
            Unit::Ascii(s) => {
                let lower = s.to_ascii_lowercase();
                if keys[pos..].starts_with(&lower) {
                    out.push(String::new());
                    if go(units, ui + 1, keys, pos + lower.len(), arts, out) {
                        return true;
                    }
                    out.pop();
                }
                false
            }
            Unit::Hanzi(c) => {
                if let Some(readings) = arts.char_pinyin.get(c) {
                    for syl in readings {
                        if keys[pos..].starts_with(syl.as_ref()) {
                            out.push(syl.to_string());
                            if go(units, ui + 1, keys, pos + syl.len(), arts, out) {
                                return true;
                            }
                            out.pop();
                        }
                    }
                }
                false
            }
        }
    }
    let mut out = Vec::with_capacity(units.len());
    go(units, 0, &keys, 0, arts, &mut out).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::tests::mini_artifacts;

    #[test]
    fn personal_lm_matches_python_backoff_ladder() {
        // mirror lm.rs mini test semantics with weighted counts
        let (a, b, x, d) = (0u32, 1u32, 2u32, 3u32);
        let mut lm = PersonalLm::default();
        lm.add_unigram(a, 100.0);
        lm.add_unigram(b, 50.0);
        lm.add_unigram(d, 850.0); // total = 1000
        lm.add_bigram(a, b, 20.0);
        lm.add_trigram(x, a, b, 8.0);
        // trigram: P = 8/8 = 1.0
        assert!((lm.logp(b, x, a) - 0.0).abs() < 1e-9);
        // bigram: alpha * 20/20
        let expect = 0.4f64.log10();
        assert!((lm.logp(b, b, a) - expect).abs() < 1e-9);
        // unigram: alpha^2 * 100/1000
        let expect = 2.0 * 0.4f64.log10() + (0.1f64).log10();
        assert!((lm.logp(a, x, x) - expect).abs() < 1e-9);
        // oov floor
        let expect = 2.0 * 0.4f64.log10() + OOV_LOGP;
        assert!((lm.logp(99, x, x) - expect).abs() < 1e-9);
    }

    #[test]
    fn add_sentence_weights_and_pads_like_lm_py() {
        let mut lm = PersonalLm::default();
        let bos = 0u32;
        lm.add_sentence(&[1, 2], bos, 0.5);
        assert!((lm.total - 1.0).abs() < 1e-9); // 2 tokens * 0.5
        // P(1 | <s> <s>) = tri(<s>,<s>,1)/bi_hist(<s>,<s>) = 0.5/0.5 = 1.0
        assert!((lm.logp(1, bos, bos) - 0.0).abs() < 1e-9);
        // P(2 | <s> 1) = 0.5/0.5 = 1.0
        assert!((lm.logp(2, bos, 1) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn commit_learns_words_scene_buckets_and_casing() {
        let arts = mini_artifacts();
        let mut store = PersonalStore::new(&arts);
        store.commit(&arts, "EEG", Some("eeg"), Some("Word.exe"));
        store.commit(&arts, "EEG", Some("eeg"), Some("Word.exe"));
        // english arc with preferred casing after 2 occurrences
        let view = store.view(Some("word.exe"));
        let m = view.matchers();
        let mut found = false;
        m.en.for_matches("eeg", 0, |end, entries| {
            if end == 3 {
                found = entries.iter().any(|e| &*e.text == "EEG");
            }
        });
        assert!(found, "personal english arc should carry user casing");
        // scene bucket knows the word, an unrelated scene falls back to global
        let tok = arts.interner.get("eeg").or(store.ext_map.get("eeg").copied());
        let tok = tok.expect("token interned");
        let p_scene = store.view(Some("word.exe")).p(tok, arts.bos, arts.bos);
        let p_other = store.view(Some("other.exe")).p(tok, arts.bos, arts.bos);
        assert!(p_scene > 0.0 && p_other > 0.0);
        // lexicon bonus active at count 2
        assert!(store.view(None).lexicon_bonus(tok) > 0.0);
    }

    #[test]
    fn alignment_recovers_pinyin_and_oov_phrase_gets_arc() {
        let arts = mini_artifacts();
        let mut store = PersonalStore::new(&arts);
        // "你水" is not a dict word; chars 你(ni) 水 — 水 unknown in mini dict
        // so use 你好你 (3 chars, all readable) which is dict-OOV as one token
        for _ in 0..2 {
            store.commit(&arts, "你好你", Some("nihaoni"), None);
        }
        let tok = store.ext_map.get("你好你").copied().expect("phrase interned");
        assert!(store.zh_added.contains(&tok), "phrase arc after 2 commits");
        let view = store.view(None);
        let mut found = false;
        view.matchers().zh.for_matches("nihaoni", 0, |end, entries| {
            if end == 7 {
                found = entries.iter().any(|e| &*e.text == "你好你");
            }
        });
        assert!(found);
        // merged token got the back-filled unigram mass
        assert!(store.global.uni.get(&tok).copied().unwrap_or(0.0) >= 2.0);
    }

    #[test]
    fn persistence_roundtrip_replays_commits() {
        let arts = mini_artifacts();
        let dir = std::env::temp_dir().join(format!(
            "mochi-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let mut store = PersonalStore::open(&arts, &dir);
            store.commit(&arts, "测试", Some("ceshi"), Some("weixin.exe"));
            assert_eq!(store.n_events, 1);
        }
        {
            let store = PersonalStore::open(&arts, &dir);
            assert_eq!(store.n_events, 1, "journal replayed on reopen");
            let tok = arts.interner.get("测试").unwrap();
            assert!(store.global.knows(tok));
            assert!(store.scenes.contains_key("weixin.exe"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn correction_replay_is_deterministic() {
        let arts = mini_artifacts();
        let dir = std::env::temp_dir().join(format!(
            "mochi-test-corr-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (wrong, right) = (
            arts.interner.get("随即").unwrap(),
            arts.interner.get("随机").unwrap(),
        );
        let p_before;
        {
            let mut store = PersonalStore::open(&arts, &dir);
            store.commit(&arts, "随即梯度", Some("suijitidu"), None);
            store.commit(&arts, "随机梯度", Some("suijitidu"), None);
            let v = store.view(None);
            p_before = (v.p(right, arts.bos, arts.bos), v.p(wrong, arts.bos, arts.bos));
            assert!(p_before.0 > p_before.1, "correction must outweigh");
        }
        {
            // journal stores raw events; replay re-runs correction detection
            let store = PersonalStore::open(&arts, &dir);
            let v = store.view(None);
            let p_after = (v.p(right, arts.bos, arts.bos), v.p(wrong, arts.bos, arts.bos));
            assert!((p_after.0 - p_before.0).abs() < 1e-9);
            assert!((p_after.1 - p_before.1).abs() < 1e-9);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_tsv_loads_lexicon_ngrams_and_arcs() {
        let arts = mini_artifacts();
        let mut store = PersonalStore::new(&arts);
        let tsv = "\
# personal-artifacts-v0
lex\thfo\t12.5
case\thfo\tHFO
lex\t你好你\t4.0
zhword\t你好你\tni hao ni
1\t你好你\t4.0
2\t<s> 你好你\t4.0
";
        let n = store.import_tsv(&arts, tsv).expect("imports");
        assert_eq!(n, 6);
        let hfo = store.ext_map.get("hfo").copied().expect("hfo interned");
        assert!(store.en_added.contains(&hfo));
        assert_eq!(store.en_surface.get(&hfo).map(|s| &**s), Some("HFO"));
        let phrase = store.ext_map.get("你好你").copied().unwrap();
        assert!(store.zh_added.contains(&phrase));
        // imported bigram drives P(你好你 | <s>) to alpha * 4/4 = 0.4
        let view = store.view(None);
        assert!((view.p(phrase, arts.bos, arts.bos) - 0.4).abs() < 1e-9);
    }
}
