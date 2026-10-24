//! Named pipe between the CLI and the island (SPEC §6.2). The pipe only
//! accepts the current user and local clients.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ngambang_core::model::Event;
use ngambang_core::protocol::{self, RateLimiter, Response};
use windows::core::{Result, HSTRING, PWSTR};
use windows::Win32::Foundation::{
    GetLastError, LocalFree, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE,
    HANDLE, HLOCAL,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_SHARE_NONE,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, WaitNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::util::{now_ms, Bus, OwnedHandle};

const MAX_CLIENTS: usize = 16;

/// The current user's SID as a string ("S-1-5-21-…").
pub fn user_sid() -> Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let token = OwnedHandle(token);
        let mut len = 0u32;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        GetTokenInformation(token.0, TokenUser, Some(buf.as_mut_ptr().cast()), len, &mut len)?;
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut s)?;
        let out = s.to_string().unwrap_or_default();
        let _ = LocalFree(Some(HLOCAL(s.0.cast())));
        Ok(out)
    }
}

pub fn default_pipe_name() -> Result<String> {
    Ok(protocol::pipe_name(&user_sid()?))
}

struct Security {
    sd: PSECURITY_DESCRIPTOR,
}

impl Security {
    fn for_user(sid: &str) -> Result<Security> {
        // Full access for this user and SYSTEM only; protected from inheritance.
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{sid})(A;;GA;;;SY)"));
        let mut sd = PSECURITY_DESCRIPTOR::default();
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None)? };
        Ok(Security { sd })
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.sd.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for Security {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.sd.0)));
        }
    }
}

unsafe impl Send for Security {}

fn create_instance(name: &HSTRING, sec: &Security, first: bool) -> Result<OwnedHandle> {
    let sa = sec.attributes();
    let mut flags = PIPE_ACCESS_DUPLEX;
    if first {
        flags |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    let h = unsafe {
        CreateNamedPipeW(
            name,
            FILE_FLAGS_AND_ATTRIBUTES(flags.0),
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            4096,
            4096,
            0,
            Some(&sa),
        )
    };
    if h.is_invalid() {
        return Err(windows::core::Error::from_thread());
    }
    Ok(OwnedHandle(h))
}

/// Accept clients forever on a background thread.
/// `accepting` is consulted per request so a disabled module can say so.
pub fn serve(name: String, bus: Bus, accepting: Arc<std::sync::atomic::AtomicBool>) -> Result<()> {
    let sid = user_sid()?;
    let sec = Security::for_user(&sid)?;
    let wname = HSTRING::from(name.as_str());
    // Create the first instance now so a second island fails fast.
    let first = create_instance(&wname, &sec, true)?;
    std::thread::Builder::new()
        .name("ngambang-pipe".into())
        .spawn(move || {
            let clients = Arc::new(AtomicUsize::new(0));
            let mut next = Some(first);
            loop {
                let pipe = match next.take() {
                    Some(p) => p,
                    None => match create_instance(&wname, &sec, false) {
                        Ok(p) => p,
                        Err(e) => {
                            log_warn!("pipe: create failed: {e}");
                            std::thread::sleep(std::time::Duration::from_secs(1));
                            continue;
                        }
                    },
                };
                let connected = unsafe { ConnectNamedPipe(pipe.0, None) };
                if connected.is_err() && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
                    continue;
                }
                if clients.load(Ordering::SeqCst) >= MAX_CLIENTS {
                    let _ = write_all(pipe.0, (Response::error("busy").to_line() + "\n").as_bytes());
                    unsafe {
                        let _ = DisconnectNamedPipe(pipe.0);
                    }
                    continue;
                }
                clients.fetch_add(1, Ordering::SeqCst);
                let (bus, clients, accepting) = (bus.clone(), clients.clone(), accepting.clone());
                let _ = std::thread::Builder::new().name("ngambang-pipe-client".into()).spawn(move || {
                    handle_client(&pipe, &bus, &accepting);
                    unsafe {
                        let _ = FlushFileBuffers(pipe.0);
                        let _ = DisconnectNamedPipe(pipe.0);
                    }
                    clients.fetch_sub(1, Ordering::SeqCst);
                });
            }
        })
        .map_err(|_| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))?;
    Ok(())
}

