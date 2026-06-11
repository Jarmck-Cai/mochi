//! Mochi brain service v0 — named pipe IPC server.
//!
//! Transport per docs/specs/ipc-v0.md:
//! - pipe name `\\.\pipe\mochi-brain-v0`
//! - message mode (PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE): one ReadFile
//!   per request, one WriteFile per response, no length prefix needed
//! - 64KB message cap
//! - server loops on accept; each connected client is served on its own
//!   thread so a plugin reconnect (or a second client) is never blocked

mod artifacts;
mod decoder;
mod engine;
mod interner;
mod lattice;
mod lm;
mod personal;
mod protocol;

use std::path::PathBuf;
use std::ptr;
use std::sync::Arc;

use engine::Engine;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_BROKEN_PIPE, ERROR_MORE_DATA, ERROR_PIPE_CONNECTED,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    FlushFileBuffers, ReadFile, WriteFile, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_MESSAGE,
    PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

const PIPE_NAME: &str = r"\\.\pipe\mochi-brain-v0";
const MAX_MESSAGE: usize = 64 * 1024;

/// Raw HANDLE is a pointer type and not Send; pass it across the thread
/// boundary as the integer it really is. The accept loop transfers exclusive
/// ownership of each connected instance to its serving thread.
struct PipeHandle(isize);
unsafe impl Send for PipeHandle {}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Default artifacts dir: `artifacts/v0` relative to the repo root — try the
/// cwd first, then walk up from the exe location (target/release/... -> repo).
fn default_artifacts_dir() -> PathBuf {
    let rel = PathBuf::from("artifacts").join("v0");
    if rel.is_dir() {
        return rel;
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        while let Some(d) = dir {
            let cand = d.join("artifacts").join("v0");
            if cand.is_dir() {
                return cand;
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    rel
}

struct Cli {
    artifacts: PathBuf,
    user_data: Option<PathBuf>,
    beam_width: usize,
    topn: usize,
    bench: bool,
    decode: Vec<String>,
}

fn parse_cli() -> Cli {
    let mut cli = Cli {
        artifacts: default_artifacts_dir(),
        user_data: None,
        beam_width: 12,
        topn: 5,
        bench: false,
        decode: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    let mut no_user_data = false;
    while let Some(arg) = args.next() {
        let mut take = |what: &str| {
            args.next()
                .unwrap_or_else(|| panic!("{} requires a value", what))
        };
        match arg.as_str() {
            "--artifacts" => cli.artifacts = PathBuf::from(take("--artifacts")),
            "--user-data" => cli.user_data = Some(PathBuf::from(take("--user-data"))),
            "--no-user-data" => no_user_data = true,
            "--beam" => cli.beam_width = take("--beam").parse().expect("--beam: number"),
            "--topn" => cli.topn = take("--topn").parse().expect("--topn: number"),
            "--bench" => cli.bench = true,
            "--decode" => {
                cli.decode.extend(args.by_ref());
                break;
            }
            other => {
                eprintln!(
                    "usage: mochi-brain [--artifacts <dir>] [--user-data <dir> | --no-user-data] \
                     [--beam N] [--topn N] [--bench | --decode <keys>...]\nunknown argument: {}",
                    other
                );
                std::process::exit(2);
            }
        }
    }
    // Personal memory lives next to the artifacts by default ("user_data"
    // sibling); --no-user-data runs stateless (bench/parity checks).
    if cli.user_data.is_none() && !no_user_data {
        let dir = cli
            .artifacts
            .parent()
            .map(|p| p.join("..").join("user_data"))
            .unwrap_or_else(|| PathBuf::from("user_data"));
        cli.user_data = Some(dir);
    }
    cli
}

/// `--bench`: per-keystream-length decode latency (median/max over the full
/// query path = lattice + beam search + candidate materialization).
fn run_bench(engine: &Engine) {
    // realistic all-pinyin stream, truncated per length (29 keys)
    const BASE: &str = "woyaoceshizhongwenshurufangfa";
    const ITERS: usize = 200;
    println!("len\tkeys\tmedian_us\tmax_us\ttop1");
    for len in 1..=20usize {
        let keys = &BASE[..len];
        // warmup
        for _ in 0..5 {
            std::hint::black_box(engine.query(keys, None));
        }
        let mut samples = Vec::with_capacity(ITERS);
        let mut top1 = String::new();
        for _ in 0..ITERS {
            let t0 = std::time::Instant::now();
            let cands = std::hint::black_box(engine.query(keys, None));
            samples.push(t0.elapsed().as_micros() as u64);
            if top1.is_empty() {
                if let Some(c) = cands.first() {
                    top1 = c.text.clone();
                }
            }
        }
        samples.sort_unstable();
        let median = samples[samples.len() / 2];
        let max = *samples.last().unwrap();
        println!("{}\t{}\t{}\t{}\t{}", len, keys, median, max, top1);
    }
}

fn main() {
    let cli = parse_cli();
    let engine = match Engine::load(
        &cli.artifacts,
        cli.user_data.as_deref(),
        cli.beam_width,
        cli.topn,
    ) {
        Ok(e) => Arc::new(e),
        Err(e) => {
            eprintln!("[brain] failed to load artifacts: {}", e);
            std::process::exit(1);
        }
    };
    if cli.bench {
        run_bench(&engine);
        return;
    }
    if !cli.decode.is_empty() {
        // one-shot decode for parity checks / debugging: keys<TAB>text<TAB>score
        for keys in &cli.decode {
            let t0 = std::time::Instant::now();
            let cands = engine.query(keys, None);
            let us = t0.elapsed().as_micros();
            for (i, c) in cands.iter().enumerate() {
                println!(
                    "{}\t#{}\t{}\t{}\t{:.4}\t{}us",
                    keys,
                    i + 1,
                    c.text,
                    c.preedit,
                    c.quality,
                    us
                );
            }
        }
        return;
    }
    eprintln!("[brain] mochi-brain listening on {}", PIPE_NAME);
    let name = wide(PIPE_NAME);
    let mut client_seq: u64 = 0;
    loop {
        // A fresh instance per client; PIPE_UNLIMITED_INSTANCES allows
        // concurrent clients, each instance owned by one serving thread.
        let pipe = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                PIPE_UNLIMITED_INSTANCES,
                MAX_MESSAGE as u32,
                MAX_MESSAGE as u32,
                0,
                ptr::null(),
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            eprintln!(
                "[brain] CreateNamedPipeW failed (err={}), retrying in 1s",
                unsafe { GetLastError() }
            );
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }
        let connected = unsafe { ConnectNamedPipe(pipe, ptr::null_mut()) };
        if connected == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
            // e.g. client connected and vanished between create and connect
            unsafe { CloseHandle(pipe) };
            continue;
        }
        client_seq += 1;
        let id = client_seq;
        eprintln!("[brain] client #{} connected", id);
        let handle = PipeHandle(pipe as isize);
        let engine = Arc::clone(&engine);
        std::thread::spawn(move || {
            serve_client(handle, id, &engine);
        });
    }
}

fn serve_client(handle: PipeHandle, id: u64, engine: &Engine) {
    let pipe = handle.0 as *mut core::ffi::c_void;
    let mut buf = vec![0u8; MAX_MESSAGE];
    loop {
        let mut read: u32 = 0;
        let ok = unsafe {
            ReadFile(
                pipe,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut read,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            if err == ERROR_MORE_DATA {
                // message larger than the 64KB protocol cap: drain is not
                // worth it in v0; drop the client, it will reconnect
                eprintln!("[brain] client #{}: message over 64KB cap, dropping", id);
            } else if err != ERROR_BROKEN_PIPE {
                eprintln!("[brain] client #{}: read error {}", id, err);
            }
            break;
        }
        if read == 0 {
            break; // client closed its end
        }
        let response = protocol::handle_message(engine, &buf[..read as usize]);
        let mut written: u32 = 0;
        let ok = unsafe {
            WriteFile(
                pipe,
                response.as_ptr(),
                response.len() as u32,
                &mut written,
                ptr::null_mut(),
            )
        };
        if ok == 0 || written as usize != response.len() {
            eprintln!("[brain] client #{}: write error {}", id, unsafe {
                GetLastError()
            });
            break;
        }
    }
    unsafe {
        FlushFileBuffers(pipe);
        DisconnectNamedPipe(pipe);
        CloseHandle(pipe);
    }
    eprintln!("[brain] client #{} disconnected", id);
}
