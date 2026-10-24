//! Clipboard read/write on the UI thread (SPEC §6.5). Content marked private
//! by password managers is never read.

use windows::core::{w, Result};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT};
use windows::Win32::UI::Shell::DROPFILES;

const MAX_CHARS: usize = ngambang_core::clipboard::MAX_TEXT;

fn format(name: windows::core::PCWSTR) -> u32 {
    unsafe { RegisterClipboardFormatW(name) }
}

fn self_format() -> u32 {
    format(w!("Ngambang.Self"))
}

struct Open;

impl Open {
    fn new(hwnd: HWND) -> Option<Open> {
        for _ in 0..5 {
            if unsafe { OpenClipboard(Some(hwnd)) }.is_ok() {
                return Some(Open);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn available(f: u32) -> bool {
    f != 0 && unsafe { IsClipboardFormatAvailable(f) }.is_ok()
}

/// The clipboard text, unless it is private, ours, or too large.
pub fn read_text(hwnd: HWND) -> Option<String> {
    let private = [w!("ExcludeClipboardContentFromMonitorProcessing"), w!("Clipboard Viewer Ignore")];
    if private.iter().any(|f| available(format(*f))) || available(self_format()) {
        return None;
    }
    let _open = Open::new(hwnd)?;
    unsafe {
        // "CanIncludeInClipboardHistory" = 0 also means private.
        let history = format(w!("CanIncludeInClipboardHistory"));
        if available(history) {
            if let Ok(h) = GetClipboardData(history) {
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u32;
                let allowed = p.is_null() || *p != 0;
                let _ = GlobalUnlock(g);
                if !allowed {
                    return None;
                }
            }
        }
        if !available(CF_UNICODETEXT.0 as u32) {
            return None;
        }
        let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
        let g = HGLOBAL(h.0);
        let p = GlobalLock(g) as *const u16;
        if p.is_null() {
            return None;
        }
        let max = (GlobalSize(g) / 2).min(MAX_CHARS + 1);
        let slice = std::slice::from_raw_parts(p, max);
        let len = slice.iter().position(|c| *c == 0).unwrap_or(max);
        let text = (len <= MAX_CHARS).then(|| String::from_utf16_lossy(&slice[..len]));
        let _ = GlobalUnlock(g);
        text
    }
}

unsafe fn alloc(bytes: &[u8]) -> Result<HGLOBAL> {
    let g = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
    let p = GlobalLock(g) as *mut u8;
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
    let _ = GlobalUnlock(g);
    Ok(g)
}

fn set(hwnd: HWND, format_id: u32, bytes: &[u8]) -> Result<()> {
    let _open = Open::new(hwnd).ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_ACCESSDENIED))?;
    unsafe {
        EmptyClipboard()?;
        SetClipboardData(format_id, Some(HANDLE(alloc(bytes)?.0)))?;
        SetClipboardData(self_format(), Some(HANDLE(alloc(&[1])?.0)))?;
    }
    Ok(())
}

pub fn write_text(hwnd: HWND, text: &str) -> Result<()> {
    let w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes: Vec<u8> = w.iter().flat_map(|c| c.to_le_bytes()).collect();
    set(hwnd, CF_UNICODETEXT.0 as u32, &bytes)
}

/// Put a file on the clipboard (paste into Explorer, chats, editors).
pub fn write_file(hwnd: HWND, path: &str) -> Result<()> {
    let header = DROPFILES { pFiles: std::mem::size_of::<DROPFILES>() as u32, fWide: true.into(), ..Default::default() };
    let mut bytes =
        unsafe { std::slice::from_raw_parts(&header as *const DROPFILES as *const u8, std::mem::size_of::<DROPFILES>()) }.to_vec();
    for c in path.encode_utf16().chain([0, 0]) {
        bytes.extend_from_slice(&c.to_le_bytes());
    }
    set(hwnd, CF_HDROP.0 as u32, &bytes)
}
