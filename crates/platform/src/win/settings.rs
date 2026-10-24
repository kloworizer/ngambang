//! The Settings window: a plain native window with standard controls, so it
//! is keyboard- and screen-reader-friendly and costs almost nothing while
//! closed. Saving rewrites only the changed lines of config.toml.

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};

use ngambang_core::config::{Config, ModuleId, DEFAULT_TOML};
use ngambang_core::geometry::Edge;
use ngambang_core::settings::{self, Form};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateFontIndirectW, DeleteObject, GetSysColorBrush, COLOR_BTNFACE, HFONT, LOGFONTW};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::system;
use super::util::{from_wide, WM_APP_SETTINGS};

/// The open Settings window, for `IsDialogMessageW` in the message loop.
pub static SETTINGS_HWND: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    static WINDOW: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

const ID_SAVE: usize = 1; // IDOK: Enter
const ID_CANCEL: usize = 2; // IDCANCEL: Esc
const ID_OPEN_FILE: usize = 3;

// Client size and column geometry, in DIPs.
const W: i32 = 640;
const H: i32 = 548;
const COL: i32 = 296;
const LABEL: i32 = 140;

struct Controls {
    autostart: HWND,
    hide_in_fullscreen: HWND,
    reduce_motion: HWND,
    hide_from_capture: HWND,
    hotkey: HWND,
    edge: HWND,
    monitor: HWND,
    offset: HWND,
    track_tags: HWND,
    volume_step: HWND,
    clipboard_tags: HWND,
    modules: Vec<(ModuleId, HWND)>,
    long_command: HWND,
    presets: HWND,
    break_after: HWND,
    ping_host: HWND,
    power_plan: HWND,
}

struct Settings {
    hwnd: HWND,
    owner: HWND,
    font: HFONT,
    c: Controls,
    monitors: Vec<String>,
    plans: Vec<(String, String)>,
}

/// Open the window, or bring it forward if it is already open.
pub fn open(owner: HWND, cfg: &Config) {
    let existing = SETTINGS_HWND.load(Ordering::SeqCst);
    if existing != 0 {
        unsafe {
            let _ = SetForegroundWindow(HWND(existing as *mut _));
        }
        return;
    }
    if let Err(e) = create(owner, cfg) {
        log_warn!("settings: {e}");
    }
}

struct Builder {
    parent: HWND,
    font: HFONT,
    scale: f32,
}

impl Builder {
    fn s(&self, v: i32) -> i32 {
        (v as f32 * self.scale).round() as i32
    }

    fn make(&self, class: PCWSTR, text: &str, style: u32, ex: WINDOW_EX_STYLE, r: (i32, i32, i32, i32), id: usize) -> HWND {
        unsafe {
            let h = CreateWindowExW(
                ex,
                class,
                &HSTRING::from(text),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
                self.s(r.0),
                self.s(r.1),
                self.s(r.2),
                self.s(r.3),
                Some(self.parent),
                Some(HMENU(id as *mut _)),
                GetModuleHandleW(None).ok().map(|m| m.into()),
                None,
            )
            .unwrap_or_default();
            SendMessageW(h, WM_SETFONT, Some(WPARAM(self.font.0 as usize)), Some(LPARAM(1)));
            h
        }
    }

    fn group(&self, text: &str, x: i32, y: i32, w: i32, h: i32) {
        self.make(w!("BUTTON"), text, BS_GROUPBOX as u32, WINDOW_EX_STYLE(0), (x, y, w, h), 0);
    }

    fn check(&self, text: &str, x: i32, y: i32, on: bool) -> HWND {
        let h = self.make(w!("BUTTON"), text, (BS_AUTOCHECKBOX as u32) | WS_TABSTOP.0, WINDOW_EX_STYLE(0), (x, y, COL - 24, 24), 0);
        unsafe { SendMessageW(h, BM_SETCHECK, Some(WPARAM(on as usize)), None) };
        h
    }

