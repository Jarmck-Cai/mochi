//! Mochi brain service v0 — named pipe IPC server.
//!
//! Transport per docs/specs/ipc-v0.md:
//! - pipe name `\\.\pipe\mochi-brain-v0`
//! - message mode (PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE): one ReadFile
//!   per request, one WriteFile per response, no length prefix needed
//! - 64KB message cap
//! - server loops on accept; each connected client is served on its own
//!   thread so a plugin reconnect (or a second client) is never blocked

mod protocol;

use std::ptr;
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

fn main() {
    eprintln!("[brain] mochi-brain v0 listening on {}", PIPE_NAME);
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
        std::thread::spawn(move || {
            serve_client(handle, id);
        });
    }
}

fn serve_client(handle: PipeHandle, id: u64) {
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
        let response = protocol::handle_message(&buf[..read as usize]);
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
