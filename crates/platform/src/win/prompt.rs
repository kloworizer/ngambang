//! The "Add task" box: a small native window with one text field, opened next
//! to the island. Enter adds the task and closes it, Esc just closes it.

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};

use ngambang_core::model::Event;
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateFontIndirectW, DeleteObject, GetSysColorBrush, COLOR_WINDOW, HFONT, LOGFONTW};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::util::{from_wide, Bus};

/// The open box, for `IsDialogMessageW` in the message loop.
pub static PROMPT_HWND: AtomicIsize = AtomicIsize::new(0);

const ID_ADD: usize = 1; // IDOK: Enter
const ID_CANCEL: usize = 2; // IDCANCEL: Esc
const EM_SETCUEBANNER: u32 = 0x1501;
const EM_LIMITTEXT: u32 = 0x00C5;
const W: i32 = 380;
const H: i32 = 92;

struct Prompt {
    edit: HWND,
    font: HFONT,
    bus: Bus,
}

thread_local! {
    static PROMPT: RefCell<Option<Prompt>> = const { RefCell::new(None) };
}

/// Open the box beside `near` (the island, in screen pixels).
pub fn open(owner: HWND, near: RECT, bus: Bus) {
    let existing = PROMPT_HWND.load(Ordering::SeqCst);
    if existing != 0 {
        unsafe {
            let _ = SetForegroundWindow(HWND(existing as *mut _));
        }
        return;
    }
    if let Err(e) = create(owner, near, bus) {
        log_warn!("prompt: {e}");
    }
}

fn create(owner: HWND, near: RECT, bus: Bus) -> windows::core::Result<()> {
    let hinstance = unsafe { GetModuleHandleW(None)? };
    let class = w!("NgambangPrompt");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: hinstance.into(),
        lpszClassName: class,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
        hIcon: super::app::app_icon(32),
        ..Default::default()
    };
    unsafe { RegisterClassExW(&wc) };
    let dpi = unsafe { GetDpiForWindow(owner) }.max(96);
    let s = |v: i32| (v as f32 * dpi as f32 / 96.0).round() as i32;
    let style = WS_POPUP | WS_CAPTION | WS_SYSMENU;
    let mut r = RECT { left: 0, top: 0, right: s(W), bottom: s(H) };
    unsafe {
        let _ = windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(&mut r, style, false, WS_EX_TOOLWINDOW, dpi);
    }
    let (ww, wh) = (r.right - r.left, r.bottom - r.top);
    // Beside the island, on whichever side has room, kept on screen.
    let (sw, sh) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let gap = s(12);
    let mut x = if near.right + gap + ww <= sw { near.right + gap } else { near.left - gap - ww };
    let mut y = (near.top + near.bottom) / 2 - wh / 2;
    if near.bottom - near.top < s(60) && near.top < sh / 2 {
        // A small island at the top: open below it instead.
        x = (near.left + near.right) / 2 - ww / 2;
        y = near.bottom + gap;
    }
    x = x.clamp(0, (sw - ww).max(0));
    y = y.clamp(0, (sh - wh).max(0));
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_CONTROLPARENT,
            class,
            w!("Add task"),
            style,
            x,
            y,
            ww,
            wh,
            None,
            None,
            Some(hinstance.into()),
            None,
        )?
    };
    let mut lf = LOGFONTW { lfHeight: -((10.0 * dpi as f32 / 72.0).round() as i32), lfWeight: 400, ..Default::default() };
    let face: Vec<u16> = "Segoe UI".encode_utf16().collect();
    lf.lfFaceName[..face.len()].copy_from_slice(&face);
    let font = unsafe { CreateFontIndirectW(&lf) };
    let child = |class: windows::core::PCWSTR, text: &str, style: u32, ex: WINDOW_EX_STYLE, r: (i32, i32, i32, i32), id: usize| unsafe {
        let h = CreateWindowExW(
            ex,
            class,
            &HSTRING::from(text),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(style),
            s(r.0),
            s(r.1),
            s(r.2),
            s(r.3),
            Some(hwnd),
            Some(HMENU(id as *mut _)),
            Some(hinstance.into()),
            None,
        )
        .unwrap_or_default();
        SendMessageW(h, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        h
    };
    let edit = child(w!("EDIT"), "", ES_AUTOHSCROLL as u32, WS_EX_CLIENTEDGE, (14, 14, W - 28, 28), 100);
    child(w!("BUTTON"), "Cancel", BS_PUSHBUTTON as u32, WINDOW_EX_STYLE(0), (W - 14 - 88 - 8 - 88, 52, 88, 28), ID_CANCEL);
    child(w!("BUTTON"), "Add", BS_DEFPUSHBUTTON as u32, WINDOW_EX_STYLE(0), (W - 14 - 88, 52, 88, 28), ID_ADD);
    unsafe {
        SendMessageW(edit, EM_LIMITTEXT, Some(WPARAM(200)), None);
        let cue = HSTRING::from("What needs doing?");
        SendMessageW(edit, EM_SETCUEBANNER, Some(WPARAM(1)), Some(LPARAM(cue.as_ptr() as isize)));
    }
    PROMPT.with(|p| *p.borrow_mut() = Some(Prompt { edit, font, bus }));
    PROMPT_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(edit));
    }
    Ok(())
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            match wp.0 & 0xFFFF {
                ID_ADD => {
                    PROMPT.with(|p| {
                        if let Some(p) = p.borrow().as_ref() {
                            let mut buf = [0u16; 256];
                            let n = unsafe { GetWindowTextW(p.edit, &mut buf) } as usize;
                            let text = from_wide(&buf[..n.min(buf.len())]);
                            if !text.trim().is_empty() {
                                p.bus.send(Event::TodoAdded(text));
                            }
                        }
                    });
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                }
                ID_CANCEL => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                _ => {}
            }
            LRESULT(0)
        }
        WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            // Clicked elsewhere: the box goes away like a menu would.
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PROMPT_HWND.store(0, Ordering::SeqCst);
            if let Some(p) = PROMPT.with(|p| p.borrow_mut().take()) {
                unsafe {
                    let _ = DeleteObject(p.font.into());
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