    fn label(&self, text: &str, x: i32, y: i32) {
        self.make(w!("STATIC"), text, 0, WINDOW_EX_STYLE(0), (x, y + 4, LABEL, 20), 0);
    }

    fn edit(&self, text: &str, x: i32, y: i32) -> HWND {
        self.make(
            w!("EDIT"),
            text,
            (ES_AUTOHSCROLL as u32) | WS_TABSTOP.0 | WS_BORDER.0,
            WINDOW_EX_STYLE(0),
            (x + LABEL, y, COL - 24 - LABEL, 24),
            0,
        )
    }

    fn combo(&self, items: &[String], selected: usize, x: i32, y: i32) -> HWND {
        let h = self.make(
            w!("COMBOBOX"),
            "",
            (CBS_DROPDOWNLIST as u32) | WS_TABSTOP.0 | WS_VSCROLL.0,
            WINDOW_EX_STYLE(0),
            (x + LABEL, y, COL - 24 - LABEL, 220),
            0,
        );
        unsafe {
            for item in items {
                let w = HSTRING::from(item.as_str());
                SendMessageW(h, CB_ADDSTRING, None, Some(LPARAM(w.as_ptr() as isize)));
            }
            SendMessageW(h, CB_SETCURSEL, Some(WPARAM(selected)), None);
        }
        h
    }

    fn button(&self, text: &str, x: i32, y: i32, w: i32, id: usize, default: bool) {
        let style = if default { BS_DEFPUSHBUTTON } else { BS_PUSHBUTTON } as u32 | WS_TABSTOP.0;
        self.make(w!("BUTTON"), text, style, WINDOW_EX_STYLE(0), (x, y, w, 32), id);
    }
}