fn handle_client(pipe: &OwnedHandle, bus: &Bus, accepting: &std::sync::atomic::AtomicBool) {
    let mut limiter = RateLimiter::default();
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut read = 0u32;
        let ok = unsafe { ReadFile(pipe.0, Some(&mut buf), Some(&mut read), None) };
        if ok.is_err() || read == 0 {
            break;
        }
        pending.extend_from_slice(&buf[..read as usize]);
        while let Some(pos) = pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = pending.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&line);
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            let reply = if !limiter.allow(now_ms()) {
                Response::error("rate limited")
            } else {
                match protocol::parse_line(text) {
                    Ok(protocol::Request::Ping) => Response::ok(),
                    Ok(_) if !accepting.load(Ordering::SeqCst) => Response::error("the commands module is turned off"),
                    Ok(req) => {
                        bus.send(Event::Pipe(req));
                        Response::ok()
                    }
                    Err(e) => Response::error(e),
                }
            };
            if write_all(pipe.0, (reply.to_line() + "\n").as_bytes()).is_err() {
                return;
            }
        }
        if pending.len() > protocol::MAX_LINE {
            let _ = write_all(pipe.0, (Response::error("line too long").to_line() + "\n").as_bytes());
            return;
        }
    }
}

fn write_all(h: HANDLE, mut data: &[u8]) -> Result<()> {
    while !data.is_empty() {
        let mut written = 0u32;
        unsafe { WriteFile(h, Some(data), Some(&mut written), None)? };
        if written == 0 {
            break;
        }
        data = &data[written as usize..];
    }
    Ok(())
}

#[derive(Debug)]
pub enum SendError {
    NotRunning,
    Rejected(String),
    Io(String),
}

/// Send one request line and wait for the reply (CLI side).
pub fn send(name: &str, line: &str) -> std::result::Result<(), SendError> {
    let wname = HSTRING::from(name);
    let pipe = loop {
        let h = unsafe {
            CreateFileW(&wname, (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_NONE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None)
        };
        match h {
            Ok(h) => break OwnedHandle(h),
            Err(_) => match unsafe { GetLastError() } {
                ERROR_PIPE_BUSY => unsafe {
                    if !WaitNamedPipeW(&wname, 2000).as_bool() {
                        return Err(SendError::Io("island is busy".into()));
                    }
                },
                ERROR_FILE_NOT_FOUND => return Err(SendError::NotRunning),
                e => return Err(SendError::Io(format!("cannot open pipe ({})", e.0))),
            },
        }
    };
    write_all(pipe.0, format!("{line}\n").as_bytes()).map_err(|e| SendError::Io(e.to_string()))?;
    let mut reply = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        let mut read = 0u32;
        let r = unsafe { ReadFile(pipe.0, Some(&mut buf), Some(&mut read), None) };
        if r.is_err() {
            if unsafe { GetLastError() } == ERROR_BROKEN_PIPE {
                break;
            }
            return Err(SendError::Io("read failed".into()));
        }
        if read == 0 {
            break;
        }
        reply.extend_from_slice(&buf[..read as usize]);
        if reply.contains(&b'\n') {
            break;
        }
    }
    let text = String::from_utf8_lossy(&reply);
    match serde_json::from_str::<Response>(text.trim()) {
        Ok(r) if r.ok => Ok(()),
        Ok(r) => Err(SendError::Rejected(r.error.unwrap_or_else(|| "rejected".into()))),
        Err(_) => Err(SendError::Io("no reply".into())),
    }
}
