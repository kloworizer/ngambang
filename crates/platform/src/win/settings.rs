//! The Settings window: a sidebar of pages with icons, and standard controls
//! on each page so it stays keyboard- and screen-reader-friendly. Nothing runs
//! while it is closed. Saving rewrites only the changed lines of config.toml.

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};

use ngambang_core::config::{Config, ModuleId, DEFAULT_TOML};
use ngambang_core::geometry::{Color, Edge, Rect};
use ngambang_core::icons::{arc_points, icon_prims};
use ngambang_core::model::Icon;
use ngambang_core::settings::{self, Form};
use ngambang_core::theme;
use ngambang_core::view::{Align, Prim, TextStyle, Weight};
use windows::core::{w, Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetKeyState, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_CONTROL, VK_NEXT, VK_PRIOR, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::gfx::Painter;
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
const ID_CITY: usize = 10;
const ID_LOCATE: usize = 11;
const ID_CHECK_UPDATE: usize = 12;
/// Posted to the Settings window when "Use my location" finishes.
const WM_LOCATED: u32 = WM_APP + 20;
const WM_MOUSELEAVE: u32 = 0x02A3;

/// The result of the last location request.
static LOCATED: std::sync::Mutex<Option<Result<(f64, f64), String>>> = std::sync::Mutex::new(None);

// Window geometry, in DIPs.
const W: i32 = 820;
const H: i32 = 588;
const SIDE: i32 = 212;
const PAD: i32 = 32;
const X0: i32 = SIDE + PAD;
const CW: i32 = W - X0 - PAD;
const LABEL: i32 = 220;
const CTRL_X: i32 = X0 + LABEL;
const CTRL_W: i32 = CW - LABEL;
const NAV_TOP: i32 = 96;
const NAV_H: i32 = 40;
const NAV_GAP: i32 = 4;

// Content colours (the content area follows the system window colour).
const INK: Color = Color::rgb(0x1B1D20);
const INK_2: Color = Color::rgb(0x5F6670);
const RULE: Color = Color::rgb(0xE4E6EA);

struct Page {
    icon: Icon,
    title: &'static str,
    subtitle: &'static str,
    accent: Color,
}

const PAGES: [Page; 7] = [
    Page { icon: Icon::Gear, title: "General", subtitle: "Startup, position and how the island behaves", accent: theme::MEDIA },
    Page { icon: Icon::Grid, title: "Modules", subtitle: "Turn on what you use; modules that are off cost nothing", accent: theme::CODER },
    Page { icon: Icon::Note, title: "Media & clipboard", subtitle: "Now playing, volume and developer clipboard", accent: theme::MEDIA },
    Page { icon: Icon::Clock, title: "Focus & commands", subtitle: "The focus timer and long-running command tags", accent: theme::GAME },
    Page { icon: Icon::Game, title: "Games", subtitle: "Break reminders, ping and power plan while playing", accent: theme::GAME },
    Page { icon: Icon::Pin, title: "Location & prayer", subtitle: "Your place for jadwal sholat and the weather", accent: theme::OK },
    Page { icon: Icon::Cloud, title: "Weather & updates", subtitle: "Early warnings near you and new versions", accent: theme::PORTS },
];

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
    custom: HWND,
    break_after: HWND,
    ping_host: HWND,
    power_plan: HWND,
    city: HWND,
    lat: HWND,
    lon: HWND,
    locate: HWND,
    method: HWND,
    asr: HWND,
    reminder: HWND,
    weather_alerts: HWND,
    quake_radius: HWND,
    update_check: HWND,
}