fn create(owner: HWND, cfg: &Config) -> windows::core::Result<()> {
    let hinstance = unsafe { GetModuleHandleW(None)? };
    let class = w!("NgambangSettings");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: hinstance.into(),
        lpszClassName: class,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        hbrBackground: unsafe { GetSysColorBrush(COLOR_BTNFACE) },
        ..Default::default()
    };
    unsafe { RegisterClassExW(&wc) };

    let dpi = unsafe { GetDpiForWindow(owner) }.max(96);
    let scale = dpi as f32 / 96.0;
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let mut r = windows::Win32::Foundation::RECT { left: 0, top: 0, right: (W as f32 * scale) as i32, bottom: (H as f32 * scale) as i32 };
    unsafe {
        let _ = windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(&mut r, style, false, WS_EX_CONTROLPARENT, dpi);
    }
    let (ww, wh) = (r.right - r.left, r.bottom - r.top);
    let (sx, sy) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class,
            w!("Ngambang settings"),
            style,
            (sx - ww) / 2,
            (sy - wh) / 2,
            ww,
            wh,
            None,
            None,
            Some(hinstance.into()),
            None,
        )?
    };

    let mut lf = LOGFONTW { lfHeight: -((9.0 * dpi as f32 / 72.0).round() as i32), lfWeight: 400, ..Default::default() };
    let face: Vec<u16> = "Segoe UI".encode_utf16().collect();
    lf.lfFaceName[..face.len()].copy_from_slice(&face);
    let font = unsafe { CreateFontIndirectW(&lf) };
    let b = Builder { parent: hwnd, font, scale };

    let form = Form::from_config(cfg, system::autostart_enabled());
    let (lx, rx) = (16, 16 + COL + 16);
    let ix = |x: i32| x + 12;

    // General
    b.group("General", lx, 12, COL, 172);
    let autostart = b.check("Start with Windows", ix(lx), 36, form.autostart);
    let hide_in_fullscreen = b.check("Sink to a thin line in fullscreen apps", ix(lx), 62, form.hide_in_fullscreen);
    let reduce_motion = b.check("Reduce motion", ix(lx), 88, form.reduce_motion);
    let hide_from_capture = b.check("Hide from screenshots and sharing", ix(lx), 114, form.hide_from_capture);
    b.label("Hotkey", ix(lx), 144);
    let hotkey = b.edit(&form.hotkey, ix(lx), 144);

    // Position
    b.group("Position", lx, 196, COL, 128);
    b.label("Screen edge", ix(lx), 220);
    let edges: Vec<String> = ["Left", "Right", "Top", "Bottom"].map(String::from).to_vec();
    let edge = b.combo(&edges, Edge::ALL.iter().position(|e| *e == form.edge).unwrap_or(0), ix(lx), 220);
    b.label("Monitor", ix(lx), 252);
    let mut monitors = vec!["primary".to_string()];
    monitors.extend(system::secondary_monitors());
    if !monitors.iter().any(|m| m.eq_ignore_ascii_case(&form.monitor)) {
        monitors.push(form.monitor.clone());
    }
    let labels: Vec<String> = monitors
        .iter()
        .map(|m| if m == "primary" { "Primary display".to_string() } else { m.trim_start_matches(r"\\.\").to_string() })
        .collect();
    let monitor = b.combo(&labels, monitors.iter().position(|m| m.eq_ignore_ascii_case(&form.monitor)).unwrap_or(0), ix(lx), 252);
    b.label("Along the edge (%)", ix(lx), 284);
    let offset = b.edit(&form.offset_pct, ix(lx), 284);

    // Media & clipboard
    b.group("Media && clipboard", lx, 336, COL, 128);
    let track_tags = b.check("Show a tag when a new track starts", ix(lx), 360, form.track_tags);
    b.label("Volume step (%)", ix(lx), 390);
    let volume_step = b.edit(&form.volume_step, ix(lx), 390);
    let clipboard_tags = b.check("Show a tag for recognised clipboard text", ix(lx), 424, form.clipboard_tags);

    // Modules
    b.group("Modules", rx, 12, COL, 266);
    let modules =
        ModuleId::ALL.iter().enumerate().map(|(i, m)| (*m, b.check(m.label(), ix(rx), 36 + i as i32 * 26, form.modules.get(*m)))).collect();

    // Commands, timer & games
    b.group("Commands, timer && games", rx, 290, COL, 192);
    b.label("Long command (s)", ix(rx), 314);
    let long_command = b.edit(&form.long_command_s, ix(rx), 314);
    b.label("Timer presets (min)", ix(rx), 346);
    let presets = b.edit(&form.timer_presets, ix(rx), 346);
    b.label("Break reminder (min)", ix(rx), 378);
    let break_after = b.edit(&form.break_after_min, ix(rx), 378);
    b.label("Ping host", ix(rx), 410);
    let ping_host = b.edit(&form.ping_host, ix(rx), 410);
    b.label("Power plan in games", ix(rx), 442);
    let plans = system::power_plans();
    let mut plan_items = vec!["Don't switch".to_string()];
    plan_items.extend(plans.iter().map(|(_, n)| n.clone()));
    let plan_index = if form.power_plan.is_empty() {
        0
    } else {
        let want = ngambang_core::format::parse_guid(&form.power_plan);
        plans.iter().position(|(g, _)| ngambang_core::format::parse_guid(g) == want).map(|i| i + 1).unwrap_or(0)
    };
    let power_plan = b.combo(&plan_items, plan_index, ix(rx), 442);

    // Buttons
    b.button("Open config file", lx, 500, 140, ID_OPEN_FILE, false);
    b.button("Cancel", W - 16 - 96 - 8 - 96, 500, 96, ID_CANCEL, false);
    b.button("Save", W - 16 - 96, 500, 96, ID_SAVE, true);

    let c = Controls {
        autostart,
        hide_in_fullscreen,
        reduce_motion,
        hide_from_capture,
        hotkey,
        edge,
        monitor,
        offset,
        track_tags,
        volume_step,
        clipboard_tags,
        modules,
        long_command,
        presets,
        break_after,
        ping_host,
        power_plan,
    };
    WINDOW.with(|w| *w.borrow_mut() = Some(Settings { hwnd, owner, font, c, monitors, plans }));
    SETTINGS_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(autostart));
    }
    Ok(())
}

