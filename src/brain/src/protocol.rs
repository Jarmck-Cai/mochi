//! ipc-v0 message types and dispatch (docs/specs/ipc-v0.md).
//!
//! v0 behavior:
//! - `query`  -> one echo candidate `MOCHI_BRAIN:<input>` with real elapsed_us
//! - `commit` -> log and reply `{"v":1,"ok":true}`
//! - unknown `v` / `method` / malformed JSON -> `{"v":1,"error":"unsupported"}`

use serde::{Deserialize, Serialize};
use std::time::Instant;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Scene {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Incoming request, dispatched on the `method` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "lowercase")]
pub enum Request {
    Query {
        v: u32,
        input: String,
        #[serde(default)]
        seg: Option<[u64; 2]>,
        #[serde(default)]
        scene: Option<Scene>,
        #[serde(default)]
        session: Option<String>,
    },
    Commit {
        v: u32,
        text: String,
        #[serde(default)]
        input: Option<String>,
        #[serde(default)]
        scene: Option<Scene>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub text: String,
    pub comment: String,
    pub preedit: String,
    pub quality: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryResponse {
    pub v: u32,
    pub candidates: Vec<Candidate>,
    pub elapsed_us: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OkResponse {
    pub v: u32,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub v: u32,
    pub error: String,
}

fn unsupported() -> Vec<u8> {
    serde_json::to_vec(&ErrorResponse {
        v: PROTOCOL_VERSION,
        error: "unsupported".to_string(),
    })
    .expect("static error response always serializes")
}

/// Handle one raw message, return the raw response. Never panics on bad
/// input; anything unrecognized gets the `unsupported` error per spec.
pub fn handle_message(raw: &[u8]) -> Vec<u8> {
    let started = Instant::now();
    let request: Request = match serde_json::from_slice(raw) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[brain] unsupported request ({} bytes): {}", raw.len(), e);
            return unsupported();
        }
    };
    match request {
        Request::Query { v, input, .. } => {
            if v != PROTOCOL_VERSION {
                eprintln!("[brain] query with unsupported v={}", v);
                return unsupported();
            }
            // v0: echo candidate proves the end-to-end link. preedit is
            // round-tripped too so the full candidate field set is exercised.
            let candidates = vec![Candidate {
                text: format!("MOCHI_BRAIN:{}", input),
                comment: "echo".to_string(),
                preedit: format!("\u{00ab}{}\u{00bb}", input), // «input»
                quality: 1.0,
            }];
            let elapsed_us = started.elapsed().as_micros() as u64;
            let response = QueryResponse {
                v: PROTOCOL_VERSION,
                candidates,
                elapsed_us,
            };
            eprintln!("[brain] query input='{}' elapsed={}us", input, elapsed_us);
            serde_json::to_vec(&response).expect("query response serializes")
        }
        Request::Commit { v, text, input, .. } => {
            if v != PROTOCOL_VERSION {
                eprintln!("[brain] commit with unsupported v={}", v);
                return unsupported();
            }
            let elapsed_us = started.elapsed().as_micros() as u64;
            eprintln!(
                "[brain] commit text='{}' input='{}' elapsed={}us",
                text,
                input.as_deref().unwrap_or(""),
                elapsed_us
            );
            serde_json::to_vec(&OkResponse {
                v: PROTOCOL_VERSION,
                ok: true,
            })
            .expect("ok response serializes")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_query_request_from_spec() {
        let raw = r#"{
            "v": 1,
            "method": "query",
            "input": "nihao",
            "seg": [0, 5],
            "scene": {"app": "weixin.exe", "title": "..."},
            "session": "weasel-session-id"
        }"#;
        let req: Request = serde_json::from_str(raw).unwrap();
        assert_eq!(
            req,
            Request::Query {
                v: 1,
                input: "nihao".to_string(),
                seg: Some([0, 5]),
                scene: Some(Scene {
                    app: Some("weixin.exe".to_string()),
                    title: Some("...".to_string()),
                }),
                session: Some("weasel-session-id".to_string()),
            }
        );
    }

    #[test]
    fn parses_query_request_with_optional_fields_missing() {
        let raw = r#"{"v":1,"method":"query","input":"a"}"#;
        let req: Request = serde_json::from_str(raw).unwrap();
        match req {
            Request::Query {
                v,
                input,
                seg,
                scene,
                session,
            } => {
                assert_eq!(v, 1);
                assert_eq!(input, "a");
                assert!(seg.is_none() && scene.is_none() && session.is_none());
            }
            _ => panic!("expected query"),
        }
    }

    #[test]
    fn parses_commit_request_from_spec() {
        let raw = r#"{"v": 1, "method": "commit", "text": "你好", "input": "nihao", "scene": {"app": "..."}}"#;
        let req: Request = serde_json::from_str(raw).unwrap();
        match req {
            Request::Commit { v, text, input, .. } => {
                assert_eq!(v, 1);
                assert_eq!(text, "你好");
                assert_eq!(input.as_deref(), Some("nihao"));
            }
            _ => panic!("expected commit"),
        }
    }

    #[test]
    fn query_response_serializes_per_spec() {
        let resp = QueryResponse {
            v: 1,
            candidates: vec![Candidate {
                text: "你好".to_string(),
                comment: "".to_string(),
                preedit: "ni hao".to_string(),
                quality: 1.0,
            }],
            elapsed_us: 1234,
        };
        let json: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(&resp).unwrap()).unwrap();
        assert_eq!(json["v"], 1);
        assert_eq!(json["candidates"][0]["text"], "你好");
        assert_eq!(json["candidates"][0]["preedit"], "ni hao");
        assert_eq!(json["candidates"][0]["quality"], 1.0);
        assert_eq!(json["elapsed_us"], 1234);
    }

    #[test]
    fn handles_query_with_echo_candidate() {
        let raw = br#"{"v":1,"method":"query","input":"nihao","seg":[0,5]}"#;
        let resp: QueryResponse = serde_json::from_slice(&handle_message(raw)).unwrap();
        assert_eq!(resp.v, 1);
        assert_eq!(resp.candidates.len(), 1);
        assert_eq!(resp.candidates[0].text, "MOCHI_BRAIN:nihao");
        assert_eq!(resp.candidates[0].quality, 1.0);
    }

    #[test]
    fn handles_commit_with_ok() {
        let raw = r#"{"v":1,"method":"commit","text":"你好","input":"nihao"}"#;
        let resp: OkResponse = serde_json::from_slice(&handle_message(raw.as_bytes())).unwrap();
        assert_eq!(resp, OkResponse { v: 1, ok: true });
    }

    #[test]
    fn rejects_unknown_method() {
        let raw = br#"{"v":1,"method":"teleport","input":"x"}"#;
        let resp: ErrorResponse = serde_json::from_slice(&handle_message(raw)).unwrap();
        assert_eq!(resp.error, "unsupported");
    }

    #[test]
    fn rejects_unknown_version() {
        let raw = br#"{"v":99,"method":"query","input":"x"}"#;
        let resp: ErrorResponse = serde_json::from_slice(&handle_message(raw)).unwrap();
        assert_eq!(resp.error, "unsupported");
    }

    #[test]
    fn rejects_malformed_json() {
        let resp: ErrorResponse =
            serde_json::from_slice(&handle_message(b"not json at all")).unwrap();
        assert_eq!(resp.error, "unsupported");
    }
}
