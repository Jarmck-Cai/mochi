//! Query engine: artifacts + scorer + decoder + personal memory behind one
//! `query()`/`commit()` pair. Shared across pipe-client threads: the general
//! tables are immutable after load; the personal layer sits behind a RwLock
//! (reads are lock-held only for the ~200µs decode; commits are rare).

use std::path::Path;
use std::sync::RwLock;

use crate::artifacts::Artifacts;
use crate::decoder::{decode_topn, Scorer};
use crate::personal::PersonalStore;
use crate::protocol::Candidate;

pub struct Engine {
    arts: Artifacts,
    personal: RwLock<PersonalStore>,
    pub beam_width: usize,
    pub topn: usize,
}

impl Engine {
    pub fn load(
        dir: &Path,
        user_data: Option<&Path>,
        beam_width: usize,
        topn: usize,
    ) -> Result<Engine, String> {
        let arts = Artifacts::load(dir)?;
        let s = &arts.stats;
        eprintln!(
            "[brain] artifacts loaded from {} in {}ms: dict={} ngrams={}/{}/{} english={} \
             syllables={} est_mem={:.1}MB",
            dir.display(),
            s.elapsed_ms,
            s.dict_entries,
            s.ngrams.0,
            s.ngrams.1,
            s.ngrams.2,
            s.english_words,
            s.syllables,
            s.est_bytes as f64 / (1024.0 * 1024.0),
        );
        let personal = match user_data {
            Some(dir) => {
                let store = PersonalStore::open(&arts, dir);
                eprintln!(
                    "[brain] personal memory at {}: {}",
                    dir.display(),
                    store.stats()
                );
                store
            }
            None => PersonalStore::new(&arts),
        };
        Ok(Engine {
            arts,
            personal: RwLock::new(personal),
            beam_width,
            topn,
        })
    }

    #[cfg(test)]
    pub fn mini() -> Engine {
        let arts = crate::artifacts::tests::mini_artifacts();
        let personal = PersonalStore::new(&arts);
        Engine {
            arts,
            personal: RwLock::new(personal),
            beam_width: 12,
            topn: 5,
        }
    }

    /// Decode one key stream into top-N candidates (ipc-v0 `query`).
    /// `app` selects the scene bucket of the personal layer (ADR-004).
    pub fn query(&self, input: &str, app: Option<&str>) -> Vec<Candidate> {
        let store = self.personal.read().expect("personal lock poisoned");
        let view = store.view(app);
        let scorer = Scorer::full(&self.arts.lm, &view);
        decode_topn(
            input,
            &self.arts.builder,
            Some(view.matchers()),
            &scorer,
            self.arts.bos,
            self.beam_width,
            self.topn,
        )
        .into_iter()
        .map(|r| Candidate {
            text: r.text,
            comment: String::new(),
            preedit: r.preedit,
            quality: r.score,
        })
        .collect()
    }

    /// Instant learning from one committed text (ipc-v0 `commit`): the next
    /// query already sees the updated memory.
    pub fn commit(&self, text: &str, input: Option<&str>, app: Option<&str>) {
        let mut store = self.personal.write().expect("personal lock poisoned");
        store.commit(&self.arts, text, input, app);
    }

    pub fn personal_stats(&self) -> String {
        self.personal
            .read()
            .map(|s| s.stats())
            .unwrap_or_else(|_| "lock poisoned".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_returns_ranked_candidates_with_preedit() {
        let e = Engine::mini();
        let cands = e.query("nihao", None);
        assert_eq!(cands[0].text, "你好");
        assert_eq!(cands[0].preedit, "ni hao");
        assert!(cands.len() <= 5);
        assert!(cands.windows(2).all(|w| w[0].quality >= w[1].quality));
    }

    #[test]
    fn query_handles_empty_and_garbage() {
        let e = Engine::mini();
        assert!(e.query("", None).is_empty());
        assert!(e.query("好", None).is_empty()); // non-ASCII guard
        assert!(!e.query("vvv", None).is_empty()); // fallback still answers
    }

    /// The M2-3 acceptance anchor in miniature: the general LM prefers 随即
    /// over 随机 before 梯度; one corrected commit flips the very next query.
    #[test]
    fn one_commit_flips_suijitidu() {
        let e = Engine::mini();
        assert_eq!(e.query("suijitidu", None)[0].text, "随即梯度");
        e.commit("随机梯度", Some("suijitidu"), None);
        assert_eq!(e.query("suijitidu", None)[0].text, "随机梯度");
    }

    /// Scene bucketing: what you teach it in one app must not leak into
    /// another scene's ranking when the habit conflicts — but global memory
    /// still backs off when the other scene is silent (ADR-004 ladder).
    #[test]
    fn scene_buckets_specialize_ranking() {
        let e = Engine::mini();
        e.commit("随机梯度", Some("suijitidu"), Some("code.exe"));
        // same scene: learned immediately
        assert_eq!(e.query("suijitidu", Some("code.exe"))[0].text, "随机梯度");
        // other scene: global personal layer still carries the correction
        assert_eq!(e.query("suijitidu", Some("weixin.exe"))[0].text, "随机梯度");
    }

    #[test]
    fn english_personal_term_with_casing_wins() {
        let e = Engine::mini();
        // teach the user's casing habit twice (lexicon threshold)
        e.commit("EEG", Some("eeg"), None);
        e.commit("EEG", Some("eeg"), None);
        let cands = e.query("eeg", None);
        assert_eq!(cands[0].text, "EEG");
    }
}
