//! Small Win32 helpers shared by the modules.

use std::sync::mpsc::Sender;

use ngambang_core::model::Event;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WPARAM};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetTickCount64};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Posted to the island window whenever a worker queued an event.
pub const WM_APP_EVENT: u32 = WM_APP + 1;
pub const WM_APP_TRAY: u32 = WM_APP + 2;
pub const WM_APP_APPBAR: u32 = WM_APP + 3;
pub const WM_APP_FOREGROUND: u32 = WM_APP + 4;
/// The Settings window saved config.toml.
pub const WM_APP_SETTINGS: u32 = WM_APP + 5;
/// A network interface changed (from the IP helper's callback).
pub const WM_APP_NET: u32 = WM_APP + 6;

/// Monotonic milliseconds.
pub fn now_ms() -> u64 {
    unsafe { GetTickCount64() }
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Local date as "YYYY-MM-DD".
pub fn today() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

/// Milliseconds until the next local midnight (+1 s).
pub fn ms_until_midnight() -> u64 {
    let t = unsafe { GetLocalTime() };
    let elapsed = (t.wHour as u64 * 3600 + t.wMinute as u64 * 60 + t.wSecond as u64) * 1000 + t.wMilliseconds as u64;
    86_400_000u64.saturating_sub(elapsed) + 1000
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Sends events from worker threads to the UI thread.
#[derive(Clone)]
pub struct Bus {
    tx: Sender<Event>,
    hwnd: isize,
}

impl Bus {
    pub fn new(tx: Sender<Event>, hwnd: HWND) -> Bus {
        Bus { tx, hwnd: hwnd.0 as isize }
    }

    pub fn send(&self, ev: Event) {
        if self.tx.send(ev).is_ok() {
            unsafe {
                let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_APP_EVENT, WPARAM(0), LPARAM(0));
            }
        }
    }
}

/// Closes a kernel handle on drop.
pub struct OwnedHandle(pub HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() && !self.0 .0.is_null() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}

/// Full image path of a process, if we may query it.
pub fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OwnedHandle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// The executable's FileDescription ("Spotify", "Google Chrome"), if any.
pub fn file_description(path: &str) -> Option<String> {
    unsafe {
        let p = HSTRING::from(path);
        let size = GetFileVersionInfoSizeW(&p, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&p, None, size, data.as_mut_ptr().cast()).ok()?;
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        // Language/codepage pairs.
        if !VerQueryValueW(data.as_ptr().cast(), &HSTRING::from("\\VarFileInfo\\Translation"), &mut ptr, &mut len).as_bool() || len < 4 {
            return None;
        }
        let pair = *(ptr as *const [u16; 2]);
        let key = format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription", pair[0], pair[1]);
        if !VerQueryValueW(data.as_ptr().cast(), &HSTRING::from(key), &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr as *const u16, len as usize));
        let s = s.trim_end_matches('\0').trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

/// A display name for a process: its FileDescription, else the exe stem.
pub fn process_display_name(path: &str) -> String {
    file_description(path).unwrap_or_else(|| {
        let exe = path.rsplit(['\\', '/']).next().unwrap_or(path);
        exe.strip_suffix(".exe").or_else(|| exe.strip_suffix(".EXE")).unwrap_or(exe).to_string()
    })
}

/// The computer's current UTC offset in minutes.
pub fn local_offset_min() -> i32 {
    match clock_event() {
        ngambang_core::model::Event::Clock { utc_offset_min, .. } => utc_offset_min,
        _ => 0,
    }
}

/// The local wall clock for the core model (date, minutes, UTC offset).
pub fn clock_event() -> ngambang_core::model::Event {
    use windows::Win32::System::SystemInformation::GetSystemTime;
    let (l, u) = unsafe { (GetLocalTime(), GetSystemTime()) };
    let minutes = |t: &windows::Win32::Foundation::SYSTEMTIME| {
        t.wHour as f64 * 60.0 + t.wMinute as f64 + t.wSecond as f64 / 60.0 + t.wMilliseconds as f64 / 60_000.0
    };
    let date = |t: &windows::Win32::Foundation::SYSTEMTIME| (t.wYear, t.wMonth, t.wDay);
    let day_shift = match date(&l).cmp(&date(&u)) {
        std::cmp::Ordering::Greater => 1440.0,
        std::cmp::Ordering::Less => -1440.0,
        std::cmp::Ordering::Equal => 0.0,
    };
    let offset = (minutes(&l) - minutes(&u) + day_shift).round() as i32;
    ngambang_core::model::Event::Clock {
        year: l.wYear as i32,
        month: l.wMonth as u32,
        day: l.wDay as u32,
        minutes: minutes(&l),
        utc_offset_min: offset,
        unix: unix_now(),
    }
}
