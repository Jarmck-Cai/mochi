#![cfg(windows)]
#[path = "../src/ipc.rs"]
mod ipc;

use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn duplicate_owner_is_rejected_and_drop_restores_namespace() {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let name = format!(r"\\.\pipe\mochi-team-test-{}-{}", std::process::id(), nonce);
    let owner = ipc::PipeListener::bind(&name).expect("first synthetic owner");
    assert!(ipc::PipeListener::bind(&name).is_err(), "second owner was accepted");
    drop(owner);
    let next = ipc::PipeListener::bind(&name).expect("Drop must release synthetic namespace");
    drop(next);
}
