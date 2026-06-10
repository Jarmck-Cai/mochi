//! Query engine: artifacts + scorer + decoder behind one `query()` call.
//! Shared read-only across pipe-client threads (all tables are immutable
//! after load; the personal layer will add interior mutability in M2-3).

use std::path::Path;

use crate::artifacts::Artifacts;
use crate::decoder::{decode_topn, Scorer};
use crate::lm::{NoPersonalLayer, PersonalLayer};
use crate::protocol::Candidate;

static NO_PERSONAL: NoPersonalLayer = NoPersonalLayer;

pub struct Engine {
    arts: Artifacts,
    /// λ₂ slot: swapped for the real personal layer in M2-3.
    personal: &'static dyn PersonalLayer,
    pub beam_width: usize,
    pub topn: usize,
}

impl Engine {
    pub fn load(dir: &Path, beam_width: usize, topn: usize) -> Result<Engine, String> {
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
        Ok(Engine {
            arts,
            personal: &NO_PERSONAL,
            beam_width,
            topn,
        })
    }

    #[cfg(test)]
    pub fn mini() -> Engine {
        Engine {
            arts: crate::artifacts::tests::mini_artifacts(),
            personal: &NO_PERSONAL,
            beam_width: 12,
            topn: 5,
        }
    }

    /// Decode one key stream into top-N candidates (ipc-v0 `query`).
    pub fn query(&self, input: &str) -> Vec<Candidate> {
        let scorer = Scorer::general_only(&self.arts.lm, self.personal);
        decode_topn(
            input,
            &self.arts.builder,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_returns_ranked_candidates_with_preedit() {
        let e = Engine::mini();
        let cands = e.query("nihao");
        assert_eq!(cands[0].text, "你好");
        assert_eq!(cands[0].preedit, "ni hao");
        assert!(cands.len() <= 5);
        assert!(cands.windows(2).all(|w| w[0].quality >= w[1].quality));
    }

    #[test]
    fn query_handles_empty_and_garbage() {
        let e = Engine::mini();
        assert!(e.query("").is_empty());
        assert!(e.query("好").is_empty()); // non-ASCII guard
        assert!(!e.query("vvv").is_empty()); // fallback still answers
    }
}