fn checked(h: HWND) -> bool {
    unsafe { SendMessageW(h, BM_GETCHECK, None, None).0 == 1 }
}

fn text(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
    from_wide(&buf[..n.min(buf.len())])
}

fn selected(h: HWND) -> usize {
    unsafe { SendMessageW(h, CB_GETCURSEL, None, None).0.max(0) as usize }
}

impl Settings {
    fn form(&self) -> Form {
        let c = &self.c;
        let mut modules = ngambang_core::config::Modules::default();
        for (m, h) in &c.modules {
            modules.set(*m, checked(*h));
        }
        let plan = selected(c.power_plan);
        Form {
            autostart: checked(c.autostart),
            edge: Edge::ALL[selected(c.edge).min(3)],
            offset_pct: text(c.offset),
            monitor: self.monitors.get(selected(c.monitor)).cloned().unwrap_or_else(|| "primary".into()),
            hotkey: text(c.hotkey),
            hide_in_fullscreen: checked(c.hide_in_fullscreen),
            reduce_motion: checked(c.reduce_motion),
            hide_from_capture: checked(c.hide_from_capture),
            modules,
            track_tags: checked(c.track_tags),
            volume_step: text(c.volume_step),
            long_command_s: text(c.long_command),
            clipboard_tags: checked(c.clipboard_tags),
            timer_presets: text(c.presets),
            break_after_min: text(c.break_after),
            ping_host: text(c.ping_host),
            power_plan: if plan == 0 { String::new() } else { self.plans.get(plan - 1).map(|p| p.0.clone()).unwrap_or_default() },
        }
    }

    /// Validate and write. Returns false (and explains) when something is wrong.
    fn save(&self) -> bool {
        let form = self.form();
        let edits = match form.edits() {
            Ok(e) => e,
            Err(msg) => {
                unsafe { MessageBoxW(Some(self.hwnd), &HSTRING::from(msg), w!("Ngambang settings"), MB_ICONWARNING) };
                return false;
            }
        };
        let path = crate::paths::config_file();
        let current = std::fs::read_to_string(&path).unwrap_or_else(|_| DEFAULT_TOML.to_string());
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::write(&path, settings::apply(&current, &edits)) {
            unsafe {
                MessageBoxW(
                    Some(self.hwnd),
                    &HSTRING::from(format!("Could not save {}:\n{e}", path.display())),
                    w!("Ngambang settings"),
                    MB_ICONERROR,
                )
            };
            return false;
        }
        if form.autostart != system::autostart_enabled() {
            system::set_autostart(form.autostart);
        }
        unsafe {
            let _ = PostMessageW(Some(self.owner), WM_APP_SETTINGS, WPARAM(0), LPARAM(0));
        }
        true
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = wp.0 & 0xFFFF;
            match id {
                ID_SAVE => {
                    let saved = WINDOW.with(|w| w.try_borrow().ok().and_then(|g| g.as_ref().map(|s| s.save())).unwrap_or(false));
                    if saved {
                        unsafe {
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                }
                ID_CANCEL => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                ID_OPEN_FILE => {
                    let path = crate::paths::config_file();
                    if !path.exists() {
                        let _ = std::fs::write(&path, DEFAULT_TOML);
                    }
                    super::app::open_text_file(&path);
                }
                _ => {}
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
            SETTINGS_HWND.store(0, Ordering::SeqCst);
            if let Some(s) = WINDOW.with(|w| w.try_borrow_mut().ok().and_then(|mut g| g.take())) {
                unsafe {
                    let _ = DeleteObject(s.font.into());
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
