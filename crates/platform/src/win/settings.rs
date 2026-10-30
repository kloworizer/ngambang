//! The Settings window: a sidebar of pages with icons, and standard controls
//! on soft cards so it stays keyboard- and screen-reader-friendly. Nothing runs
//! while it is closed. Saving rewrites only the changed lines of config.toml.

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::Mutex;

use ngambang_core::config::{CalendarFeed, Config, ModuleId, Style, WatchItem, DEFAULT_TOML};
use ngambang_core::finance::SearchHit;
use ngambang_core::geometry::{Color, Edge, Rect};
use ngambang_core::icons::{arc_points, icon_prims};
use ngambang_core::model::Icon;
use ngambang_core::settings::{self, Form, CALENDAR_COLORS};
use ngambang_core::soft;
use ngambang_core::theme;
use ngambang_core::view::{Align, Prim, TextStyle, Weight};
use windows::core::{w, Interface, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_IGNORE, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::*;
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
const ID_STYLE: usize = 12;
const ID_LOCATE: usize = 11;
const ID_MODULE_UP: usize = 20;
const ID_MODULE_DOWN: usize = 21;
const ID_FEED_ADD: usize = 30;
const ID_FEED_REMOVE: usize = 31;
const ID_FEED_UPDATE: usize = 32;
const ID_SEARCH: usize = 40;
const ID_RESULTS: usize = 41;
const ID_WATCH_ADD: usize = 42;
const ID_WATCH: usize = 43;
const ID_WATCH_UP: usize = 44;
const ID_WATCH_DOWN: usize = 45;
const ID_WATCH_REMOVE: usize = 46;
const ID_WATCH_LEVELS: usize = 47;
const ID_REPO: usize = 50;
const ID_ISSUES: usize = 51;
/// Posted to the Settings window when "Use my location" finishes.
const WM_LOCATED: u32 = WM_APP + 20;
/// Posted when a symbol search finishes.
const WM_SEARCHED: u32 = WM_APP + 21;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_SEARCH: usize = 1;
const EN_CHANGE: u32 = 0x0300;
const LBN_DBLCLK: u32 = 2;
const EM_SETCUEBANNER: u32 = 0x1501;

/// The result of the last location request.
static LOCATED: Mutex<Option<Result<(f64, f64), String>>> = Mutex::new(None);
/// A finished symbol search: its request number and result.
type SearchResult = (u64, Result<Vec<SearchHit>, String>);
/// The latest symbol search.
static SEARCHED: Mutex<Option<SearchResult>> = Mutex::new(None);
static SEARCH_SEQ: AtomicU64 = AtomicU64::new(0);

// Window geometry, in DIPs.
const W: i32 = 840;
const H: i32 = 660;
const SIDE: i32 = 212;
const PAD: i32 = 40;
const X0: i32 = SIDE + PAD;
const CW: i32 = W - X0 - PAD;
const LABEL: i32 = 210;
const CTRL_X: i32 = X0 + LABEL;
const CTRL_W: i32 = CW - LABEL;
const NAV_TOP: i32 = 96;
const NAV_H: i32 = 40;
const NAV_GAP: i32 = 4;
const FOOTER: i32 = H - 64;

// The content area: a light, soft surface with raised cards.
const CONTENT: Color = Color::rgb(0xE6E9EE);
const CARD_DARK: Color = Color::rgb(0xA9B1BC);
const CARD_LIGHT: Color = Color::rgb(0xFFFFFF);
const INK: Color = Color::rgb(0x1B1D20);
const INK_2: Color = Color::rgb(0x59606A);

struct Page {
    icon: Icon,
    title: &'static str,
    subtitle: &'static str,
    accent: Color,
}

/// Who made Ngambang, and where it lives (shown on the About page).
const AUTHOR: &str = "Esha Indra";
const REPO: &str = env!("CARGO_PKG_REPOSITORY");

const PAGES: [Page; 11] = [
    Page { icon: Icon::Gear, title: "General", subtitle: "Startup, position and how the island behaves", accent: theme::MEDIA },
    Page { icon: Icon::Grid, title: "Modules", subtitle: "Turn modules on or off and set the order of their tabs", accent: theme::CODER },
    Page { icon: Icon::Note, title: "Media & clipboard", subtitle: "Now playing, volume and developer clipboard", accent: theme::MEDIA },
    Page { icon: Icon::Clock, title: "Focus & commands", subtitle: "The focus timer and long-running command tags", accent: theme::GAME },
    Page { icon: Icon::Game, title: "Games & power", subtitle: "Breaks while playing, battery and power plans", accent: theme::GAME },
    Page { icon: Icon::Pin, title: "Location & prayer", subtitle: "Your place for jadwal sholat and the weather", accent: theme::OK },
    Page { icon: Icon::Cloud, title: "Weather & updates", subtitle: "Early warnings near you and new versions", accent: theme::PORTS },
    Page {
        icon: Icon::Calendar,
        title: "Calendar",
        subtitle: "Link Google Calendar (or any iCal calendar), as many as you like",
        accent: theme::CODER,
    },
    Page {
        icon: Icon::Chart,
        title: "Markets",
        subtitle: "Search shares, indices, crypto and currencies for your watchlist",
        accent: theme::GAME,
    },
    Page {
        icon: Icon::Grid,
        title: "Notch & screen",
        subtitle: "What the notch turns through, fullscreen apps and screen sharing",
        accent: theme::PORTS,
    },
    Page { icon: Icon::Open, title: "About", subtitle: "Who made Ngambang and where to find it", accent: theme::MEDIA },
];

/// The Settings page that configures a module.
pub fn page_for(m: ModuleId) -> usize {
    match m {
        ModuleId::Media | ModuleId::Clipboard | ModuleId::Mixer => 2,
        ModuleId::Timer | ModuleId::Commands => 3,
        ModuleId::Game | ModuleId::Power => 4,
        ModuleId::Prayer => 5,
        ModuleId::Weather => 6,
        ModuleId::Calendar => 7,
        ModuleId::Finance => 8,
        _ => 1,
    }
}

struct Controls {
    autostart: HWND,
    hide_in_fullscreen: HWND,
    move_in_fullscreen: HWND,
    style: HWND,
    reduce_motion: HWND,
    hide_from_capture: HWND,
    hotkey: HWND,
    edge: HWND,
    monitor: HWND,
    offset: HWND,
    track_tags: HWND,
    volume_step: HWND,
    clipboard_tags: HWND,
    modules: HWND,
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
    rain_tags: HWND,
    notch_glances: HWND,
    notch_glance_s: HWND,
    hide_when_sharing: HWND,
    snap: HWND,
    fullscreen_ignore: HWND,
    fullscreen_sink: HWND,
    quake_radius: HWND,
    update_check: HWND,
    feeds: HWND,
    feed_name: HWND,
    feed_url: HWND,
    feed_color: HWND,
    cal_reminder: HWND,
    cal_days: HWND,
    search: HWND,
    search_status: HWND,
    results: HWND,
    watch: HWND,
    price_alerts: HWND,
    move_pct: HWND,
    news: HWND,
    above: HWND,
    below: HWND,
    hijri_adjust: HWND,
    break_min: HWND,
    long_break: HWND,
    long_every: HWND,
    focus_assist: HWND,
    battery_plan: HWND,
    plugged_plan: HWND,
    low_battery: HWND,
    network_tags: HWND,
}

struct Settings {
    hwnd: HWND,
    owner: HWND,
    font: HFONT,
    content_brush: HBRUSH,
    c: Controls,
    monitors: Vec<String>,
    plans: Vec<(String, String)>,
    /// City name kept when "Custom" is selected.
    custom_city: String,
    /// Modules in tab order, with their on/off state.
    order: Vec<(ModuleId, bool)>,
    feeds: Vec<CalendarFeed>,
    watchlist: Vec<WatchItem>,
    hits: Vec<SearchHit>,
    page: usize,
    hover: Option<usize>,
    tracking: bool,
    /// Controls on each page, shown only while that page is open.
    pages: Vec<Vec<HWND>>,
    /// Cards drawn behind each section: (page, top, bottom, title).
    cards: Vec<(usize, i32, i32, &'static str)>,
    /// Grey hint texts.
    hints: Vec<HWND>,
    scale: f32,
    painter: Option<Painter>,
    target: Option<ID2D1DCRenderTarget>,
}

/// Open the window at a page, or bring it forward if it is already open.
pub fn open(owner: HWND, cfg: &Config, page: usize) {
    let existing = SETTINGS_HWND.load(Ordering::SeqCst);
    if existing != 0 {
        with(|s| s.show_page(page));
        unsafe {
            let _ = SetForegroundWindow(HWND(existing as *mut _));
        }
        return;
    }
    if let Err(e) = create(owner, cfg, page) {
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
    with(|s| {
        let n = PAGES.len() as i32;
        s.show_page(((s.page as i32 + step + n) % n) as usize);
    });
    true
}

/// Builds one page at a time, top to bottom, with a card per section.
struct Builder {
    parent: HWND,
    font: HFONT,
    scale: f32,
    page: usize,
    /// Next free y on the page.
    y: i32,
    pages: Vec<Vec<HWND>>,
    cards: Vec<(usize, i32, i32, &'static str)>,
    hints: Vec<HWND>,
}

impl Builder {
    fn s(&self, v: i32) -> i32 {
        (v as f32 * self.scale).round() as i32
    }

    fn start_page(&mut self, page: usize) {
        self.page = page;
        self.y = 112;
    }

    /// A new card with a title; controls follow below it.
    fn section(&mut self, title: &'static str) {
        if self.cards.last().is_some_and(|c| c.0 == self.page) {
            self.y += 46;
        }
        self.cards.push((self.page, self.y - 16, self.y, title));
        self.y += 26;
    }

    /// The current card grows to `bottom`.
    fn grow(&mut self, bottom: i32) {
        if let Some(c) = self.cards.last_mut().filter(|c| c.0 == self.page) {
            c.2 = c.2.max(bottom + 14);
        }
    }

    fn make(&mut self, class: PCWSTR, text: &str, style: u32, ex: WINDOW_EX_STYLE, r: (i32, i32, i32, i32), id: usize) -> HWND {
        let h = unsafe {
            let h = CreateWindowExW(
                ex,
                class,
                &HSTRING::from(text),
                WS_CHILD | WINDOW_STYLE(style),
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

    fn check_at(&mut self, text: &str, x: i32, w: i32, on: bool) -> HWND {
        let y = self.y;
        let h = self.make(w!("BUTTON"), text, (BS_AUTOCHECKBOX as u32) | WS_TABSTOP.0, WINDOW_EX_STYLE(0), (x, y, w, 26), 0);
        unsafe { SendMessageW(h, BM_SETCHECK, Some(WPARAM(on as usize)), None) };
        self.grow(y + 26);
        h
    }

    fn check(&mut self, text: &str, on: bool) -> HWND {
        let h = self.check_at(text, X0, CW, on);
        self.y += 30;
        h
    }

    fn label_at(&mut self, text: &str, y: i32) {
        self.make(w!("STATIC"), text, 0, WINDOW_EX_STYLE(0), (X0, y + 5, LABEL - 12, 20), 0);
    }

    fn hint(&mut self, text: &str) {
        let y = self.y;
        let h = self.make(w!("STATIC"), text, 0, WINDOW_EX_STYLE(0), (X0, y, CW, 36), 0);
        self.hints.push(h);
        self.grow(y + 34);
        self.y += 40;
    }

    fn edit(&mut self, label: &str, text: &str) -> HWND {
        let y = self.y;
        self.label_at(label, y);
        let h = self.make(w!("EDIT"), text, (ES_AUTOHSCROLL as u32) | WS_TABSTOP.0, WS_EX_CLIENTEDGE, (CTRL_X, y, CTRL_W, 28), 0);
        self.grow(y + 28);
        self.y += 38;
        h
    }

    fn combo(&mut self, label: &str, items: &[String], selected: usize, id: usize) -> HWND {
        let y = self.y;
        self.label_at(label, y);
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
        self.grow(y + 28);
        self.y += 38;
        h
    }

    fn button_at(&mut self, text: &str, x: i32, y: i32, w: i32, id: usize, default: bool) -> HWND {
        let style = if default { BS_DEFPUSHBUTTON } else { BS_PUSHBUTTON } as u32 | WS_TABSTOP.0;
        let h = self.make(w!("BUTTON"), text, style, WINDOW_EX_STYLE(0), (x, y, w, 32), id);
        self.grow(y + 32);
        h
    }

    /// A row of buttons under the control column.
    fn buttons(&mut self, list: &[(&str, usize)]) {
        let y = self.y;
        let n = list.len() as i32;
        let bw = (CTRL_W - 8 * (n - 1)) / n;
        for (i, (text, id)) in list.iter().enumerate() {
            self.button_at(text, CTRL_X + i as i32 * (bw + 8), y, bw, *id, false);
        }
        self.y += 42;
    }

    fn listview(&mut self, x: i32, w: i32, h: i32, checks: bool, columns: &[(&str, i32)]) -> HWND {
        let y = self.y;
        let style = (LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS) | if columns.len() == 1 { LVS_NOCOLUMNHEADER } else { 0 };
        let lv = self.make(w!("SysListView32"), "", style | WS_TABSTOP.0 | WS_BORDER.0, WINDOW_EX_STYLE(0), (x, y, w, h), 0);
        unsafe {
            let ex = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | if checks { LVS_EX_CHECKBOXES } else { 0 };
            SendMessageW(lv, LVM_SETEXTENDEDLISTVIEWSTYLE, Some(WPARAM(ex as usize)), Some(LPARAM(ex as isize)));
            for (i, (title, width)) in columns.iter().enumerate() {
                let mut text: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
                let col =
                    LVCOLUMNW { mask: LVCF_TEXT | LVCF_WIDTH, cx: self.s(*width), pszText: PWSTR(text.as_mut_ptr()), ..Default::default() };
                SendMessageW(lv, LVM_INSERTCOLUMNW, Some(WPARAM(i)), Some(LPARAM(&col as *const _ as isize)));
            }
        }
        self.grow(y + h);
        self.y += h + 10;
        lv
    }

    fn listbox(&mut self, x: i32, w: i32, h: i32, id: usize) -> HWND {
        let y = self.y;
        let style = (LBS_NOTIFY as u32) | (LBS_NOINTEGRALHEIGHT as u32) | WS_VSCROLL.0 | WS_TABSTOP.0 | WS_BORDER.0;
        let lb = self.make(w!("LISTBOX"), "", style, WINDOW_EX_STYLE(0), (x, y, w, h), id);
        self.grow(y + h);
        self.y += h + 10;
        lb
    }
}

// ---- list helpers -------------------------------------------------------

fn lv_fill(lv: HWND, rows: &[Vec<String>], checks: Option<&[bool]>) {
    unsafe {
        SendMessageW(lv, LVM_DELETEALLITEMS, None, None);
        for (i, row) in rows.iter().enumerate() {
            let mut text: Vec<u16> = row[0].encode_utf16().chain(std::iter::once(0)).collect();
            let item = LVITEMW { mask: LVIF_TEXT, iItem: i as i32, pszText: PWSTR(text.as_mut_ptr()), ..Default::default() };
            SendMessageW(lv, LVM_INSERTITEMW, None, Some(LPARAM(&item as *const _ as isize)));
            for (col, cell) in row.iter().enumerate().skip(1) {
                let mut text: Vec<u16> = cell.encode_utf16().chain(std::iter::once(0)).collect();
                let sub = LVITEMW { iSubItem: col as i32, pszText: PWSTR(text.as_mut_ptr()), ..Default::default() };
                SendMessageW(lv, LVM_SETITEMTEXTW, Some(WPARAM(i)), Some(LPARAM(&sub as *const _ as isize)));
            }
            if let Some(c) = checks {
                // State image 2 is "checked", 1 "unchecked".
                let image = if c.get(i).copied().unwrap_or(false) { 2u32 } else { 1u32 };
                let state =
                    LVITEMW { stateMask: LVIS_STATEIMAGEMASK, state: LIST_VIEW_ITEM_STATE_FLAGS(image << 12), ..Default::default() };
                SendMessageW(lv, LVM_SETITEMSTATE, Some(WPARAM(i)), Some(LPARAM(&state as *const _ as isize)));
            }
        }
    }
}

fn lv_checked(lv: HWND, i: usize) -> bool {
    let state = unsafe { SendMessageW(lv, LVM_GETITEMSTATE, Some(WPARAM(i)), Some(LPARAM(LVIS_STATEIMAGEMASK.0 as isize))).0 as u32 };
    (state >> 12) == 2
}

fn lv_selected(lv: HWND) -> Option<usize> {
    let i = unsafe { SendMessageW(lv, LVM_GETNEXTITEM, Some(WPARAM(usize::MAX)), Some(LPARAM(LVNI_SELECTED as isize))).0 };
    (i >= 0).then_some(i as usize)
}

fn lv_select(lv: HWND, i: usize) {
    unsafe {
        let both = LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0);
        let state = LVITEMW { stateMask: both, state: both, ..Default::default() };
        SendMessageW(lv, LVM_SETITEMSTATE, Some(WPARAM(i)), Some(LPARAM(&state as *const _ as isize)));
        SendMessageW(lv, LVM_ENSUREVISIBLE, Some(WPARAM(i)), Some(LPARAM(0)));
    }
}

fn lb_fill(lb: HWND, items: &[String]) {
    unsafe {
        SendMessageW(lb, LB_RESETCONTENT, None, None);
        for item in items {
            let w = HSTRING::from(item.as_str());
            SendMessageW(lb, LB_ADDSTRING, None, Some(LPARAM(w.as_ptr() as isize)));
        }
    }
}

fn lb_selected(lb: HWND) -> Option<usize> {
    let i = unsafe { SendMessageW(lb, LB_GETCURSEL, None, None).0 };
    (i >= 0).then_some(i as usize)
}

fn create(owner: HWND, cfg: &Config, start_page: usize) -> windows::core::Result<()> {
    let hinstance = unsafe { GetModuleHandleW(None)? };
    let class = w!("NgambangSettings");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: hinstance.into(),
        lpszClassName: class,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
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
            ((sy - wh) / 2).max(0),
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
    let mut b = Builder { parent: hwnd, font, scale, page: 0, y: 112, pages: Vec::new(), cards: Vec::new(), hints: Vec::new() };
    let form = Form::from_config(cfg, system::autostart_enabled());

    // General
    b.start_page(0);
    b.section("STARTUP");
    let autostart = b.check("Start with Windows", form.autostart);
    let reduce_motion = b.check("Reduce motion (no morphs or pop-ups)", form.reduce_motion);
    let hide_from_capture = b.check("Hide the island from screenshots and screen sharing", form.hide_from_capture);
    let hide_in_fullscreen = b.check("Sink to a thin line while a fullscreen app has focus", form.hide_in_fullscreen);
    let move_in_fullscreen = b.check("With two or more displays, move to another one while a fullscreen app runs", form.move_in_fullscreen);
    let hotkey = b.edit("Open with hotkey", &form.hotkey);
    b.section("POSITION");
    let looks: Vec<String> = ["Orb on any edge", "Notch at the top"].map(String::from).to_vec();
    let style = b.combo("Look", &looks, Style::ALL.iter().position(|s| *s == form.style).unwrap_or(0), ID_STYLE);
    let edges: Vec<String> = ["Left", "Right", "Top", "Bottom"].map(String::from).to_vec();
    let edge = b.combo("Screen edge", &edges, Edge::ALL.iter().position(|e| *e == form.edge).unwrap_or(0), 0);
    let mut monitors = vec!["primary".to_string()];
    monitors.extend(system::secondary_monitors());
    if !monitors.iter().any(|m| m.eq_ignore_ascii_case(&form.monitor)) {
        monitors.push(form.monitor.clone());
    }
    let labels: Vec<String> = monitors
        .iter()
        .map(|m| if m == "primary" { "Primary display".to_string() } else { m.trim_start_matches(r"\\.\").to_string() })
        .collect();
    let monitor = b.combo("Monitor", &labels, monitors.iter().position(|m| m.eq_ignore_ascii_case(&form.monitor)).unwrap_or(0), 0);
    let offset = b.edit("Along the edge (%)", &form.offset_pct);
    b.hint("Tip: drag the orb, a tag or an open slip to move the island to any edge; a notch slides along the top.");

    // Modules: one list, ticked to show, in tab order.
    b.start_page(1);
    b.section("MODULES AND TAB ORDER");
    let list_y = b.y;
    let modules = b.listview(X0, CW - 140, 336, true, &[("Module", CW - 170)]);
    b.button_at("Move up", X0 + CW - 128, list_y, 128, ID_MODULE_UP, false);
    b.button_at("Move down", X0 + CW - 128, list_y + 40, 128, ID_MODULE_DOWN, false);
    b.hint("Ticked modules show on the island, in this order along the tab rail. The most urgent one still opens first.");

    // Media & clipboard
    b.start_page(2);
    b.section("MEDIA");
    let track_tags = b.check("Pop up the song, cover and controls when a track starts", form.track_tags);
    let volume_step = b.edit("Volume step (%)", &form.volume_step);
    b.section("CLIPBOARD");
    let clipboard_tags = b.check("Show a tag for JWT, JSON, UUIDs, timestamps, colours and links", form.clipboard_tags);

    // Focus & commands
    b.start_page(3);
    b.section("FOCUS TIMER");
    let presets = b.edit("Preset rounds (minutes)", &form.timer_presets);
    let custom = b.edit("Custom round (minutes)", &form.timer_custom);
    let break_min = b.edit("Break after a round (minutes)", &form.break_min);
    let long_break = b.edit("Long break (minutes)", &form.long_break_min);
    let long_every = b.edit("Long break every (rounds)", &form.long_every);
    let focus_assist = b.check("Turn on Do not disturb during rounds (Windows 11)", form.focus_assist);
    b.section("COMMANDS");
    let long_command = b.edit("Long command (seconds)", &form.long_command_s);
    b.hint("Used by the shell hook: run `ngambang hook powershell` to set it up.");

    // Games
    b.start_page(4);
    b.section("WHILE PLAYING");
    let break_after = b.edit("Break reminder (minutes)", &form.break_after_min);
    let ping_host = b.edit("Ping host", &form.ping_host);
    let plans = system::power_plans();
    let mut plan_items = vec!["Don't switch".to_string()];
    plan_items.extend(plans.iter().map(|(_, n)| n.clone()));
    let plan_index = if form.power_plan.is_empty() {
        0
    } else {
        let want = ngambang_core::format::parse_guid(&form.power_plan);
        plans.iter().position(|(g, _)| ngambang_core::format::parse_guid(g) == want).map(|i| i + 1).unwrap_or(0)
    };
    let power_plan = b.combo("Power plan in games", &plan_items, plan_index, 0);
    b.section("BATTERY & NETWORK");
    let plan_at = |guid: &str| {
        let want = ngambang_core::format::parse_guid(guid);
        plans.iter().position(|(g, _)| !guid.is_empty() && ngambang_core::format::parse_guid(g) == want).map(|i| i + 1).unwrap_or(0)
    };
    let battery_plan = b.combo("Power plan on battery", &plan_items, plan_at(&form.battery_plan), 0);
    let plugged_plan = b.combo("Power plan plugged in", &plan_items, plan_at(&form.plugged_plan), 0);
    let low_battery = b.edit("Warn at battery level (%)", &form.low_battery);
    let network_tags = b.check("Tell me when the connection or a VPN drops or comes back", form.network_tags);

    // Location & prayer
    b.start_page(5);
    b.section("LOCATION");
    let cities = ngambang_core::prayer::CITIES;
    let mut city_items = vec!["Custom (coordinates below)".to_string()];
    city_items.extend(cities.iter().map(|c| format!("{} — {}", c.0, c.1)));
    let city_index = cities.iter().position(|c| c.0.eq_ignore_ascii_case(&form.prayer_city)).map(|i| i + 1).unwrap_or(0);
    let city = b.combo("City", &city_items, city_index, ID_CITY);
    let lat = b.edit("Latitude", &form.prayer_lat);
    let lon = b.edit("Longitude", &form.prayer_lon);
    let y = b.y;
    let locate = b.button_at("Use my location", CTRL_X, y, CTRL_W, ID_LOCATE, false);
    b.y += 42;
    b.section("JADWAL SHOLAT");
    let methods: Vec<String> = ngambang_core::prayer::Method::ALL.iter().map(|m| m.label().to_string()).collect();
    let method = b.combo(
        "Calculation method",
        &methods,
        ngambang_core::prayer::Method::ALL.iter().position(|m| *m == form.prayer_method).unwrap_or(0),
        0,
    );
    let asr_items = vec!["Standard (Shafi'i, Maliki, Hanbali)".to_string(), "Hanafi".to_string()];
    let asr = b.combo("Asr", &asr_items, (form.prayer_asr == ngambang_core::prayer::Asr::Hanafi) as usize, 0);
    let reminder = b.edit("Reminder (minutes before)", &form.prayer_reminder);
    let hijri_adjust = b.edit("Hijri date correction (days)", &form.hijri_adjust);

    // Weather & updates
    b.start_page(6);
    b.section("WEATHER & EARLY WARNINGS");
    let weather_alerts = b.check("Pop up disasters near my location", form.weather_alerts);
    let rain_tags = b.check("Pop up shortly before rain starts", form.rain_tags);
    let quake_radius = b.edit("Earthquake radius (km)", &form.quake_radius_km);
    b.hint("Weather by Open-Meteo; warnings from GDACS and USGS. Nothing is fetched while offline.");
    b.section("UPDATES");
    let update_check = b.check("Check GitHub for new versions once a day", form.update_check);
    b.hint("To check now, right-click the orb or the tray icon: Check for updates.");

    // Calendar
    b.start_page(7);
    b.section("LINKED CALENDARS");
    b.hint("In Google Calendar: Settings ▸ your calendar ▸ Integrate calendar ▸ \"Secret address in iCal format\". Pick a calendar below to edit it.");
    let feeds = b.listview(X0, CW, 96, false, &[("Calendar", 150), ("Colour", 70), ("Address", CW - 230)]);
    let feed_name = b.edit("Name", "");
    let feed_url = b.edit("Secret iCal address", "");
    let colors: Vec<String> = CALENDAR_COLORS.iter().map(|c| c.0.to_string()).collect();
    let feed_color = b.combo("Colour", &colors, form.calendar_feeds.len() % CALENDAR_COLORS.len(), 0);
    b.buttons(&[("Add", ID_FEED_ADD), ("Save changes", ID_FEED_UPDATE), ("Remove", ID_FEED_REMOVE)]);
    b.section("REMINDERS");
    let cal_reminder = b.edit("Remind me (minutes before)", &form.calendar_reminder);
    let cal_days = b.edit("Days ahead in the slip", &form.calendar_days);

    // Markets
    b.start_page(8);
    b.section("FIND A SYMBOL");
    let search = b.make(w!("EDIT"), "", (ES_AUTOHSCROLL as u32) | WS_TABSTOP.0, WS_EX_CLIENTEDGE, (CTRL_X, b.y, CTRL_W, 28), ID_SEARCH);
    b.label_at("Search", b.y);
    b.grow(b.y + 28);
    b.y += 34;
    let search_status =
        b.make(w!("STATIC"), "Type a company, index, coin or currency pair", 0, WINDOW_EX_STYLE(0), (CTRL_X, b.y, CTRL_W, 18), 0);
    b.hints.push(search_status);
    b.y += 24;
    let results = b.listbox(CTRL_X, CTRL_W, 60, ID_RESULTS);
    b.buttons(&[("Add to watchlist", ID_WATCH_ADD)]);
    b.section("YOUR WATCHLIST");
    let wy = b.y;
    let watch = b.listbox(X0, CW - 140, 112, ID_WATCH);
    b.button_at("Move up", X0 + CW - 128, wy, 128, ID_WATCH_UP, false);
    b.button_at("Move down", X0 + CW - 128, wy + 40, 128, ID_WATCH_DOWN, false);
    b.button_at("Remove", X0 + CW - 128, wy + 80, 128, ID_WATCH_REMOVE, false);
    // Alerts on one row: big moves (with the step in %) and fresh headlines.
    let price_alerts = b.check_at("Tag moves of (%)", X0, 150, form.price_alerts);
    let move_pct =
        b.make(w!("EDIT"), &form.move_pct, (ES_AUTOHSCROLL as u32) | WS_TABSTOP.0, WS_EX_CLIENTEDGE, (X0 + 154, b.y - 1, 52, 28), 0);
    let news = b.check_at("Tag fresh headlines about the watchlist", X0 + 232, CW - 232, form.news);
    b.y += 30;
    // Price levels for the selected symbol.
    let ly = b.y;
    b.label_at("Alert above / below", ly - 4);
    let above = b.make(w!("EDIT"), "", (ES_AUTOHSCROLL as u32) | WS_TABSTOP.0, WS_EX_CLIENTEDGE, (CTRL_X, ly, 110, 28), 0);
    let below = b.make(w!("EDIT"), "", (ES_AUTOHSCROLL as u32) | WS_TABSTOP.0, WS_EX_CLIENTEDGE, (CTRL_X + 118, ly, 110, 28), 0);
    b.button_at("Set for selected", CTRL_X + 236, ly - 2, 150, ID_WATCH_LEVELS, false);
    b.y += 34;

    // Notch & screen
    b.start_page(9);
    b.section("NOTCH");
    let notch_glances = b.edit("Turns through", &form.notch_glances);
    let notch_glance_s = b.edit("Seconds each", &form.notch_glance_s);
    b.hint("Any of weather, markets, todo, calendar, prayer, in the order you want. Music always comes first while it plays.");
    b.section("FULLSCREEN APPS & SHARING");
    let hide_when_sharing = b.check("Hide while I share my screen in Zoom, Teams, Meet …", form.hide_when_sharing);
    let snap = b.check("Snap to the middle and the ends of an edge while dragging", form.snap);
    let fullscreen_ignore = b.edit("Never move for", &form.fullscreen_ignore);
    let fullscreen_sink = b.edit("Sink instead of moving for", &form.fullscreen_sink);
    b.hint("Program names separated by commas, like obs64.exe, vlc.exe.");

    // About
    b.start_page(10);
    b.section("NGAMBANG");
    for (label, value) in [("Version", ngambang_core::VERSION), ("Made by", AUTHOR), ("Source code", REPO)] {
        let y = b.y;
        b.label_at(label, y);
        b.make(w!("STATIC"), value, 0, WINDOW_EX_STYLE(0), (CTRL_X, y + 5, CTRL_W, 20), 0);
        b.grow(y + 28);
        b.y += 32;
    }
    b.buttons(&[("Open on GitHub", ID_REPO), ("Report an issue", ID_ISSUES)]);
    b.hint("An edge-docked floating island for Windows. MIT + Commons Clause licence; see LICENSE and THIRD_PARTY_NOTICES.md.");

    // Footer, on every page.
    b.page = PAGES.len();
    let footer: Vec<HWND> = vec![
        b.button_at("Open config file", X0, H - 48, 140, ID_OPEN_FILE, false),
        b.button_at("Cancel", W - PAD - 96 - 8 - 96, H - 48, 96, ID_CANCEL, false),
        b.button_at("Save", W - PAD - 96, H - 48, 96, ID_SAVE, true),
    ];
    for h in &footer {
        unsafe {
            let _ = ShowWindow(*h, SW_SHOWNA);
        }
    }
    unsafe {
        let cue = HSTRING::from("BBCA, bitcoin, ^JKSE, USD/IDR…");
        SendMessageW(search, EM_SETCUEBANNER, Some(WPARAM(1)), Some(LPARAM(cue.as_ptr() as isize)));
        let cue = HSTRING::from("https://calendar.google.com/…/basic.ics");
        SendMessageW(feed_url, EM_SETCUEBANNER, Some(WPARAM(1)), Some(LPARAM(cue.as_ptr() as isize)));
    }

    let c = Controls {
        autostart,
        hide_in_fullscreen,
        move_in_fullscreen,
        style,
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
        rain_tags,
        notch_glances,
        notch_glance_s,
        hide_when_sharing,
        snap,
        fullscreen_ignore,
        fullscreen_sink,
        quake_radius,
        update_check,
        feeds,
        feed_name,
        feed_url,
        feed_color,
        cal_reminder,
        cal_days,
        search,
        search_status,
        results,
        watch,
        price_alerts,
        move_pct,
        news,
        above,
        below,
        hijri_adjust,
        break_min,
        long_break,
        long_every,
        focus_assist,
        battery_plan,
        plugged_plan,
        low_battery,
        network_tags,
    };
    let painter = unsafe {
        D2D1CreateFactory::<ID2D1Factory1>(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&D2D1_FACTORY_OPTIONS::default()))
            .ok()
            .and_then(|f| Painter::new(f).ok())
    };
    let colorref = COLORREF(((CONTENT.b() as u32) << 16) | ((CONTENT.g() as u32) << 8) | CONTENT.r() as u32);
    let content_brush = unsafe { CreateSolidBrush(colorref) };
    let mut s = Settings {
        hwnd,
        owner,
        font,
        content_brush,
        c,
        monitors,
        plans,
        custom_city: if city_index == 0 { form.prayer_city.clone() } else { "My location".into() },
        order: form.module_order.iter().map(|m| (*m, form.modules.get(*m))).collect(),
        feeds: form.calendar_feeds.clone(),
        watchlist: form.watchlist.clone(),
        hits: Vec::new(),
        page: usize::MAX,
        hover: None,
        tracking: false,
        pages: b.pages,
        cards: b.cards,
        hints: b.hints,
        scale,
        painter,
        target: None,
    };
    s.fill_modules();
    s.fill_feeds();
    s.fill_watchlist();
    s.show_page(start_page.min(PAGES.len() - 1));
    WINDOW.with(|w| *w.borrow_mut() = Some(s));
    SETTINGS_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(())
}

fn checked(h: HWND) -> bool {
    unsafe { SendMessageW(h, BM_GETCHECK, None, None).0 == 1 }
}

fn text(h: HWND) -> String {
    let mut buf = [0u16; 1024];
    let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
    from_wide(&buf[..n.min(buf.len())])
}

fn selected(h: HWND) -> usize {
    unsafe { SendMessageW(h, CB_GETCURSEL, None, None).0.max(0) as usize }
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
        for (i, (m, _)) in self.order.iter().enumerate() {
            modules.set(*m, lv_checked(c.modules, i));
        }
        let plan = selected(c.power_plan);
        Form {
            autostart: checked(c.autostart),
            edge: Edge::ALL[selected(c.edge).min(3)],
            offset_pct: text(c.offset),
            monitor: self.monitors.get(selected(c.monitor)).cloned().unwrap_or_else(|| "primary".into()),
            hotkey: text(c.hotkey),
            hide_in_fullscreen: checked(c.hide_in_fullscreen),
            move_in_fullscreen: checked(c.move_in_fullscreen),
            style: Style::ALL[selected(c.style).min(Style::ALL.len() - 1)],
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
            module_order: self.order.iter().map(|(m, _)| *m).collect(),
            calendar_feeds: self.feeds.clone(),
            calendar_reminder: text(c.cal_reminder),
            calendar_days: text(c.cal_days),
            watchlist: self.watchlist.clone(),
            price_alerts: checked(c.price_alerts),
            move_pct: text(c.move_pct),
            news: checked(c.news),
            hijri_adjust: text(c.hijri_adjust),
            break_min: text(c.break_min),
            long_break_min: text(c.long_break),
            long_every: text(c.long_every),
            focus_assist: checked(c.focus_assist),
            battery_plan: match selected(c.battery_plan) {
                0 => String::new(),
                i => self.plans.get(i - 1).map(|p| p.0.clone()).unwrap_or_default(),
            },
            plugged_plan: match selected(c.plugged_plan) {
                0 => String::new(),
                i => self.plans.get(i - 1).map(|p| p.0.clone()).unwrap_or_default(),
            },
            low_battery: text(c.low_battery),
            network_tags: checked(c.network_tags),
            notch_glances: text(c.notch_glances),
            notch_glance_s: text(c.notch_glance_s),
            fullscreen_ignore: text(c.fullscreen_ignore),
            fullscreen_sink: text(c.fullscreen_sink),
            hide_when_sharing: checked(c.hide_when_sharing),
            snap: checked(c.snap),
            rain_tags: checked(c.rain_tags),
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
                    "Latitude" | "Longitude" | "Prayer reminder" | "Hijri date" => 5,
                    "Break" | "Long break" | "Long break every" => 3,
                    "Low battery" | "Battery plan" | "Plugged-in plan" => 4,
                    "Earthquake radius" => 6,
                    "Calendar address" | "Event reminder" | "Days ahead" => 7,
                    "Price move" => 8,
                    "Notch glances" | "Notch glance" | "Never move for" | "Sink instead of moving for" => 9,
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

    fn fill_modules(&self) {
        let rows: Vec<Vec<String>> = self.order.iter().map(|(m, _)| vec![m.label().to_string()]).collect();
        let checks: Vec<bool> = self.order.iter().map(|(_, on)| *on).collect();
        lv_fill(self.c.modules, &rows, Some(&checks));
    }

    /// Move the selected module up (-1) or down (+1), keeping every tick.
    fn move_module(&mut self, dir: i32) {
        for i in 0..self.order.len() {
            self.order[i].1 = lv_checked(self.c.modules, i);
        }
        let Some(i) = lv_selected(self.c.modules) else { return };
        let j = i as i32 + dir;
        if j < 0 || j >= self.order.len() as i32 {
            return;
        }
        self.order.swap(i, j as usize);
        self.fill_modules();
        lv_select(self.c.modules, j as usize);
        unsafe {
            let _ = SetFocus(Some(self.c.modules));
        }
    }

    fn fill_feeds(&self) {
        let rows: Vec<Vec<String>> = self
            .feeds
            .iter()
            .map(|f| {
                let colour = CALENDAR_COLORS.iter().find(|c| c.1.eq_ignore_ascii_case(&f.color)).map(|c| c.0).unwrap_or("—");
                vec![f.name.clone(), colour.to_string(), f.url.clone()]
            })
            .collect();
        lv_fill(self.c.feeds, &rows, None);
    }

    /// The calendar described by the Name / address / Colour fields.
    fn feed_from_fields(&self, index: usize) -> Result<CalendarFeed, String> {
        let url = text(self.c.feed_url).trim().to_string();
        if !(url.starts_with("https://") || url.starts_with("webcal://")) {
            return Err("Paste the calendar's secret iCal address: it starts with https:// (or webcal://).".into());
        }
        let name = text(self.c.feed_name).trim().to_string();
        let name = if name.is_empty() { format!("Calendar {}", index + 1) } else { name };
        let color = CALENDAR_COLORS[selected(self.c.feed_color).min(CALENDAR_COLORS.len() - 1)].1.to_string();
        Ok(CalendarFeed { name, url, color })
    }

    /// A calendar was picked in the list: put it in the fields to edit.
    fn feed_selected(&self) {
        let Some(f) = lv_selected(self.c.feeds).and_then(|i| self.feeds.get(i)) else { return };
        set_text(self.c.feed_name, &f.name);
        set_text(self.c.feed_url, &f.url);
        let colour = CALENDAR_COLORS.iter().position(|c| c.1.eq_ignore_ascii_case(&f.color)).unwrap_or(0);
        unsafe { SendMessageW(self.c.feed_color, CB_SETCURSEL, Some(WPARAM(colour)), None) };
    }

    fn update_feed(&mut self) -> Result<(), String> {
        let Some(i) = lv_selected(self.c.feeds) else { return Err("Pick a calendar in the list first.".into()) };
        self.feeds[i] = self.feed_from_fields(i)?;
        self.fill_feeds();
        lv_select(self.c.feeds, i);
        Ok(())
    }

    fn add_feed(&mut self) -> Result<(), String> {
        let feed = self.feed_from_fields(self.feeds.len())?;
        self.feeds.push(feed);
        self.fill_feeds();
        set_text(self.c.feed_name, "");
        set_text(self.c.feed_url, "");
        unsafe { SendMessageW(self.c.feed_color, CB_SETCURSEL, Some(WPARAM(self.feeds.len() % CALENDAR_COLORS.len())), None) };
        Ok(())
    }

    fn fill_watchlist(&self) {
        let items: Vec<String> = self
            .watchlist
            .iter()
            .map(|w| {
                let mut label = if w.name.is_empty() { w.symbol.clone() } else { format!("{} — {}", w.symbol, w.name) };
                if let Some(a) = w.above {
                    label += &format!("  ↑{}", ngambang_core::finance::format_level(a));
                }
                if let Some(b) = w.below {
                    label += &format!("  ↓{}", ngambang_core::finance::format_level(b));
                }
                label
            })
            .collect();
        lb_fill(self.c.watch, &items);
    }

    /// The alert levels typed for the selected symbol; an error to show, if any.
    fn set_levels(&mut self) -> Option<String> {
        let Some(i) = lb_selected(self.c.watch) else { return Some("Pick a symbol in your watchlist first.".into()) };
        let parse = |t: String| -> Result<Option<f64>, String> {
            let t = t.trim().replace(',', "");
            if t.is_empty() {
                return Ok(None);
            }
            t.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0).map(Some).ok_or_else(|| format!("\"{t}\" is not a price"))
        };
        let (above, below) = match (parse(text(self.c.above)), parse(text(self.c.below))) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => return Some(e),
        };
        if let Some(w) = self.watchlist.get_mut(i) {
            w.above = above;
            w.below = below;
        }
        self.fill_watchlist();
        unsafe { SendMessageW(self.c.watch, LB_SETCURSEL, Some(WPARAM(i)), None) };
        None
    }

    fn add_watch(&mut self) {
        // The selected result, else the first one, else the text as a symbol.
        let hit = lb_selected(self.c.results).and_then(|i| self.hits.get(i)).or(self.hits.first()).cloned();
        let item = match hit {
            Some(h) => WatchItem { symbol: h.symbol, name: h.name, ..Default::default() },
            None => {
                let typed = text(self.c.search).trim().to_uppercase();
                if typed.is_empty() || typed.contains(' ') {
                    return;
                }
                WatchItem { symbol: typed, name: String::new(), ..Default::default() }
            }
        };
        if self.watchlist.iter().any(|w| w.symbol.eq_ignore_ascii_case(&item.symbol)) || self.watchlist.len() >= 20 {
            return;
        }
        self.watchlist.push(item);
        self.fill_watchlist();
        unsafe { SendMessageW(self.c.watch, LB_SETCURSEL, Some(WPARAM(self.watchlist.len() - 1)), None) };
    }

    fn move_watch(&mut self, dir: i32) {
        let Some(i) = lb_selected(self.c.watch) else { return };
        let j = i as i32 + dir;
        if j < 0 || j >= self.watchlist.len() as i32 {
            return;
        }
        self.watchlist.swap(i, j as usize);
        self.fill_watchlist();
        unsafe { SendMessageW(self.c.watch, LB_SETCURSEL, Some(WPARAM(j as usize)), None) };
    }

    /// Start a search for what is in the search box (after typing pauses).
    fn search(&self) {
        let query = text(self.c.search);
        let seq = SEARCH_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
        if query.trim().is_empty() {
            lb_fill(self.c.results, &[]);
            set_text(self.c.search_status, "Type a company, index, coin or currency pair");
            return;
        }
        set_text(self.c.search_status, "Searching…");
        let target = self.hwnd.0 as isize;
        let _ = std::thread::Builder::new().name("ngambang-search".into()).spawn(move || {
            let result = super::net::search_symbols(&query);
            *SEARCHED.lock().unwrap_or_else(|e| e.into_inner()) = Some((seq, result));
            unsafe {
                let _ = PostMessageW(Some(HWND(target as *mut _)), WM_SEARCHED, WPARAM(0), LPARAM(0));
            }
        });
    }

    fn searched(&mut self) {
        let Some((seq, result)) = SEARCHED.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        if seq != SEARCH_SEQ.load(Ordering::SeqCst) {
            return; // An older search; a newer one is on its way.
        }
        match result {
            Ok(hits) => {
                let status = match hits.len() {
                    0 => "No matches. \"Add to watchlist\" takes the text as a symbol (like BBCA.JK).".to_string(),
                    n => format!("{n} matches. Double-click one to add it."),
                };
                lb_fill(self.c.results, &hits.iter().map(|h| h.label()).collect::<Vec<_>>());
                self.hits = hits;
                set_text(self.c.search_status, &status);
            }
            Err(e) => {
                self.hits.clear();
                lb_fill(self.c.results, &[]);
                set_text(self.c.search_status, &format!("Search failed: {e}"));
            }
        }
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
        let text = |p: &mut Vec<Prim>, r: Rect, t: &str, style: TextStyle, color: Color| {
            p.push(Prim::Text { rect: r, text: t.into(), style, color, align: Align::Start, wrap: false })
        };
        // Content background and the sidebar.
        p.push(Prim::RoundRect { rect: Rect::new(0.0, 0.0, W as f32, H as f32), radius: 0.0, fill: Some(CONTENT), stroke: None });
        p.push(Prim::RoundRect { rect: Rect::new(0.0, 0.0, SIDE as f32, H as f32), radius: 0.0, fill: Some(theme::SURFACE), stroke: None });

        // The orb as a logo.
        let (cx, cy) = (40.0, 44.0);
        p.extend(soft::raised_circle(cx, cy, 19.0, theme::SURFACE, 3.0));
        p.extend(soft::well_circle(cx, cy, 14.0));
        p.push(Prim::Polyline { points: arc_points(cx, cy, 11.5, -90.0, 130.0, 32), width: 3.0, color: theme::MEDIA, closed: false });
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
            let r = Rect::new(14.0, (NAV_TOP + i as i32 * (NAV_H + NAV_GAP)) as f32, (SIDE - 28) as f32, NAV_H as f32);
            let active = i == self.page;
            if active {
                p.extend(soft::well(r, 12.0));
            } else if self.hover == Some(i) {
                p.extend(soft::raised(r, 12.0, theme::SURFACE, 3.0));
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

        // Page header: a raised icon tile, title, subtitle.
        let page = &PAGES[self.page];
        let tile = Rect::new(X0 as f32, 26.0, 46.0, 46.0);
        p.extend(soft::raised_with(tile, 14.0, CONTENT, 5.0, CARD_DARK, CARD_LIGHT));
        p.extend(icon_prims(page.icon, tile.inset(12.0), INK));
        text(
            &mut p,
            Rect::new(X0 as f32 + 62.0, 24.0, CW as f32 - 62.0, 28.0),
            page.title,
            TextStyle { size: 20.0, weight: Weight::Semibold, mono: false },
            INK,
        );
        text(
            &mut p,
            Rect::new(X0 as f32 + 62.0, 52.0, CW as f32 - 62.0, 20.0),
            page.subtitle,
            TextStyle { size: 13.0, weight: Weight::Regular, mono: false },
            INK_2,
        );
        // A soft card behind each section.
        for (_, top, bottom, title) in self.cards.iter().filter(|c| c.0 == self.page) {
            let card = Rect::new((X0 - 18) as f32, *top as f32, (CW + 36) as f32, (bottom - top) as f32);
            p.extend(soft::raised_with(card, 18.0, CONTENT, 6.0, CARD_DARK, CARD_LIGHT));
            text(
                &mut p,
                Rect::new(X0 as f32, *top as f32 + 12.0, CW as f32, 18.0),
                title,
                TextStyle { size: 11.0, weight: Weight::Semibold, mono: false },
                INK_2,
            );
        }
        p.push(Prim::RoundRect {
            rect: Rect::new(X0 as f32, FOOTER as f32, CW as f32, 1.0),
            radius: 0.0,
            fill: Some(CARD_DARK.with_alpha(0x80)),
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
                FillRect(hdc, &rc, self.content_brush);
            }
            let _ = EndPaint(self.hwnd, &ps);
        }
    }
}

fn with<R>(f: impl FnOnce(&mut Settings) -> R) -> Option<R> {
    WINDOW.with(|w| w.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn warn(hwnd: HWND, msg: &str) {
    unsafe {
        MessageBoxW(Some(hwnd), &HSTRING::from(msg), w!("Ngambang settings"), MB_ICONWARNING);
    }
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
            // Labels and checkboxes sit on the soft cards; hints are grey.
            let hdc = HDC(wp.0 as *mut _);
            let (hint, brush) = with(|s| (s.hints.contains(&HWND(lp.0 as *mut _)), s.content_brush)).unwrap_or((false, HBRUSH::default()));
            unsafe {
                SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, COLORREF(if hint { 0x006A_605A } else { 0x0020_1D1B }));
                LRESULT(if brush.is_invalid() { GetSysColorBrush(COLOR_BTNFACE).0 as isize } else { brush.0 as isize })
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
        WM_TIMER if wp.0 == TIMER_SEARCH => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_SEARCH);
            }
            with(|s| s.search());
            LRESULT(0)
        }
        WM_SEARCHED => {
            with(|s| s.searched());
            LRESULT(0)
        }
        WM_NOTIFY => {
            // A different calendar picked in the list: show it for editing.
            let hdr = unsafe { &*(lp.0 as *const NMHDR) };
            if hdr.code == LVN_ITEMCHANGED {
                let nm = unsafe { &*(lp.0 as *const NMLISTVIEW) };
                if (nm.uNewState & LVIS_SELECTED.0) != 0 && (nm.uOldState & LVIS_SELECTED.0) == 0 {
                    with(|s| {
                        if hdr.hwndFrom == s.c.feeds {
                            s.feed_selected();
                        }
                    });
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wp.0 & 0xFFFF;
            let code = (wp.0 >> 16) as u32;
            match id {
                ID_SAVE => match with(|s| s.save()) {
                    Some(Ok(())) => unsafe {
                        let _ = DestroyWindow(hwnd);
                    },
                    Some(Err(msg)) => warn(hwnd, &msg),
                    None => {}
                },
                ID_CANCEL => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                ID_REPO => system::shell_open(REPO),
                ID_ISSUES => system::shell_open(&format!("{REPO}/issues")),
                // A notch starts in the middle of the top edge.
                ID_STYLE if code == CBN_SELCHANGE => {
                    with(|s| {
                        if Style::ALL.get(selected(s.c.style)) == Some(&Style::Notch) {
                            set_text(s.c.offset, "50");
                        }
                    });
                }
                ID_CITY if code == CBN_SELCHANGE => {
                    with(|s| {
                        if let Some(c) = selected(s.c.city).checked_sub(1).and_then(|i| ngambang_core::prayer::CITIES.get(i)) {
                            set_text(s.c.lat, &format!("{:.4}", c.2));
                            set_text(s.c.lon, &format!("{:.4}", c.3));
                        }
                    });
                }
                ID_MODULE_UP | ID_MODULE_DOWN => {
                    with(|s| s.move_module(if id == ID_MODULE_UP { -1 } else { 1 }));
                }
                ID_FEED_ADD => {
                    if let Some(Err(msg)) = with(|s| s.add_feed()) {
                        warn(hwnd, &msg);
                    }
                }
                ID_FEED_UPDATE => {
                    if let Some(Err(msg)) = with(|s| s.update_feed()) {
                        warn(hwnd, &msg);
                    }
                }
                ID_FEED_REMOVE => {
                    with(|s| {
                        if let Some(i) = lv_selected(s.c.feeds) {
                            s.feeds.remove(i);
                            s.fill_feeds();
                        }
                    });
                }
                ID_SEARCH if code == EN_CHANGE => unsafe {
                    // Search once typing pauses.
                    SetTimer(Some(hwnd), TIMER_SEARCH, 450, None);
                },
                ID_RESULTS if code == LBN_DBLCLK => {
                    with(|s| s.add_watch());
                }
                ID_WATCH_ADD => {
                    with(|s| s.add_watch());
                }
                ID_WATCH_UP | ID_WATCH_DOWN => {
                    with(|s| s.move_watch(if id == ID_WATCH_UP { -1 } else { 1 }));
                }
                ID_WATCH_LEVELS => {
                    let err = with(|s| s.set_levels()).flatten();
                    if let Some(msg) = err {
                        warn(hwnd, &msg);
                    }
                }
                ID_WATCH if code == LBN_SELCHANGE => {
                    with(|s| {
                        if let Some(w) = lb_selected(s.c.watch).and_then(|i| s.watchlist.get(i)) {
                            let f = |v: Option<f64>| v.map(ngambang_core::finance::format_level).unwrap_or_default();
                            set_text(s.c.above, &f(w.above));
                            set_text(s.c.below, &f(w.below));
                        }
                    });
                }
                ID_WATCH_REMOVE => {
                    with(|s| {
                        if let Some(i) = lb_selected(s.c.watch) {
                            s.watchlist.remove(i);
                            s.fill_watchlist();
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
                    let _ = DeleteObject(s.content_brush.into());
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