struct Settings {
    hwnd: HWND,
    owner: HWND,
    font: HFONT,
    c: Controls,
    monitors: Vec<String>,
    plans: Vec<(String, String)>,
    /// City name kept when "Custom" is selected.
    custom_city: String,
    page: usize,
    hover: Option<usize>,
    tracking: bool,
    /// Controls on each page, shown only while that page is open.
    pages: Vec<Vec<HWND>>,
    /// Section titles drawn on each page: (page, y, text).
    sections: Vec<(usize, i32, &'static str)>,
    /// Grey hint texts.
    hints: Vec<HWND>,
    scale: f32,
    painter: Option<Painter>,
    target: Option<ID2D1DCRenderTarget>,
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

/// Ctrl+Tab and Ctrl+PageUp/PageDown switch pages. Called by the message loop
/// before dialog navigation; true when the key was used.
pub fn pre_translate(msg: &MSG) -> bool {
    let h = SETTINGS_HWND.load(Ordering::SeqCst);
    if h == 0 || msg.message != WM_KEYDOWN {
        return false;
    }
    let settings = HWND(h as *mut _);
    if msg.hwnd != settings && !unsafe { IsChild(settings, msg.hwnd) }.as_bool() {
        return false;
    }
    let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    let key = msg.wParam.0 as u16;
    let step = match key {
        k if k == VK_TAB.0 && ctrl => {
            if shift {
                -1
            } else {
                1
            }
        }
        k if k == VK_NEXT.0 && ctrl => 1,
        k if k == VK_PRIOR.0 && ctrl => -1,
        _ => return false,
    };
    WINDOW.with(|w| {
        if let Ok(mut g) = w.try_borrow_mut() {
            if let Some(s) = g.as_mut() {
                let n = PAGES.len() as i32;
                s.show_page(((s.page as i32 + step + n) % n) as usize);
            }
        }
    });
    true
}

struct Builder {
    parent: HWND,
    font: HFONT,
    scale: f32,
    page: usize,
    pages: Vec<Vec<HWND>>,
    sections: Vec<(usize, i32, &'static str)>,
    hints: Vec<HWND>,
}

impl Builder {
    fn s(&self, v: i32) -> i32 {
        (v as f32 * self.scale).round() as i32
    }

    fn make(&mut self, class: PCWSTR, text: &str, style: u32, ex: WINDOW_EX_STYLE, r: (i32, i32, i32, i32), id: usize) -> HWND {
        let visible = if self.page == 0 { WS_VISIBLE } else { WINDOW_STYLE(0) };
        let h = unsafe {
            let h = CreateWindowExW(
                ex,
                class,
                &HSTRING::from(text),
                WS_CHILD | visible | WINDOW_STYLE(style),
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
        };
        while self.pages.len() <= self.page {
            self.pages.push(Vec::new());
        }
        self.pages[self.page].push(h);
        h
    }

    fn section(&mut self, y: i32, text: &'static str) {
        self.sections.push((self.page, y, text));
    }

    fn check_at(&mut self, text: &str, x: i32, y: i32, w: i32, on: bool) -> HWND {
        let h = self.make(w!("BUTTON"), text, (BS_AUTOCHECKBOX as u32) | WS_TABSTOP.0, WINDOW_EX_STYLE(0), (x, y, w, 26), 0);
        unsafe { SendMessageW(h, BM_SETCHECK, Some(WPARAM(on as usize)), None) };
        h
    }

    fn check(&mut self, text: &str, y: i32, on: bool) -> HWND {
        self.check_at(text, X0, y, CW, on)
    }

    fn label(&mut self, text: &str, y: i32) {
        self.make(w!("STATIC"), text, 0, WINDOW_EX_STYLE(0), (X0, y + 5, LABEL - 12, 20), 0);
    }

    fn hint(&mut self, text: &str, y: i32) {
        let h = self.make(w!("STATIC"), text, 0, WINDOW_EX_STYLE(0), (X0, y, CW, 36), 0);
        self.hints.push(h);
    }

    fn edit(&mut self, label: &str, text: &str, y: i32) -> HWND {
        self.label(label, y);
        self.make(w!("EDIT"), text, (ES_AUTOHSCROLL as u32) | WS_TABSTOP.0, WS_EX_CLIENTEDGE, (CTRL_X, y, CTRL_W, 28), 0)
    }

    fn combo(&mut self, label: &str, items: &[String], selected: usize, y: i32, id: usize) -> HWND {
        self.label(label, y);
        let h = self.make(
            w!("COMBOBOX"),
            "",
            (CBS_DROPDOWNLIST as u32) | WS_TABSTOP.0 | WS_VSCROLL.0,
            WINDOW_EX_STYLE(0),
            (CTRL_X, y, CTRL_W, 320),
            id,
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

    fn button(&mut self, text: &str, x: i32, y: i32, w: i32, id: usize, default: bool) -> HWND {
        let style = if default { BS_DEFPUSHBUTTON } else { BS_PUSHBUTTON } as u32 | WS_TABSTOP.0;
        self.make(w!("BUTTON"), text, style, WINDOW_EX_STYLE(0), (x, y, w, 32), id)
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
        hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
        hIcon: super::app::app_icon(32),
        hIconSm: super::app::app_icon(16),
        ..Default::default()
    };
    unsafe { RegisterClassExW(&wc) };

    let dpi = unsafe { GetDpiForWindow(owner) }.max(96);
    let scale = dpi as f32 / 96.0;
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
    let mut r = RECT { left: 0, top: 0, right: (W as f32 * scale) as i32, bottom: (H as f32 * scale) as i32 };
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

    let mut lf = LOGFONTW { lfHeight: -((9.5 * dpi as f32 / 72.0).round() as i32), lfWeight: 400, ..Default::default() };
    let face: Vec<u16> = "Segoe UI".encode_utf16().collect();
    lf.lfFaceName[..face.len()].copy_from_slice(&face);
    let font = unsafe { CreateFontIndirectW(&lf) };
    let mut b = Builder { parent: hwnd, font, scale, page: 0, pages: Vec::new(), sections: Vec::new(), hints: Vec::new() };
    let form = Form::from_config(cfg, system::autostart_enabled());

    // General
    b.page = 0;
    b.section(112, "STARTUP");
    let autostart = b.check("Start with Windows", 134, form.autostart);
    let reduce_motion = b.check("Reduce motion (no morphs or pop-ups)", 162, form.reduce_motion);
    let hide_from_capture = b.check("Hide the island from screenshots and screen sharing", 190, form.hide_from_capture);
    let hide_in_fullscreen = b.check("Sink to a thin line while a fullscreen app has focus", 218, form.hide_in_fullscreen);
    let hotkey = b.edit("Open with hotkey", &form.hotkey, 254);
    b.section(304, "POSITION");
    let edges: Vec<String> = ["Left", "Right", "Top", "Bottom"].map(String::from).to_vec();
    let edge = b.combo("Screen edge", &edges, Edge::ALL.iter().position(|e| *e == form.edge).unwrap_or(0), 326, 0);
    let mut monitors = vec!["primary".to_string()];
    monitors.extend(system::secondary_monitors());
    if !monitors.iter().any(|m| m.eq_ignore_ascii_case(&form.monitor)) {
        monitors.push(form.monitor.clone());
    }
    let labels: Vec<String> = monitors
        .iter()
        .map(|m| if m == "primary" { "Primary display".to_string() } else { m.trim_start_matches(r"\\.\").to_string() })
        .collect();
    let monitor = b.combo("Monitor", &labels, monitors.iter().position(|m| m.eq_ignore_ascii_case(&form.monitor)).unwrap_or(0), 364, 0);
    let offset = b.edit("Along the edge (%)", &form.offset_pct, 402);
    b.hint("Tip: drag the orb, a tag or an open slip to move the island to any edge.", 444);

    // Modules
    b.page = 1;
    b.section(112, "SHOW ON THE ISLAND");
    let half = CW / 2;
    let modules = ModuleId::ALL
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let (col, row) = ((i % 2) as i32, (i / 2) as i32);
            (*m, b.check_at(m.label(), X0 + col * half, 136 + row * 34, half - 8, form.modules.get(*m)))
        })
        .collect();
    b.hint("The tray menu's Modules list changes the same switches.", 136 + 6 * 34 + 16);

    // Media & clipboard
    b.page = 2;
    b.section(112, "MEDIA");
    let track_tags = b.check("Pop up the song, cover and controls when a track starts", 134, form.track_tags);
    let volume_step = b.edit("Volume step (%)", &form.volume_step, 170);
    b.section(226, "CLIPBOARD");
    let clipboard_tags = b.check("Show a tag for JWT, JSON, UUIDs, timestamps, colours and links", 248, form.clipboard_tags);

    // Focus & commands
    b.page = 3;
    b.section(112, "FOCUS TIMER");
    let presets = b.edit("Preset rounds (minutes)", &form.timer_presets, 134);
    let custom = b.edit("Custom round (minutes)", &form.timer_custom, 172);
    b.hint("Presets are buttons in the timer slip; − and + there change the custom round.", 208);
    b.section(256, "COMMANDS");
    let long_command = b.edit("Long command (seconds)", &form.long_command_s, 278);
    b.hint("Used by the shell hook: run `ngambang hook powershell` to set it up.", 314);

    // Games
    b.page = 4;
    b.section(112, "WHILE PLAYING");
    let break_after = b.edit("Break reminder (minutes)", &form.break_after_min, 134);
    let ping_host = b.edit("Ping host", &form.ping_host, 172);
    let plans = system::power_plans();
    let mut plan_items = vec!["Don't switch".to_string()];
    plan_items.extend(plans.iter().map(|(_, n)| n.clone()));
    let plan_index = if form.power_plan.is_empty() {
        0
    } else {
        let want = ngambang_core::format::parse_guid(&form.power_plan);
        plans.iter().position(|(g, _)| ngambang_core::format::parse_guid(g) == want).map(|i| i + 1).unwrap_or(0)
    };
    let power_plan = b.combo("Power plan in games", &plan_items, plan_index, 210, 0);
    b.hint("Only real games count: a game-library folder, exclusive fullscreen or a busy GPU. 0 turns the reminder off.", 252);

    // Location & prayer
    b.page = 5;
    b.section(112, "LOCATION");
    let cities = ngambang_core::prayer::CITIES;
    let mut city_items = vec!["Custom (coordinates below)".to_string()];
    city_items.extend(cities.iter().map(|c| format!("{} — {}", c.0, c.1)));
    let city_index = cities.iter().position(|c| c.0.eq_ignore_ascii_case(&form.prayer_city)).map(|i| i + 1).unwrap_or(0);
    let city = b.combo("City", &city_items, city_index, 134, ID_CITY);
    let lat = b.edit("Latitude", &form.prayer_lat, 172);
    let lon = b.edit("Longitude", &form.prayer_lon, 210);
    let locate = b.button("Use my location", CTRL_X, 248, CTRL_W, ID_LOCATE, false);
    b.section(304, "JADWAL SHOLAT");
    let methods: Vec<String> = ngambang_core::prayer::Method::ALL.iter().map(|m| m.label().to_string()).collect();
    let method = b.combo(
        "Calculation method",
        &methods,
        ngambang_core::prayer::Method::ALL.iter().position(|m| *m == form.prayer_method).unwrap_or(0),
        326,
        0,
    );
    let asr_items = vec!["Standard (Shafi'i, Maliki, Hanbali)".to_string(), "Hanafi".to_string()];
    let asr = b.combo("Asr", &asr_items, (form.prayer_asr == ngambang_core::prayer::Asr::Hanafi) as usize, 364, 0);
    let reminder = b.edit("Reminder (minutes before)", &form.prayer_reminder, 402);
    b.hint("Turn on Jadwal sholat on the Modules page to see it on the island.", 442);

    // Weather & updates
    b.page = 6;
    b.section(112, "WEATHER & EARLY WARNINGS");
    let weather_alerts = b.check("Pop up disasters near my location", 134, form.weather_alerts);
    let quake_radius = b.edit("Earthquake radius (km)", &form.quake_radius_km, 170);
    b.hint("Weather by Open-Meteo; warnings from GDACS and USGS. Nothing is fetched while offline.", 206);
    b.section(256, "UPDATES");
    let update_check = b.check("Check GitHub for new versions once a day", 278, form.update_check);
    b.button("Check for updates now", X0, 314, 200, ID_CHECK_UPDATE, false);

    // Footer, on every page.
    b.page = 0;
    let footer: Vec<HWND> = vec![
        b.button("Open config file", X0, H - 48, 140, ID_OPEN_FILE, false),
        b.button("Cancel", W - PAD - 96 - 8 - 96, H - 48, 96, ID_CANCEL, false),
        b.button("Save", W - PAD - 96, H - 48, 96, ID_SAVE, true),
    ];
    b.pages[0].retain(|h| !footer.contains(h));

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
        custom,
        break_after,
        ping_host,
        power_plan,
        city,
        lat,
        lon,
        locate,
        method,
        asr,
        reminder,
        weather_alerts,
        quake_radius,
        update_check,
    };
    let painter = unsafe {
        D2D1CreateFactory::<ID2D1Factory1>(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&D2D1_FACTORY_OPTIONS::default()))
            .ok()
            .and_then(|f| Painter::new(f).ok())
    };
    WINDOW.with(|w| {
        *w.borrow_mut() = Some(Settings {
            hwnd,
            owner,
            font,
            c,
            monitors,
            plans,
            custom_city: if city_index == 0 { form.prayer_city.clone() } else { "My location".into() },
            page: 0,
            hover: None,
            tracking: false,
            pages: b.pages,
            sections: b.sections,
            hints: b.hints,
            scale,
            painter,
            target: None,
        })
    });
    SETTINGS_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(autostart));
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

fn window_color() -> Color {
    let c = unsafe { GetSysColor(COLOR_WINDOW) };
    Color::rgb(((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF))
}

/// The page under a point in the sidebar (DIPs).
fn nav_at(x: f32, y: f32) -> Option<usize> {
    if x >= SIDE as f32 || y < NAV_TOP as f32 {
        return None;
    }
    let i = ((y - NAV_TOP as f32) / (NAV_H + NAV_GAP) as f32) as usize;
    let within = (y - NAV_TOP as f32) - (i as i32 * (NAV_H + NAV_GAP)) as f32 <= NAV_H as f32;
    (i < PAGES.len() && within).then_some(i)
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
            timer_custom: text(c.custom),
            break_after_min: text(c.break_after),
            ping_host: text(c.ping_host),
            power_plan: if plan == 0 { String::new() } else { self.plans.get(plan - 1).map(|p| p.0.clone()).unwrap_or_default() },
            prayer_city: match selected(c.city) {
                0 => self.custom_city.clone(),
                i => ngambang_core::prayer::CITIES.get(i - 1).map(|x| x.0.to_string()).unwrap_or_default(),
            },
            prayer_lat: text(c.lat),
            prayer_lon: text(c.lon),
            prayer_method: ngambang_core::prayer::Method::ALL[selected(c.method).min(ngambang_core::prayer::Method::ALL.len() - 1)],
            prayer_asr: if selected(c.asr) == 1 { ngambang_core::prayer::Asr::Hanafi } else { ngambang_core::prayer::Asr::Standard },
            prayer_reminder: text(c.reminder),
            update_check: checked(c.update_check),
            weather_alerts: checked(c.weather_alerts),
            quake_radius_km: text(c.quake_radius),
        }
    }

    /// Validate and write. The error is a sentence to show the user; the
    /// caller shows it once this window's state is no longer borrowed.
    fn save(&mut self) -> Result<(), String> {
        let form = self.form();
        let edits = match form.edits() {
            Ok(e) => e,
            Err(msg) => {
                // Open the page the problem is on.
                let page = match msg.split(':').next().unwrap_or("") {
                    "Position along the edge" | "Hotkey" => 0,
                    "Volume step" => 2,
                    "Long command" | "Timer presets" | "Custom round" => 3,
                    "Break reminder" | "Ping host" | "Power plan" => 4,
                    "Latitude" | "Longitude" | "Prayer reminder" => 5,
                    "Earthquake radius" => 6,
                    _ => self.page,
                };
                self.show_page(page);
                return Err(msg);
            }
        };
        let path = crate::paths::config_file();
        let current = std::fs::read_to_string(&path).unwrap_or_else(|_| DEFAULT_TOML.to_string());
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::write(&path, settings::apply(&current, &edits)) {
            return Err(format!("Could not save {}:\n{e}", path.display()));
        }
        if form.autostart != system::autostart_enabled() {
            system::set_autostart(form.autostart);
        }
        unsafe {
            let _ = PostMessageW(Some(self.owner), WM_APP_SETTINGS, WPARAM(0), LPARAM(0));
        }
        Ok(())
    }

    fn show_page(&mut self, page: usize) {
        if page == self.page || page >= PAGES.len() {
            return;
        }
        unsafe {
            for h in self.pages.get(self.page).into_iter().flatten() {
                let _ = ShowWindow(*h, SW_HIDE);
            }
            for h in self.pages.get(page).into_iter().flatten() {
                let _ = ShowWindow(*h, SW_SHOWNA);
            }
            self.page = page;
            // Focus the first control that takes focus on the new page.
            if let Some(first) = self.pages[page].iter().find(|h| (GetWindowLongPtrW(**h, GWL_STYLE) as u32 & WS_TABSTOP.0) != 0) {
                let _ = SetFocus(Some(*first));
            }
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn prims(&self) -> Vec<Prim> {
        let mut p = Vec::new();
        let bg = window_color();
        let text = |p: &mut Vec<Prim>, r: Rect, t: &str, style: TextStyle, color: Color| {
            p.push(Prim::Text { rect: r, text: t.into(), style, color, align: Align::Start, wrap: false })
        };
        // Content background and the sidebar.
        p.push(Prim::RoundRect { rect: Rect::new(0.0, 0.0, W as f32, H as f32), radius: 0.0, fill: Some(bg), stroke: None });
        p.push(Prim::RoundRect { rect: Rect::new(0.0, 0.0, SIDE as f32, H as f32), radius: 0.0, fill: Some(theme::SURFACE), stroke: None });

        // The orb as a logo.
        let (cx, cy) = (40.0, 44.0);
        p.push(Prim::Circle { cx, cy, r: 18.0, fill: Some(theme::SURFACE_3), stroke: Some((theme::HAIRLINE, 1.0)) });
        p.push(Prim::Circle { cx, cy, r: 12.0, fill: None, stroke: Some((theme::TRACK, 3.0)) });
        p.push(Prim::Polyline { points: arc_points(cx, cy, 12.0, -90.0, 130.0, 32), width: 3.0, color: theme::MEDIA, closed: false });
        p.push(Prim::Circle { cx, cy, r: 4.5, fill: Some(theme::MEDIA), stroke: None });
        text(&mut p, Rect::new(70.0, 24.0, 130.0, 24.0), "Ngambang", TextStyle::TITLE, theme::TEXT);
        text(
            &mut p,
            Rect::new(70.0, 46.0, 130.0, 18.0),
            &format!("version {}", ngambang_core::VERSION),
            TextStyle::MONO_SMALL,
            theme::LABEL,
        );

        for (i, page) in PAGES.iter().enumerate() {
            let r = Rect::new(12.0, (NAV_TOP + i as i32 * (NAV_H + NAV_GAP)) as f32, (SIDE - 24) as f32, NAV_H as f32);
            let active = i == self.page;
            if active {
                p.push(Prim::RoundRect { rect: r, radius: 10.0, fill: Some(theme::SURFACE_3), stroke: None });
                p.push(Prim::RoundRect {
                    rect: Rect::new(r.x, r.y + 10.0, 3.0, r.h - 20.0),
                    radius: 1.5,
                    fill: Some(page.accent),
                    stroke: None,
                });
            } else if self.hover == Some(i) {
                p.push(Prim::RoundRect { rect: r, radius: 10.0, fill: Some(theme::SURFACE_2), stroke: None });
            }
            let ic = if active { page.accent } else { theme::TEXT_2 };
            p.extend(icon_prims(page.icon, Rect::new(r.x + 14.0, r.y + 11.0, 18.0, 18.0), ic));
            let style = TextStyle { size: 13.5, weight: if active { Weight::Semibold } else { Weight::Regular }, mono: false };
            text(&mut p, Rect::new(r.x + 44.0, r.y, r.w - 48.0, r.h), page.title, style, if active { theme::TEXT } else { theme::TEXT_2 });
        }
        text(
            &mut p,
            Rect::new(24.0, (H - 40) as f32, (SIDE - 32) as f32, 18.0),
            "Ctrl+Tab: next page",
            TextStyle { size: 11.0, weight: Weight::Regular, mono: false },
            theme::LABEL,
        );

        // Page header: icon tile, title, subtitle.
        let page = &PAGES[self.page];
        let tile = Rect::new(X0 as f32, 28.0, 44.0, 44.0);
        p.push(Prim::RoundRect { rect: tile, radius: 12.0, fill: Some(theme::SURFACE), stroke: None });
        p.extend(icon_prims(page.icon, tile.inset(11.0), page.accent));
        text(
            &mut p,
            Rect::new(X0 as f32 + 58.0, 26.0, CW as f32 - 58.0, 28.0),
            page.title,
            TextStyle { size: 20.0, weight: Weight::Semibold, mono: false },
            INK,
        );
        text(
            &mut p,
            Rect::new(X0 as f32 + 58.0, 54.0, CW as f32 - 58.0, 20.0),
            page.subtitle,
            TextStyle { size: 13.0, weight: Weight::Regular, mono: false },
            INK_2,
        );
        p.push(Prim::RoundRect { rect: Rect::new(X0 as f32, 92.0, CW as f32, 1.0), radius: 0.0, fill: Some(RULE), stroke: None });
        for (_, y, t) in self.sections.iter().filter(|s| s.0 == self.page) {
            text(
                &mut p,
                Rect::new(X0 as f32, *y as f32 - 4.0, CW as f32, 18.0),
                t,
                TextStyle { size: 11.0, weight: Weight::Semibold, mono: false },
                INK_2,
            );
        }
        // Footer rule above the buttons.
        p.push(Prim::RoundRect {
            rect: Rect::new(X0 as f32, (H - 64) as f32, CW as f32, 1.0),
            radius: 0.0,
            fill: Some(RULE),
            stroke: None,
        });
        p
    }

    fn paint(&mut self) {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(self.hwnd, &mut ps);
            let mut rc = RECT::default();
            let _ = GetClientRect(self.hwnd, &mut rc);
            if self.target.is_none() {
                if let Some(painter) = &self.painter {
                    let props = D2D1_RENDER_TARGET_PROPERTIES {
                        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                        dpiX: 0.0,
                        dpiY: 0.0,
                        usage: D2D1_RENDER_TARGET_USAGE_NONE,
                        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
                    };
                    self.target = painter.d2d_factory.CreateDCRenderTarget(&props).ok();
                }
            }
            let mut ok = false;
            if let (Some(t), Some(painter)) = (&self.target, &self.painter) {
                if t.BindDC(hdc, &rc).is_ok() {
                    t.BeginDraw();
                    let dpi = 96.0 * self.scale;
                    t.SetDpi(dpi, dpi);
                    if let Ok(dc) = t.cast::<ID2D1DeviceContext>() {
                        let _ = painter.draw(&dc, &self.prims());
                    }
                    ok = t.EndDraw(None, None).is_ok();
                }
            }
            if !ok {
                // Device lost or no Direct2D: start over next time, plain background now.
                self.target = None;
                FillRect(hdc, &rc, GetSysColorBrush(COLOR_WINDOW));
            }
            let _ = EndPaint(self.hwnd, &ps);
        }
    }
}

fn with<R>(f: impl FnOnce(&mut Settings) -> R) -> Option<R> {
    WINDOW.with(|w| w.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            if with(|s| s.paint()).is_none() {
                return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_CTLCOLORSTATIC => {
            // Labels and checkboxes sit on the content background; hints are grey.
            let hdc = HDC(wp.0 as *mut _);
            let hint = with(|s| s.hints.contains(&HWND(lp.0 as *mut _))).unwrap_or(false);
            unsafe {
                SetBkMode(hdc, TRANSPARENT);
                let ink = if hint { 0x0070_665F } else { GetSysColor(COLOR_WINDOWTEXT) };
                SetTextColor(hdc, COLORREF(ink));
                LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize)
            }
        }
        WM_LBUTTONDOWN => {
            let (x, y) = ((lp.0 & 0xFFFF) as i16 as f32, ((lp.0 >> 16) & 0xFFFF) as i16 as f32);
            with(|s| {
                if let Some(i) = nav_at(x / s.scale, y / s.scale) {
                    s.show_page(i);
                }
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = ((lp.0 & 0xFFFF) as i16 as f32, ((lp.0 >> 16) & 0xFFFF) as i16 as f32);
            with(|s| {
                if !s.tracking {
                    s.tracking = true;
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: s.hwnd,
                        dwHoverTime: 0,
                    };
                    unsafe {
                        let _ = TrackMouseEvent(&mut t);
                    }
                }
                let hover = nav_at(x / s.scale, y / s.scale);
                if hover != s.hover {
                    s.hover = hover;
                    let r = RECT { left: 0, top: 0, right: (SIDE as f32 * s.scale) as i32, bottom: (H as f32 * s.scale) as i32 };
                    unsafe {
                        let _ = InvalidateRect(Some(s.hwnd), Some(&r), false);
                        SetCursor(LoadCursorW(None, if hover.is_some() { IDC_HAND } else { IDC_ARROW }).ok());
                    }
                }
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|s| {
                s.tracking = false;
                if s.hover.take().is_some() {
                    unsafe {
                        let _ = InvalidateRect(Some(s.hwnd), None, false);
                    }
                }
            });
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wp.0 & 0xFFFF;
            match id {
                ID_SAVE => match with(|s| s.save()) {
                    Some(Ok(())) => unsafe {
                        let _ = DestroyWindow(hwnd);
                    },
                    Some(Err(msg)) => unsafe {
                        MessageBoxW(Some(hwnd), &HSTRING::from(msg), w!("Ngambang settings"), MB_ICONWARNING);
                    },
                    None => {}
                },
                ID_CANCEL => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                ID_CITY if (wp.0 >> 16) as u32 == CBN_SELCHANGE => {
                    with(|s| {
                        if let Some(c) = selected(s.c.city).checked_sub(1).and_then(|i| ngambang_core::prayer::CITIES.get(i)) {
                            set_text(s.c.lat, &format!("{:.4}", c.2));
                            set_text(s.c.lon, &format!("{:.4}", c.3));
                        }
                    });
                }
                ID_LOCATE => {
                    with(|s| {
                        set_text(s.c.locate, "Locating…");
                        unsafe {
                            let _ = EnableWindow(s.c.locate, false);
                        }
                    });
                    let target = hwnd.0 as isize;
                    let _ = std::thread::Builder::new().name("ngambang-locate".into()).spawn(move || {
                        *LOCATED.lock().unwrap_or_else(|e| e.into_inner()) = Some(locate());
                        unsafe {
                            let _ = PostMessageW(Some(HWND(target as *mut _)), WM_LOCATED, WPARAM(0), LPARAM(0));
                        }
                    });
                }
                ID_CHECK_UPDATE => {
                    let owner = with(|s| s.owner.0 as isize).unwrap_or(0);
                    unsafe {
                        let _ = PostMessageW(Some(HWND(owner as *mut _)), WM_APP_SETTINGS, WPARAM(1), LPARAM(0));
                    }
                }
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
        WM_LOCATED => {
            let result = LOCATED.lock().unwrap_or_else(|e| e.into_inner()).take();
            with(|s| {
                set_text(s.c.locate, "Use my location");
                unsafe {
                    let _ = EnableWindow(s.c.locate, true);
                }
                if let Some(Ok((la, lo))) = result {
                    set_text(s.c.lat, &format!("{la:.4}"));
                    set_text(s.c.lon, &format!("{lo:.4}"));
                    unsafe { SendMessageW(s.c.city, CB_SETCURSEL, Some(WPARAM(0)), None) };
                    s.custom_city = "My location".into();
                }
            });
            if let Some(Err(msg)) = result {
                unsafe {
                    let r = MessageBoxW(
                        Some(hwnd),
                        &HSTRING::from(format!("{msg}\n\nOpen Windows location settings?")),
                        w!("Ngambang settings"),
                        MB_ICONINFORMATION | MB_YESNO,
                    );
                    if r == IDYES {
                        system::shell_open("ms-settings:privacy-location");
                    }
                }
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

fn set_text(h: HWND, t: &str) {
    unsafe {
        let _ = SetWindowTextW(h, &HSTRING::from(t));
    }
}

/// Ask Windows for the device location (needs "Let desktop apps access your location").
fn locate() -> Result<(f64, f64), String> {
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator};
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
    }
    let denied = || "Windows did not share your location. Turn on Location and \"Let desktop apps access your location\".".to_string();
    let access = Geolocator::RequestAccessAsync().and_then(|a| a.join()).map_err(|_| denied())?;
    if access != GeolocationAccessStatus::Allowed {
        return Err(denied());
    }
    let pos = Geolocator::new()
        .and_then(|g| g.GetGeopositionAsync())
        .and_then(|op| op.join())
        .and_then(|p| p.Coordinate())
        .and_then(|c| c.Point())
        .and_then(|p| p.Position())
        .map_err(|e| format!("Could not get your location: {}", e.message()))?;
    Ok((pos.Latitude, pos.Longitude))
}
