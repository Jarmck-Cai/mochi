//! Named-pipe ownership and current-process-user isolation.
use std::io;
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::ptr;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_MESSAGE,
    PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct LocalAllocation(*mut core::ffi::c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

pub struct PipeListener {
    name: Vec<u16>,
    descriptor: LocalAllocation,
    pending: OwnedHandle,
}

impl PipeListener {
    pub fn bind(name: &str) -> io::Result<Self> {
        if name.encode_utf16().any(|c| c == 0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "pipe name contains NUL",
            ));
        }
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let descriptor = current_user_descriptor()?;
        let pending = create_instance(&name, &descriptor, true)?;
        Ok(Self {
            name,
            descriptor,
            pending,
        })
    }

    /// Transfers a connected instance only after reserving its successor.
    /// The caller must disconnect and close the returned HANDLE.
    pub fn accept(&mut self) -> io::Result<isize> {
        unsafe {
            if ConnectNamedPipe(self.pending.0, ptr::null_mut()) == 0 {
                let error = GetLastError();
                if error != ERROR_PIPE_CONNECTED {
                    if error == ERROR_NO_DATA {
                        // Reuse the same instance: closing it would release ownership.
                        if DisconnectNamedPipe(self.pending.0) == 0 {
                            return Err(io::Error::last_os_error());
                        }
                    }
                    return Err(io::Error::from_raw_os_error(error as i32));
                }
            }
        }
        let next = match create_instance(&self.name, &self.descriptor, false) {
            Ok(next) => next,
            Err(error) => {
                // Retain the namespace and make this instance reusable on retry.
                unsafe {
                    DisconnectNamedPipe(self.pending.0);
                }
                return Err(error);
            }
        };
        let connected = std::mem::replace(&mut self.pending, next);
        let raw = connected.0 as isize;
        std::mem::forget(connected);
        Ok(raw)
    }
}

impl AsRawHandle for PipeListener {
    fn as_raw_handle(&self) -> RawHandle {
        self.pending.0
    }
}

fn create_instance(
    name: &[u16],
    descriptor: &LocalAllocation,
    first: bool,
) -> io::Result<OwnedHandle> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let pipe = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX
                | if first {
                    FILE_FLAG_FIRST_PIPE_INSTANCE
                } else {
                    0
                },
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            64 * 1024,
            64 * 1024,
            0,
            &attributes,
        )
    };
    if pipe == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(OwnedHandle(pipe))
    }
}

fn current_user_descriptor() -> io::Result<LocalAllocation> {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle(token);
        let mut needed = 0;
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        // usize storage supplies the alignment required by TOKEN_USER.
        let mut storage = vec![
            0usize;
            (needed as usize + std::mem::size_of::<usize>() - 1)
                / std::mem::size_of::<usize>()
        ];
        if GetTokenInformation(
            token.0,
            TokenUser,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*(storage.as_ptr() as *const TOKEN_USER);
        let mut sid = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut sid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let sid_allocation = LocalAllocation(sid.cast());
        let mut len = 0;
        while *sid.add(len) != 0 {
            len += 1;
        }
        let sid_text = String::from_utf16(std::slice::from_raw_parts(sid, len))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid token SID"))?;
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{})", sid_text)
            .encode_utf16()
            .chain(Some(0))
            .collect();
        drop(sid_allocation);
        let mut descriptor = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(LocalAllocation(descriptor))
    }
}
