//! The island window and the loop that connects OS input, workers, the core
//! model and the renderer.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use ngambang_core::config::{set_toml_value, Config, ModuleId, Style, DEFAULT_TOML};
use ngambang_core::geometry::{Edge, Rect};
use ngambang_core::hotkey::Hotkey;
use ngambang_core::layout;
use ngambang_core::model::{Action, Effect, Event, IslandState, Key, Model, PlayLog, Sampler, SliderTarget, TimerId};
use ngambang_core::theme;
use ngambang_core::view::{self, HitKind, Prim, Scene, SceneKind};
use serde::{Deserialize, Serialize};
use windows::core::{w, Result, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::DataExchange::{AddClipboardFormatListener, RemoveClipboardFormatListener};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_NEW, ABM_REMOVE, ABN_FULLSCREENAPP, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::audio::AudioWorker;
use super::comp::{Anim, Renderer};
use super::gfx::Gfx;
use super::media::MediaWorker;
use super::system::{self, StatsSampler};
use super::tray::{self, Tray};
use super::util::*;
use super::watch::{self, Stopper};
use super::{clipboard, pipe, ports};

pub struct RunOptions {
    /// Log informational messages too.
    pub verbose: bool,
}

const TIMER_CORE: usize = 100;
const TIMER_SECOND: usize = 200;
const TIMER_PORTS: usize = 201;
const TIMER_CLEANUP: usize = 300;
const TIMER_MIDNIGHT: usize = 301;
/// Trims the working set once the island has been at rest for a while.
const TIMER_TRIM: usize = 302;
const TRIM_AFTER_MS: u32 = 10_000;
/// Looks at the foreground window again: borderless "windowed fullscreen"
/// games often resize after they take focus, which raises no event.
const TIMER_FULLSCREEN: usize = 303;
const FULLSCREEN_POLL_MS: u32 = 1500;
/// Compares the model's clock with Windows' now and then: a time sync, a zone
/// change or a sleep that sent no message would otherwise shift reminders.
const TIMER_CLOCK: usize = 304;
const CLOCK_CHECK_MS: u32 = 60_000;
/// A fullscreen app that did not look like a game is measured again this often
/// (games often load with a quiet GPU).
const REPROBE_MS: u64 = 10_000;
const HOTKEY_ID: i32 = 1;
const DRAG_THRESHOLD: f32 = 8.0;
/// Not exported by the WindowsAndMessaging module.
const WM_MOUSELEAVE: u32 = 0x02A3;

static HOOK_HWND: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct SavedState {
    playlog: PlayLog,
    /// Power plan to restore if we exited during a game.
    power_restore: Option<String>,
    /// Last weather and warnings, shown while offline.
    weather: Option<ngambang_core::weather::Weather>,
    alerts: Vec<ngambang_core::weather::Alert>,
    todos: Vec<ngambang_core::model::TodoItem>,
    /// Upcoming events from linked calendars, shown while offline.
    calendar: Vec<ngambang_core::calendar::CalEvent>,
    /// The Indonesian holiday calendar was linked once (removing it sticks).
    holidays_linked: bool,
    /// Pinned clipboard entries (the rest of the history is not kept).
    clip_pins: Vec<ngambang_core::model::ClipEntry>,
    notes: Vec<ngambang_core::model::Note>,
}

fn load_state() -> SavedState {
    std::fs::read_to_string(crate::paths::state_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_state(s: &SavedState) {
    let path = crate::paths::state_file();
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(t) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(path, t);
    }
}

/// Read config.toml, creating it with defaults on first run.
pub fn load_config() -> (Config, Vec<String>) {
    let path = crate::paths::config_file();
    match std::fs::read_to_string(&path) {
        Ok(text) => Config::parse(&text),
        Err(_) => {
            if let Some(d) = path.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(&path, DEFAULT_TOML);
            (Config::default(), Vec::new())
        }
    }
}

/// Change values in config.toml in place, keeping the user's comments.
fn edit_config(edits: &[(&str, &str, String)]) {
    let path = crate::paths::config_file();
    let mut text = std::fs::read_to_string(&path).unwrap_or_else(|_| DEFAULT_TOML.to_string());
    for (table, key, value) in edits {
        text = set_toml_value(&text, table, key, value);
    }
    if let Err(e) = std::fs::write(&path, text) {
        log_warn!("config: cannot write {}: {e}", path.display());
    }
}

struct Press {
    /// None: pressed on the background, which only drags.
    action: Option<Action>,
    /// Screen pixels where the button went down (the scene can change while dragging).
    screen: POINT,
    /// Pressed on the orb, its tag or a slip's background: a drag moves the island.
    draggable: bool,
}

struct Workers {
    media: Option<MediaWorker>,
    audio: Option<AudioWorker>,
    audio_mixer: bool,
    stats: Option<StatsSampler>,
    captures: Option<Stopper>,
    downloads: Option<Stopper>,
    privacy: Option<Stopper>,
    _config: Option<Stopper>,
    clipboard: bool,
    net: Option<super::net::NetWorker>,
}

struct App {
    hwnd: HWND,
    model: Model,
    gfx: Gfx,
    renderer: Renderer,
    rx: Receiver<Event>,
    bus: Bus,
    applied: Config,
    scene: Option<Scene>,
    scene_rect: Rect,
    /// After flipping to a smaller slip with the pointer outside it: the
    /// earlier slip's area, which keeps counting as the island until the
    /// pointer comes back in or leaves it, so the flip doesn't close it.
    hold: Option<Rect>,
    bx: Rect,
    work: Rect,
    scale: f32,
    /// The display the island is on now.
    monitor: HMONITOR,
    /// The display it docks to (config); a fullscreen app there may move it.
    home: HMONITOR,
    /// Moved to this display while a fullscreen app covers the home one.
    displaced: Option<HMONITOR>,
    /// A fullscreen app (not an overlay) covers the home display.
    covered: bool,
    tracking: bool,
    press: Option<Press>,
    slider: Option<(SliderTarget, Rect)>,
    dragging: bool,
    focus: Option<usize>,
    wheel: i32,
    workers: Workers,
    tray: Option<Tray>,
    accepting: Arc<AtomicBool>,
    state: SavedState,
    last_fullscreen: Option<String>,
    /// The fullscreen app's process, to notice when it exits.
    last_fullscreen_pid: Option<u32>,
    /// The fullscreen app's window: it stays in front while the focus is on another display.
    last_fullscreen_hwnd: Option<HWND>,
    last_probe_ms: u64,
    taskbar_created: u32,
    reduce_motion: bool,
    hook: HWINEVENTHOOK,
    hotkey: bool,
    /// What the last tag said, to animate a change of content.
    last_tag: Option<String>,
    /// The notch's glance last drawn, to slide in the next one.
    last_glance: usize,
    /// First half of a character outside the Basic Multilingual Plane (emoji).
    high_surrogate: Option<u16>,
}

pub fn run(opts: RunOptions) -> i32 {
    super::log::init();
    match run_inner(opts) {
        Ok(code) => code,
        Err(e) => {
            log_warn!("fatal: {e}");
            unsafe {
                MessageBoxW(None, &HSTRING::from(format!("Ngambang could not start:\n{e}")), w!("Ngambang"), MB_ICONERROR);
            }
            1
        }
    }
}

fn run_inner(opts: RunOptions) -> Result<i32> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    let sid = pipe::user_sid().unwrap_or_else(|_| "user".into());
    let _mutex = unsafe { CreateMutexW(None, true, &HSTRING::from(format!("Local\\Ngambang-{sid}")))? };
    if unsafe { windows::Win32::Foundation::GetLastError() } == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
        return Ok(0);
    }
    if opts.verbose {
        log_info!("starting {}", ngambang_core::VERSION);
    }

    let hinstance = unsafe { GetModuleHandleW(None)? };
    let class = w!("NgambangIsland");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: hinstance.into(),
        lpszClassName: class,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        hIcon: app_icon(32),
        ..Default::default()
    };
    unsafe { RegisterClassExW(&wc) };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
            class,
            w!("Ngambang"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(hinstance.into()),
            None,
        )?
    };
    HOOK_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

    let app = App::new(hwnd)?;
    APP.with(|a| *a.borrow_mut() = Some(app));
    APP.with(|a| {
        if let Some(app) = a.borrow_mut().as_mut() {
            app.start();
        }
    });

    let mut msg = MSG::default();
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            // Tab, Enter and Esc inside the Settings window.
            if super::settings::pre_translate(&msg) {
                continue;
            }
            let settings = super::settings::SETTINGS_HWND.load(Ordering::SeqCst);
            if settings != 0 && IsDialogMessageW(HWND(settings as *mut _), &msg).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    APP.with(|a| {
        if let Some(mut app) = a.borrow_mut().take() {
            app.shutdown();
        }
    });
    Ok(0)
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let handled = APP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => guard.as_mut().and_then(|app| app.handle(msg, wp, lp)),
        // Re-entered (e.g. a message sent while we were handling another).
        Err(_) => None,
    });
    match handled {
        Some(r) => r,
        None => {
            if msg == WM_DESTROY {
                unsafe { PostQuitMessage(0) };
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }
}

unsafe extern "system" fn win_event(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    let h = HOOK_HWND.load(Ordering::SeqCst);
    if h != 0 {
        let _ = PostMessageW(Some(HWND(h as *mut _)), WM_APP_FOREGROUND, WPARAM(0), LPARAM(0));
    }
}

/// The app icon from the executable's resources, at `size` pixels.
pub(crate) fn app_icon(size: i32) -> HICON {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return HICON::default() };
        LoadImageW(Some(module.into()), windows::core::PCWSTR(std::ptr::without_provenance(1)), IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
            .map(|h| HICON(h.0))
            .unwrap_or_default()
    }
}

fn timer_index(id: TimerId) -> usize {
    TimerId::ALL.iter().position(|t| *t == id).unwrap_or(0)
}

fn loword(v: usize) -> u32 {
    (v & 0xFFFF) as u32
}

fn lparam_point(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

fn monitor_name(m: HMONITOR) -> Option<String> {
    let mut mi = MONITORINFOEXW {
        monitorInfo: MONITORINFO { cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32, ..Default::default() },
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(m, &mut mi.monitorInfo).as_bool().then(|| from_wide(&mi.szDevice)) }
}

fn is_primary(m: HMONITOR) -> bool {
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(m, &mut mi).as_bool() && (mi.dwFlags & MONITORINFOF_PRIMARY) != 0 }
}

fn pick_monitor(name: &str) -> HMONITOR {
    let primary = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    if name.eq_ignore_ascii_case("primary") || name.is_empty() {
        return primary;
    }
    all_monitors().into_iter().find(|m| monitor_name(*m).is_some_and(|n| n.eq_ignore_ascii_case(name))).unwrap_or(primary)
}

fn all_monitors() -> Vec<HMONITOR> {
    unsafe extern "system" fn collect(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows::core::BOOL {
        let v = &mut *(data.0 as *mut Vec<HMONITOR>);
        v.push(m);
        true.into()
    }
    let mut all: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut all as *mut _ as isize));
    }
    all
}

/// Another display to wait on while `home` shows a fullscreen app: the
/// primary one, or else the first other.
fn other_monitor(home: HMONITOR) -> Option<HMONITOR> {
    let others: Vec<HMONITOR> = all_monitors().into_iter().filter(|m| *m != home).collect();
    others.iter().copied().find(|m| is_primary(*m)).or_else(|| others.first().copied())
}

/// (scale, work area in DIPs) of a monitor.
fn monitor_metrics(m: HMONITOR) -> (f32, Rect) {
    let (mut dx, mut dy) = (96u32, 96u32);
    unsafe {
        let _ = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    }
    let scale = (dx as f32 / 96.0).max(0.5);
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(m, &mut mi);
    }
    let r = mi.rcWork;
    (scale, Rect::new(r.left as f32 / scale, r.top as f32 / scale, (r.right - r.left) as f32 / scale, (r.bottom - r.top) as f32 / scale))
}

impl App {
    fn new(hwnd: HWND) -> Result<App> {
        let (config, warnings) = load_config();
        let gfx = Gfx::new()?;
        let renderer = Renderer::new(hwnd, &gfx)?;
        let (tx, rx) = channel();
        let bus = Bus::new(tx, hwnd);
        let seed = now_ms() ^ (unix_now() as u64).rotate_left(17) ^ (std::process::id() as u64) << 32;
        let mut model = Model::new(config.clone(), seed, now_ms());
        let state = load_state();
        model.playlog = state.playlog.clone();
        for w in warnings {
            bus.send(Event::Notice(w));
        }
        Ok(App {
            hwnd,
            model,
            gfx,
            renderer,
            rx,
            bus,
            applied: config,
            scene: None,
            scene_rect: Rect::default(),
            hold: None,
            bx: Rect::default(),
            work: Rect::default(),
            scale: 1.0,
            monitor: HMONITOR::default(),
            home: HMONITOR::default(),
            displaced: None,
            covered: false,
            tracking: false,
            press: None,
            slider: None,
            dragging: false,
            focus: None,
            wheel: 0,
            workers: Workers {
                media: None,
                audio: None,
                audio_mixer: false,
                stats: None,
                captures: None,
                downloads: None,
                privacy: None,
                _config: None,
                clipboard: false,
                net: None,
            },
            tray: None,
            accepting: Arc::new(AtomicBool::new(true)),
            state,
            last_fullscreen: None,
            last_fullscreen_pid: None,
            last_fullscreen_hwnd: None,
            last_probe_ms: 0,
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            reduce_motion: system::reduce_motion(),
            hook: HWINEVENTHOOK::default(),
            hotkey: false,
            last_tag: None,
            last_glance: 0,
            high_surrogate: None,
        })
    }

    fn start(&mut self) {
        low_memory_priority();
        // Restore a power plan left behind by a crash during a game.
        if let Some(g) = self.state.power_restore.take().and_then(|s| system::guid_from_str(&s)) {
            system::set_power_plan(&g);
            save_state(&self.state);
        }
        match pipe::default_pipe_name().and_then(|name| pipe::serve(name, self.bus.clone(), self.accepting.clone())) {
            Ok(()) => {}
            Err(e) => log_warn!("pipe: {e}"),
        }
        match Tray::new(self.hwnd) {
            Ok(t) => self.tray = Some(t),
            Err(e) => log_warn!("tray: {e}"),
        }
        unsafe {
            let mut abd = APPBARDATA {
                cbSize: std::mem::size_of::<APPBARDATA>() as u32,
                hWnd: self.hwnd,
                uCallbackMessage: WM_APP_APPBAR,
                ..Default::default()
            };
            SHAppBarMessage(ABM_NEW, &mut abd);
            self.hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(win_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            );
            SetTimer(Some(self.hwnd), TIMER_MIDNIGHT, ms_until_midnight() as u32, None);
        }
        // Link Indonesia's public holidays once, for configs made before it was a default.
        if !self.state.holidays_linked {
            self.state.holidays_linked = true;
            save_state(&self.state);
            let mut feeds = self.model.config.calendar.feeds.clone();
            if !feeds.iter().any(|f| f.url == ngambang_core::config::HOLIDAYS_URL) {
                feeds.push(ngambang_core::config::CalendarFeed::holidays());
                edit_config(&[("calendar", "feeds", ngambang_core::settings::feeds_literal(&feeds))]);
                self.model.config.calendar.feeds = feeds;
            }
        }
        match watch::config(self.bus.clone(), crate::paths::config_file()) {
            Ok(s) => self.workers._config = Some(s),
            Err(e) => log_warn!("config watch: {e}"),
        }
        self.apply_config(true);
        // Cached warnings go in before the clock is known, so they count as
        // already seen and don't raise tags again after a restart.
        if !self.state.alerts.is_empty() {
            self.dispatch(Event::Alerts(self.state.alerts.clone()));
        }
        if let Some(w) = self.state.weather.clone() {
            self.dispatch(Event::Weather(w));
        }
        if !self.state.todos.is_empty() {
            self.dispatch(Event::Todos(self.state.todos.clone()));
        }
        if !self.state.clip_pins.is_empty() {
            self.dispatch(Event::ClipPins(self.state.clip_pins.clone()));
        }
        if !self.state.notes.is_empty() {
            self.dispatch(Event::Notes(self.state.notes.clone()));
        }
        // Battery, power plans, the connection; files can be dropped on the island.
        self.dispatch(Event::Power(super::extras::power_state()));
        self.refresh_plans();
        self.watch_network();
        super::extras::accept_drops(self.hwnd);
        if let Some(a) = &self.workers.audio {
            a.query_mic();
        }
        if !self.state.calendar.is_empty() && !self.model.config.calendar.feeds.is_empty() {
            self.dispatch(Event::Calendar(self.state.calendar.clone()));
        }
        self.dispatch(Event::Day(today()));
        self.dispatch(clock_event());
        self.check_fullscreen();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            SetTimer(Some(self.hwnd), TIMER_TRIM, TRIM_AFTER_MS, None);
            SetTimer(Some(self.hwnd), TIMER_FULLSCREEN, FULLSCREEN_POLL_MS, None);
            SetTimer(Some(self.hwnd), TIMER_CLOCK, CLOCK_CHECK_MS, None);
        }
    }

    /// Re-sync the model's clock when it has drifted from Windows' clock.
    fn check_clock(&mut self) {
        let ev = clock_event();
        let Event::Clock { year, month, day, utc_offset_min, unix, .. } = &ev else { return };
        let synced = self.model.clock.is_some_and(|c| {
            (c.year, c.month, c.day, c.utc_offset_min) == (*year, *month, *day, *utc_offset_min)
                && self.model.unix_now().is_some_and(|u| (u - unix).abs() <= 20)
        });
        if !synced {
            self.dispatch(ev);
        }
    }

    fn shutdown(&mut self) {
        // Don't leave Do not disturb on behind us.
        if self.model.timer.is_running() && !self.model.on_break && self.model.config.timer.focus_assist {
            system::focus_session(false);
        }
        if let Some(g) = self.state.power_restore.take().and_then(|s| system::guid_from_str(&s)) {
            system::set_power_plan(&g);
        }
        self.state.playlog = self.model.playlog.clone();
        self.state.todos = self.model.todos.clone();
        save_state(&self.state);
        unsafe {
            let mut abd = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: self.hwnd, ..Default::default() };
            SHAppBarMessage(ABM_REMOVE, &mut abd);
            if !self.hook.is_invalid() {
                let _ = UnhookWinEvent(self.hook);
            }
            if self.workers.clipboard {
                let _ = RemoveClipboardFormatListener(self.hwnd);
            }
        }
        self.tray = None;
    }

    // ---- placement & rendering ------------------------------------------

    fn place_window(&mut self) {
        let dock = &self.model.config.dock;
        self.home = pick_monitor(&dock.monitor);
        self.monitor = self.displaced.unwrap_or(self.home);
        let (scale, work) = monitor_metrics(self.monitor);
        let bx = layout::window_box(dock.edge(), dock.offset, work);
        // Moving along the same edge keeps the drawn scene; only a new size or scale redraws.
        let resized = scale != self.scale || bx.w != self.bx.w || bx.h != self.bx.h || self.scene.is_none();
        self.scale = scale;
        self.work = work;
        self.bx = bx;
        let s = scale;
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                (self.bx.x * s).round() as i32,
                (self.bx.y * s).round() as i32,
                (self.bx.w * s).ceil() as i32,
                (self.bx.h * s).ceil() as i32,
                SWP_NOACTIVATE,
            );
        }
        if resized {
            let _ = self.renderer.set_scale(scale);
            let _ = self.renderer.set_box(self.bx.w, self.bx.h);
            self.renderer.reset_ring();
            self.scene = None;
        }
    }

    fn render(&mut self) {
        let edge = self.model.config.dock.edge();
        let notch = self.model.config.dock.style == Style::Notch;
        let mut scene = view::build(&self.model, edge, &self.gfx);
        if let Some(i) = self.focus.filter(|_| self.model.keyboard_open) {
            if let Some(h) = scene.hits.get(i) {
                scene.prims.push(Prim::RoundRect { rect: h.rect.inset(-2.0), radius: 10.0, fill: None, stroke: Some((theme::TEXT, 2.0)) });
            }
        }
        let screen = layout::place(edge, self.model.config.dock.offset, self.work, self.bx, scene.size, scene.flush);
        let local = layout::local(screen, self.bx);
        let now = now_ms();
        let reduce = self.reduce_motion || self.model.config.motion.reduce;
        // Hover and press feedback on the orb.
        let feedback = match scene.kind {
            // The notch hangs from the edge: growing would lift it off.
            SceneKind::Orb if notch => 1.0,
            SceneKind::Orb if self.press.is_some() && !self.dragging => 0.92,
            SceneKind::Orb if self.model.hover && !self.dragging => 1.1,
            _ => 1.0,
        };
        if self.scene.as_ref() == Some(&scene) && self.scene_rect == local {
            let _ = self.renderer.ring(scene.ring, local, now);
            let _ = self.renderer.feedback(feedback, reduce);
            return;
        }
        let tag_text = (scene.kind == SceneKind::Tag).then(|| self.model.current_tag().map(|t| t.text.clone())).flatten();
        let anim = match &self.scene {
            None => Anim::None,
            Some(_) if self.dragging => Anim::None,
            // A tag arriving on its own (not a hover summary) pops up.
            Some(prev) if prev.kind != scene.kind && scene.kind == SceneKind::Tag && !self.model.hover_tag => Anim::Pop,
            // Slips slide out of the docked edge and back into it.
            Some(prev) if !matches!(prev.kind, SceneKind::Slip(_)) && matches!(scene.kind, SceneKind::Slip(_)) => {
                Anim::Drawer { open: true, from: toward_edge(edge) }
            }
            Some(prev) if matches!(prev.kind, SceneKind::Slip(_)) && !matches!(scene.kind, SceneKind::Slip(_)) => {
                Anim::Drawer { open: false, from: toward_edge(edge) }
            }
            // Flipping slips: slide in the order of the tabs.
            Some(Scene { kind: SceneKind::Slip(a), .. }) if scene.kind != SceneKind::Slip(*a) => {
                let tabs = self.model.available_slips();
                let pos = |m: ModuleId| tabs.iter().position(|t| *t == m).unwrap_or(0) as i32;
                let to = if let SceneKind::Slip(b) = scene.kind { pos(b) } else { 0 };
                Anim::Flip { dir: if to >= pos(*a) { 1.0 } else { -1.0 } }
            }
            Some(prev) if prev.kind != scene.kind => Anim::Morph,
            // The notch turned to its next glance.
            Some(_) if notch && scene.kind == SceneKind::Orb && self.model.carousel != self.last_glance => Anim::Swap,
            Some(_) if scene.kind == SceneKind::Tag && tag_text.is_some() && tag_text != self.last_tag => Anim::Swap,
            Some(_) => Anim::None,
        };
        self.last_tag = tag_text;
        self.last_glance = self.model.carousel;
        if let Err(e) = self.renderer.show(&self.gfx, &scene, local, anim, reduce) {
            log_warn!("render: {e}; recreating device");
            if self.recreate_device().is_err() || self.renderer.show(&self.gfx, &scene, local, Anim::None, true).is_err() {
                return;
            }
        }
        let _ = self.renderer.feedback(feedback, reduce);
        // Back at rest: give memory back after a while.
        let at_rest = matches!(scene.kind, SceneKind::Orb | SceneKind::Seam | SceneKind::Hidden);
        if at_rest && self.scene.as_ref().is_some_and(|s| s.kind != scene.kind) {
            unsafe { SetTimer(Some(self.hwnd), TIMER_TRIM, TRIM_AFTER_MS, None) };
        }
        if self.renderer.has_outgoing() {
            unsafe { SetTimer(Some(self.hwnd), TIMER_CLEANUP, 450, None) };
        }
        let _ = self.renderer.ring(scene.ring, local, now);
        // Slip to slip: if the pointer was over the island and the new slip
        // no longer covers it, hold the old area.
        let flip = matches!((self.scene.as_ref().map(|s| s.kind), scene.kind), (Some(SceneKind::Slip(_)), SceneKind::Slip(_)));
        self.hold = match self.cursor_local() {
            Some((x, y)) if flip && !self.dragging && !local.contains(x, y) => {
                let before = self.hold.unwrap_or(self.scene_rect);
                before.contains(x, y).then(|| before.union(&local))
            }
            _ => None,
        };
        self.set_region(&scene, local);
        self.scene = Some(scene);
        self.scene_rect = local;
    }

    /// The cursor in window-local DIPs.
    fn cursor_local(&self) -> Option<(f32, f32)> {
        let mut pt = POINT::default();
        unsafe {
            GetCursorPos(&mut pt).ok()?;
            let _ = ScreenToClient(self.hwnd, &mut pt);
        }
        Some((pt.x as f32 / self.scale, pt.y as f32 / self.scale))
    }

    /// Back over the slip itself (or gone): drop the held area.
    fn release_hold(&mut self) {
        if self.hold.take().is_some() {
            if let Some(scene) = self.scene.take() {
                self.set_region(&scene, self.scene_rect);
                self.scene = Some(scene);
            }
        }
    }

    fn reduce(&self) -> bool {
        self.reduce_motion || self.model.config.motion.reduce
    }

    fn recreate_device(&mut self) -> Result<()> {
        self.gfx = Gfx::new()?;
        self.renderer.set_device(&self.gfx)?;
        self.scene = None;
        Ok(())
    }

    fn set_region(&self, scene: &Scene, local: Rect) {
        let s = self.scale;
        unsafe {
            let rgn = match scene.kind {
                SceneKind::Hidden => CreateRectRgn(0, 0, 0, 0),
                SceneKind::Seam => {
                    // A few extra pixels so the edge is easy to hit.
                    let pad = 6.0;
                    let r = match self.model.config.dock.edge() {
                        Edge::Left => Rect::new(local.x, local.y, local.w + pad, local.h),
                        Edge::Right => Rect::new(local.x - pad, local.y, local.w + pad, local.h),
                        Edge::Top => Rect::new(local.x, local.y, local.w, local.h + pad),
                        Edge::Bottom => Rect::new(local.x, local.y - pad, local.w, local.h + pad),
                    };
                    CreateRectRgn((r.x * s) as i32, (r.y * s) as i32, (r.right() * s).ceil() as i32, (r.bottom() * s).ceil() as i32)
                }
                SceneKind::Slip(_) if self.hold.is_some() => {
                    let r = self.hold.unwrap_or(local);
                    CreateRectRgn((r.x * s) as i32, (r.y * s) as i32, (r.right() * s).ceil() as i32, (r.bottom() * s).ceil() as i32)
                }
                _ => {
                    let d = (scene.clip_radius * 2.0 * s) as i32;
                    CreateRoundRectRgn(
                        (local.x * s) as i32,
                        (local.y * s) as i32,
                        (local.right() * s).ceil() as i32 + 1,
                        (local.bottom() * s).ceil() as i32 + 1,
                        d,
                        d,
                    )
                }
            };
            SetWindowRgn(self.hwnd, Some(rgn), true);
        }
    }

    // ---- model ----------------------------------------------------------

    fn dispatch(&mut self, ev: Event) {
        let fx = self.model.update(now_ms(), ev);
        for e in fx {
            self.exec(e);
        }
        if self.model.config != self.applied {
            self.apply_config(false);
        }
    }

    fn apply_config(&mut self, first: bool) {
        let old = std::mem::replace(&mut self.applied, self.model.config.clone());
        let cfg = self.applied.clone();
        if first || old.dock != cfg.dock {
            self.place_window();
            if !first {
                self.update_displaced();
            }
        }
        if first || old.dock.hotkey != cfg.dock.hotkey {
            unsafe {
                if self.hotkey {
                    let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_ID);
                }
                self.hotkey = false;
                if let Some(hk) = Hotkey::parse(&cfg.dock.hotkey) {
                    let mods = HOT_KEY_MODIFIERS(hk.modifiers) | MOD_NOREPEAT;
                    if RegisterHotKey(Some(self.hwnd), HOTKEY_ID, mods, hk.vk).is_ok() {
                        self.hotkey = true;
                    } else {
                        self.bus.send(Event::Notice(format!("Hotkey {} is used by another app", cfg.dock.hotkey)));
                    }
                }
            }
        }
        if first || old.privacy.hide_from_capture != cfg.privacy.hide_from_capture {
            let affinity = if cfg.privacy.hide_from_capture { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
            unsafe {
                let _ = SetWindowDisplayAffinity(self.hwnd, affinity);
            }
        }
        if let Some(s) = &self.workers.stats {
            if old.game.ping_host != cfg.game.ping_host {
                s.host(cfg.game.ping_host.clone());
            }
        }
        self.sync_workers(first || old.captures != cfg.captures);
        self.render();
    }

    /// Start or stop OS sources so only enabled modules cost anything.
    fn sync_workers(&mut self, captures_changed: bool) {
        let m = self.model.config.modules.clone();
        let on = |id: ModuleId| m.get(id);
        if on(ModuleId::Media) != self.workers.media.is_some() {
            self.workers.media = on(ModuleId::Media).then(|| MediaWorker::start(self.bus.clone()));
        }
        // Master volume (orb wheel) and mic mute need the audio worker too.
        let want_audio = on(ModuleId::Mixer) || on(ModuleId::Media) || on(ModuleId::Privacy);
        // Per-app sessions: the Mixer shows them, and the wheel turns the player's own volume.
        let sessions = on(ModuleId::Mixer) || on(ModuleId::Media);
        if want_audio != self.workers.audio.is_some() {
            self.workers.audio = want_audio.then(|| AudioWorker::start(self.bus.clone(), sessions));
            self.workers.audio_mixer = sessions;
        } else if let Some(a) = &self.workers.audio {
            if self.workers.audio_mixer != sessions {
                a.mixer(sessions);
                self.workers.audio_mixer = sessions;
            }
        }
        if on(ModuleId::Clipboard) != self.workers.clipboard {
            unsafe {
                if on(ModuleId::Clipboard) {
                    self.workers.clipboard = AddClipboardFormatListener(self.hwnd).is_ok();
                } else {
                    let _ = RemoveClipboardFormatListener(self.hwnd);
                    self.workers.clipboard = false;
                }
            }
        }
        if on(ModuleId::Captures) != self.workers.captures.is_some() || (captures_changed && on(ModuleId::Captures)) {
            self.workers.captures = None;
            self.workers.downloads = None;
            if on(ModuleId::Captures) {
                let dirs = watch::capture_dirs(&self.model.config.captures.extra_dirs);
                self.workers.captures = watch::captures(self.bus.clone(), dirs).map_err(|e| log_warn!("captures: {e}")).ok();
                self.workers.downloads = watch::downloads(self.bus.clone()).map_err(|e| log_warn!("downloads: {e}")).ok();
            }
        }
        self.load_launch_icons();
        if on(ModuleId::Privacy) != self.workers.privacy.is_some() {
            self.workers.privacy =
                if on(ModuleId::Privacy) { watch::privacy(self.bus.clone()).map_err(|e| log_warn!("privacy: {e}")).ok() } else { None };
        }
        self.accepting.store(on(ModuleId::Commands), Ordering::SeqCst);
        let c = &self.model.config;
        let net = super::net::NetConfig {
            weather: on(ModuleId::Weather),
            city: c.location.city.clone(),
            lat: c.location.latitude,
            lon: c.location.longitude,
            auto_update: c.update.auto_check,
            calendars: if on(ModuleId::Calendar) { c.calendar.feeds.iter().map(|f| f.url.clone()).collect() } else { Vec::new() },
            calendar_days: c.calendar.days,
            symbols: if on(ModuleId::Finance) { c.finance.watchlist.iter().map(|w| w.symbol.clone()).collect() } else { Vec::new() },
            names: c.finance.watchlist.iter().map(|w| w.name.clone()).collect(),
            price_alerts: c.finance.price_alerts,
            news: c.finance.news,
        };
        match &self.workers.net {
            Some(n) => n.configure(net),
            None => self.workers.net = Some(super::net::NetWorker::start(self.bus.clone(), net)),
        }
        if !on(ModuleId::Game) {
            self.workers.stats = None;
        }
    }

    fn stats(&mut self) -> &StatsSampler {
        let bus = self.bus.clone();
        let host = self.model.config.game.ping_host.clone();
        self.workers.stats.get_or_insert_with(|| StatsSampler::start(bus, host))
    }

    fn exec(&mut self, e: Effect) {
        match e {
            Effect::Render => self.render(),
            Effect::SetTimer(id, ms) => unsafe {
                SetTimer(Some(self.hwnd), TIMER_CORE + timer_index(id), ms.clamp(1, u32::MAX as u64) as u32, None);
            },
            Effect::KillTimer(id) => unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_CORE + timer_index(id));
            },
            Effect::StartSampler(s) | Effect::StopSampler(s) => {
                let start = matches!(e, Effect::StartSampler(_));
                match s {
                    Sampler::Second => unsafe {
                        if start {
                            SetTimer(Some(self.hwnd), TIMER_SECOND, 1000, None);
                        } else {
                            let _ = KillTimer(Some(self.hwnd), TIMER_SECOND);
                        }
                    },
                    Sampler::Ports => unsafe {
                        if start {
                            SetTimer(Some(self.hwnd), TIMER_PORTS, 3000, None);
                            self.refresh_ports();
                        } else {
                            let _ = KillTimer(Some(self.hwnd), TIMER_PORTS);
                        }
                    },
                    Sampler::Gpu => self.stats().gpu(start),
                    Sampler::Ping => self.stats().ping(start),
                    Sampler::Finance => {
                        if let Some(n) = &self.workers.net {
                            n.finance(start);
                        }
                    }
                    Sampler::Peaks => {
                        if let Some(a) = &self.workers.audio {
                            a.peaks(start);
                        }
                    }
                }
            }
            Effect::Media { session, cmd } => {
                if let Some(m) = &self.workers.media {
                    m.command(session, cmd);
                }
            }
            Effect::MediaKey => system::media_key(),
            Effect::SetMasterVolume(v) => self.workers.audio.iter().for_each(|a| a.set_master(v)),
            Effect::SetMasterMute(m) => self.workers.audio.iter().for_each(|a| a.set_master_mute(m)),
            Effect::SetAppVolume(pid, v) => self.workers.audio.iter().for_each(|a| a.set_app(pid, v)),
            Effect::SetAppMute(pid, m) => self.workers.audio.iter().for_each(|a| a.set_app_mute(pid, m)),
            Effect::MuteMic => self.workers.audio.iter().for_each(|a| a.mute_mic()),
            Effect::Open(target) => system::shell_open(&target),
            Effect::ShowInFolder(path) => system::show_in_folder(&path),
            Effect::CopyText(text) => {
                if let Err(e) = clipboard::write_text(self.hwnd, &text) {
                    log_warn!("clipboard: {e}");
                }
            }
            Effect::CopyFile(path) => {
                if let Err(e) = clipboard::write_file(self.hwnd, &path) {
                    log_warn!("clipboard: {e}");
                }
            }
            Effect::Kill(pid) => ports::kill(pid),
            Effect::FocusWindow(h) => unsafe {
                let target = HWND(h as usize as *mut _);
                if IsWindow(Some(target)).as_bool() {
                    if IsIconic(target).as_bool() {
                        let _ = ShowWindow(target, SW_RESTORE);
                    }
                    let _ = SetForegroundWindow(target);
                }
            },
            Effect::RefreshPorts => self.refresh_ports(),
            Effect::GameStarted(_) => {
                if let Some(plan) = system::guid_from_str(&self.model.config.game.power_plan) {
                    if self.state.power_restore.is_none() {
                        if let Some(cur) = system::active_power_plan() {
                            if cur != plan && system::set_power_plan(&plan) {
                                self.state.power_restore = Some(system::guid_to_string(&cur));
                                save_state(&self.state);
                            }
                        }
                    }
                }
                self.assert_topmost();
            }
            Effect::GameEnded => {
                if let Some(g) = self.state.power_restore.take().and_then(|s| system::guid_from_str(&s)) {
                    system::set_power_plan(&g);
                }
                self.state.playlog = self.model.playlog.clone();
                save_state(&self.state);
            }
            Effect::SavePlayLog => {
                self.state.playlog = self.model.playlog.clone();
                save_state(&self.state);
            }
            Effect::SaveModule(m, on) => edit_config(&[("modules", m.key(), on.to_string())]),
            Effect::InstallUpdate => match super::net::install_pending() {
                Ok(()) => unsafe {
                    let _ = DestroyWindow(self.hwnd);
                },
                Err(e) => {
                    self.dispatch(Event::Notice(e));
                    system::shell_open(ngambang_core::update::RELEASES_PAGE);
                }
            },
            Effect::RefreshWeather => {
                if let Some(n) = &self.workers.net {
                    n.refresh();
                }
            }
            Effect::OpenSettings(m) => super::settings::open(self.hwnd, &self.model.config, super::settings::page_for(m)),
            Effect::SaveTodos => {
                self.state.todos = self.model.todos.clone();
                save_state(&self.state);
            }
            Effect::SaveValue(table, key, value) => edit_config(&[(table, key, value)]),
            Effect::FocusSession(on) => system::focus_session(on),
            Effect::PasteText(text, paste) => {
                if let Err(e) = clipboard::write_text(self.hwnd, &text) {
                    log_warn!("clipboard: {e}");
                } else if paste {
                    super::extras::send_paste();
                }
            }
            Effect::SaveClipPins => {
                self.state.clip_pins = self.model.clip_history.iter().filter(|e| e.pinned).cloned().collect();
                save_state(&self.state);
            }
            Effect::SaveNotes => {
                self.state.notes = self.model.notes.clone();
                save_state(&self.state);
            }
            Effect::SetPowerPlan(guid) => {
                if let Some(g) = system::guid_from_str(&guid) {
                    system::set_power_plan(&g);
                }
            }
            Effect::MicToggle => self.workers.audio.iter().for_each(|a| a.toggle_mic()),
            Effect::TakeFocus => unsafe {
                let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE.0 as isize));
                let _ = SetForegroundWindow(self.hwnd);
                let _ = SetFocus(Some(self.hwnd));
                self.focus = None;
            },
            Effect::ReleaseFocus => unsafe {
                let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE.0 as isize);
                self.focus = None;
            },
        }
    }

    /// Power plans and the active one, for the Battery slip.
    fn refresh_plans(&mut self) {
        let active = system::active_power_plan().map(|g| system::guid_to_string(&g));
        self.dispatch(Event::PowerPlans(system::power_plans(), active));
    }

    /// Icons for pinned items that have none yet.
    fn load_launch_icons(&mut self) {
        let missing: Vec<String> =
            self.model.config.launch.items.iter().map(|i| i.path.clone()).filter(|p| !self.model.launch_icons.contains_key(p)).collect();
        for path in missing {
            if let Some(art) = super::extras::file_icon(&path) {
                let fx = self.model.update(now_ms(), Event::LaunchIcon(path, art));
                for e in fx {
                    if e != Effect::Render {
                        self.exec(e);
                    }
                }
            }
        }
    }

    /// Hear about network changes; each is checked on a worker thread.
    fn watch_network(&mut self) {
        use windows::Win32::NetworkManagement::IpHelper::{NotifyIpInterfaceChange, MIB_IPINTERFACE_ROW, MIB_NOTIFICATION_TYPE};
        use windows::Win32::Networking::WinSock::AF_UNSPEC;
        unsafe extern "system" fn changed(_: *const core::ffi::c_void, _: *const MIB_IPINTERFACE_ROW, _: MIB_NOTIFICATION_TYPE) {
            let h = HOOK_HWND.load(Ordering::SeqCst);
            if h != 0 {
                let _ = PostMessageW(Some(HWND(h as *mut _)), WM_APP_NET, WPARAM(0), LPARAM(0));
            }
        }
        let mut handle = windows::Win32::Foundation::HANDLE::default();
        unsafe {
            let _ = NotifyIpInterfaceChange(AF_UNSPEC, Some(changed), None, false, &mut handle);
        }
        self.check_network();
    }

    fn check_network(&self) {
        let bus = self.bus.clone();
        let _ = std::thread::Builder::new().name("ngambang-netstate".into()).spawn(move || {
            // Changes come in bursts while an adapter settles.
            std::thread::sleep(std::time::Duration::from_millis(1500));
            bus.send(Event::Network(super::extras::net_state()));
        });
    }

    fn refresh_ports(&self) {
        let bus = self.bus.clone();
        let _ = std::thread::Builder::new().name("ngambang-ports".into()).spawn(move || bus.send(Event::Ports(ports::scan())));
    }

    fn assert_topmost(&self) {
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    /// Move to another display while a fullscreen app covers the home one
    /// (when that is on and there is another display), or back home.
    fn update_displaced(&mut self) {
        let want = if self.covered && self.model.config.dock.move_in_fullscreen { other_monitor(self.home) } else { None };
        if want == self.displaced {
            return;
        }
        let was = self.displaced.is_some();
        self.displaced = want;
        self.place_window();
        if was != want.is_some() {
            self.dispatch(Event::Displaced(want.is_some()));
        } else {
            self.render();
        }
    }

    fn check_fullscreen(&mut self) {
        let app = system::fullscreen_app(self.hwnd, self.home, self.last_fullscreen_hwnd);
        self.last_fullscreen_hwnd = app.as_ref().map(|(_, _, w)| *w);
        let app = app.map(|(p, pid, _)| (p, pid));
        let path = app.as_ref().map(|(p, _)| p.clone());
        if path == self.last_fullscreen {
            // Same app: a quiet one may still turn out to be a game once it loads.
            if let Some((path, pid)) = app {
                let not_game_yet = self.model.fullscreen == ngambang_core::model::Fullscreen::Media && self.model.game.is_none();
                if not_game_yet && now_ms().saturating_sub(self.last_probe_ms) >= REPROBE_MS {
                    self.probe_gpu(path, pid);
                }
            }
            return;
        }
        self.last_fullscreen = path.clone();
        let previous_pid = std::mem::replace(&mut self.last_fullscreen_pid, app.as_ref().map(|(_, pid)| *pid));
        let Some((path, pid)) = app else {
            self.dispatch(Event::Fullscreen { active: false, exe: String::new(), signals: Default::default() });
            self.covered = false;
            self.update_displaced();
            // A game that closed ends its session as soon as its process is gone.
            if let (Some(pid), true) = (previous_pid, self.model.game.is_some()) {
                watch_exit(pid, self.bus.clone());
            }
            return;
        };
        // Decide right away from the path and the exclusive-mode flag ...
        let signals = ngambang_core::games::Signals { exclusive_d3d: system::exclusive_d3d(), gpu_3d_pct: None };
        // Overlays (Snipping Tool, Start) don't move the island; anything else
        // moves it away first, so it never sinks or drops its tags meanwhile.
        let kind = ngambang_core::games::classify(&path, signals, &self.model.config.game);
        self.covered = kind != ngambang_core::games::FullscreenKind::Ignore;
        self.update_displaced();
        self.dispatch(Event::Fullscreen { active: true, exe: path.clone(), signals });
        // ... then measure the app's own 3D load; a busy GPU makes it a game.
        self.probe_gpu(path, pid);
    }

    fn probe_gpu(&mut self, path: String, pid: u32) {
        self.last_probe_ms = now_ms();
        let exclusive = system::exclusive_d3d();
        let (bus, ours, monitor) = (self.bus.clone(), self.hwnd.0 as isize, self.home.0 as isize);
        let last = self.last_fullscreen_hwnd.map(|w| w.0 as isize);
        let _ = std::thread::Builder::new().name("ngambang-gpu-probe".into()).spawn(move || {
            let gpu = system::process_gpu_3d(pid);
            let last = last.map(|w| HWND(w as *mut _));
            let still = system::fullscreen_app(HWND(ours as *mut _), HMONITOR(monitor as *mut _), last).is_some_and(|(p, _, _)| p == path);
            if still && gpu.is_some_and(|g| g >= ngambang_core::games::GPU_GAME_PCT) {
                bus.send(Event::Fullscreen {
                    active: true,
                    exe: path,
                    signals: ngambang_core::games::Signals { exclusive_d3d: exclusive, gpu_3d_pct: gpu },
                });
            }
        });
    }

    /// After a click or drag released mouse capture, Windows stops tracking
    /// the pointer: start again, or report that it already left.
    fn after_capture(&mut self) {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let over = unsafe { WindowFromPoint(pt) } == self.hwnd;
        self.tracking = over;
        if over {
            let mut t = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = TrackMouseEvent(&mut t);
            }
        } else if self.model.hover {
            self.dispatch(Event::PointerLeave);
        }
    }

    // ---- input ----------------------------------------------------------

    /// Client pixel → scene-local DIPs.
    fn to_scene(&self, x: i32, y: i32) -> (f32, f32) {
        (x as f32 / self.scale - self.scene_rect.x, y as f32 / self.scale - self.scene_rect.y)
    }

    fn hit(&self, x: f32, y: f32) -> Option<HitKind> {
        self.scene.as_ref()?.hit_at(x, y).map(|h| h.kind.clone())
    }

    fn slider_value(&self, r: Rect, x: f32) -> f32 {
        ((x - r.x) / r.w.max(1.0)).clamp(0.0, 1.0)
    }

    /// Where the cursor would dock the island: edge, offset and monitor name.
    /// A notch only slides along the top edge.
    fn dock_at_cursor(notch: Option<Edge>) -> (Edge, f32, String) {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let mon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
        let (scale, work) = monitor_metrics(mon);
        let (mut edge, mut offset) = layout::nearest_edge(work, pt.x as f32 / scale, pt.y as f32 / scale);
        if let Some(keep) = notch {
            edge = keep;
            offset = (((pt.x as f32 / scale - work.x) / work.w.max(1.0)).clamp(0.0, 1.0) * 1000.0).round() / 1000.0;
        }
        let monitor = if is_primary(mon) { "primary".to_string() } else { monitor_name(mon).unwrap_or_else(|| "primary".into()) };
        (edge, offset, monitor)
    }

    /// Follow the cursor while dragging: the island moves along the nearest edge as you go.
    fn drag_to_cursor(&mut self) {
        let notch = (self.model.config.dock.style == Style::Notch).then_some(self.model.config.dock.edge);
        let (edge, offset, monitor) = Self::dock_at_cursor(notch);
        // Dropped somewhere new: that is home now, fullscreen app or not.
        if self.displaced.take().is_some() {
            self.covered = false;
            self.last_fullscreen = None;
            self.last_fullscreen_hwnd = None;
            self.dispatch(Event::Fullscreen { active: false, exe: String::new(), signals: Default::default() });
            self.dispatch(Event::Displaced(false));
        }
        let d = &self.model.config.dock;
        if d.edge == edge && d.offset == offset && d.monitor == monitor {
            return;
        }
        let edge_changed = d.edge != edge;
        let d = &mut self.model.config.dock;
        d.edge = edge;
        d.offset = offset;
        d.monitor = monitor;
        // Applied directly: no config reload or worker changes on every mouse move.
        self.applied.dock = self.model.config.dock.clone();
        if edge_changed {
            // The orb sits on the other axis now; draw it again there.
            self.scene = None;
        }
        self.place_window();
        self.render();
    }

    fn finish_drag(&mut self) {
        self.drag_to_cursor();
        let d = self.model.config.dock.clone();
        edit_config(&[
            ("dock", "edge", format!("\"{}\"", d.edge.name())),
            ("dock", "offset", format!("{}", d.offset)),
            ("dock", "monitor", format!("\"{}\"", d.monitor.replace('\\', "\\\\"))),
        ]);
        self.render();
    }

    fn activate_focused(&mut self) {
        let Some(i) = self.focus else { return };
        let Some(kind) = self.scene.as_ref().and_then(|s| s.hits.get(i)).map(|h| h.kind.clone()) else { return };
        if let HitKind::Click(a) = kind {
            self.dispatch(Event::Click(a));
        }
    }

    fn move_focus(&mut self, back: bool) {
        let n = self.scene.as_ref().map(|s| s.hits.len()).unwrap_or(0);
        if n == 0 {
            return;
        }
        self.focus = Some(match (self.focus, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        });
        self.scene = None;
        self.render();
    }

    fn nudge_focused_slider(&mut self, delta: f32) {
        let Some(i) = self.focus else { return };
        let Some(h) = self.scene.as_ref().and_then(|s| s.hits.get(i)).cloned() else { return };
        if let HitKind::Slider(t) = h.kind {
            let cur = match t {
                SliderTarget::Master => self.model.mixer.master.level,
                SliderTarget::App(pid) => self.model.mixer.apps.iter().find(|a| a.pid == pid).map(|a| a.volume.level).unwrap_or(0.0),
                SliderTarget::MediaSeek => self.model.front_media().and_then(|s| s.progress_at(now_ms())).unwrap_or(0.0),
            };
            self.dispatch(Event::Slider(t, (cur + delta).clamp(0.0, 1.0)));
        }
    }

    fn tray_menu(&mut self) {
        let Some(tray) = &self.tray else { return };
        // A menu only dismisses properly when its owner can be the foreground window.
        let ex = unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) };
        unsafe { SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE.0 as isize)) };
        let picked = tray.show_menu(&self.model.config, self.model.paused, system::autostart_enabled(), self.model.update.as_deref());
        unsafe {
            if !self.model.keyboard_open {
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE.0 as isize);
            }
            let _ = PostMessageW(Some(self.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        }
        let Some(cmd) = picked else { return };
        match cmd {
            c if (tray::CMD_MODULE..tray::CMD_MODULE + ModuleId::ALL.len() as u32).contains(&c) => {
                let m = ModuleId::ALL[(c - tray::CMD_MODULE) as usize];
                let on = !self.model.config.modules.get(m);
                self.dispatch(Event::SetModule(m, on));
            }
            c if (tray::CMD_EDGE..tray::CMD_EDGE + 4).contains(&c) => {
                let edge = Edge::ALL[(c - tray::CMD_EDGE) as usize];
                edit_config(&[("dock", "edge", format!("\"{}\"", edge.name())), ("dock", "style", "\"orb\"".into())]);
                let mut cfg = self.model.config.clone();
                cfg.dock.edge = edge;
                cfg.dock.style = Style::Orb;
                self.dispatch(Event::Config(cfg));
            }
            tray::CMD_NOTCH => {
                // A notch starts in the middle of the top edge.
                edit_config(&[("dock", "style", "\"notch\"".into()), ("dock", "offset", "0.5".into())]);
                let mut cfg = self.model.config.clone();
                cfg.dock.style = Style::Notch;
                cfg.dock.offset = 0.5;
                self.scene = None;
                self.dispatch(Event::Config(cfg));
            }
            tray::CMD_SETTINGS => super::settings::open(self.hwnd, &self.model.config, 0),
            tray::CMD_UPDATE => {
                if self.model.update.is_some() {
                    self.exec(Effect::InstallUpdate);
                } else if let Some(n) = &self.workers.net {
                    n.check_update();
                }
            }
            tray::CMD_PAUSE => {
                let p = !self.model.paused;
                self.dispatch(Event::Pause(p));
            }
            tray::CMD_AUTOSTART => system::set_autostart(!system::autostart_enabled()),
            tray::CMD_CONFIG => {
                let path = crate::paths::config_file();
                if !path.exists() {
                    let _ = std::fs::write(&path, DEFAULT_TOML);
                }
                open_text_file(&path);
            }
            tray::CMD_LOG => system::shell_open(&crate::paths::local_dir().to_string_lossy()),
            tray::CMD_QUIT => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
            _ => {}
        }
    }

    fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let zero = Some(LRESULT(0));
        match msg {
            WM_APP_EVENT => {
                let events: Vec<Event> = self.rx.try_iter().collect();
                for ev in events {
                    match &ev {
                        Event::Weather(w) => {
                            self.state.weather = Some(w.clone());
                            save_state(&self.state);
                        }
                        Event::Alerts(a) => {
                            self.state.alerts = a.clone();
                            save_state(&self.state);
                        }
                        Event::Calendar(c) => {
                            self.state.calendar = c.clone();
                            save_state(&self.state);
                        }
                        _ => {}
                    }
                    self.dispatch(ev);
                }
                zero
            }
            WM_TIMER => {
                let id = wp.0;
                if (TIMER_CORE..TIMER_CORE + TimerId::ALL.len()).contains(&id) {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), id);
                    }
                    self.dispatch(Event::Timer(TimerId::ALL[id - TIMER_CORE]));
                } else if id == TIMER_SECOND {
                    self.dispatch(Event::Sample(Sampler::Second));
                } else if id == TIMER_PORTS {
                    self.refresh_ports();
                } else if id == TIMER_CLEANUP {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), id);
                    }
                    let _ = self.renderer.drop_outgoing();
                } else if id == TIMER_FULLSCREEN {
                    self.check_fullscreen();
                } else if id == TIMER_CLOCK {
                    self.check_clock();
                } else if id == TIMER_TRIM {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), id);
                    }
                    if matches!(self.model.state, IslandState::Orb | IslandState::Seam | IslandState::Hidden) {
                        self.gfx.trim();
                        trim_memory();
                    }
                } else if id == TIMER_MIDNIGHT {
                    unsafe { SetTimer(Some(self.hwnd), TIMER_MIDNIGHT, ms_until_midnight() as u32, None) };
                    self.dispatch(Event::Day(today()));
                    self.dispatch(clock_event());
                }
                zero
            }
            WM_MOUSEMOVE => {
                let (px, py) = lparam_point(lp);
                if self.hold.is_some() && self.scene_rect.contains(px as f32 / self.scale, py as f32 / self.scale) {
                    self.release_hold();
                }
                let (x, _) = self.to_scene(px, py);
                if !self.tracking {
                    self.tracking = true;
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: self.hwnd,
                        dwHoverTime: 0,
                    };
                    unsafe {
                        let _ = TrackMouseEvent(&mut t);
                    }
                    self.dispatch(Event::PointerEnter);
                }
                if let Some((target, r)) = self.slider {
                    let v = self.slider_value(r, x);
                    self.dispatch(Event::Slider(target, v));
                } else if self.dragging {
                    self.drag_to_cursor();
                } else if let Some(p) = &self.press {
                    let mut pt = POINT::default();
                    unsafe {
                        let _ = GetCursorPos(&mut pt);
                    }
                    let moved = (((pt.x - p.screen.x).pow(2) + (pt.y - p.screen.y).pow(2)) as f32).sqrt();
                    if p.draggable && moved > DRAG_THRESHOLD * self.scale {
                        self.dragging = true;
                        let _ = self.renderer.press(None, true);
                        unsafe { SetCursor(LoadCursorW(None, IDC_SIZEALL).ok()) };
                        self.drag_to_cursor();
                    }
                }
                zero
            }
            WM_SETCURSOR => {
                if self.dragging {
                    return Some(LRESULT(1));
                }
                let mut pt = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut pt);
                    let _ = ScreenToClient(self.hwnd, &mut pt);
                }
                let (x, y) = self.to_scene(pt.x, pt.y);
                if self.hit(x, y).is_some() {
                    unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
                    return Some(LRESULT(1));
                }
                None
            }
            WM_LBUTTONDOWN => {
                let (px, py) = lparam_point(lp);
                let (x, y) = self.to_scene(px, py);
                match self.hit(x, y) {
                    Some(HitKind::Slider(t)) => {
                        let r = self.scene.as_ref().and_then(|s| s.hit_at(x, y)).map(|h| h.rect).unwrap_or_default();
                        self.slider = Some((t, r));
                        unsafe { SetCapture(self.hwnd) };
                        self.dispatch(Event::Slider(t, self.slider_value(r, x)));
                    }
                    hit => {
                        // The orb, a tag's body, a glance's items and a slip's background
                        // all drag the island; a press that does not move still clicks.
                        let action = match hit {
                            Some(HitKind::Click(a)) => Some(a),
                            _ => None,
                        };
                        let draggable = match &action {
                            None => matches!(self.model.state, IslandState::Slips | IslandState::Tag),
                            Some(Action::SelectSlip(_)) => self.model.state == IslandState::Tag,
                            Some(a) => *a == Action::ToggleSlips,
                        };
                        let mut screen = POINT::default();
                        unsafe {
                            let _ = GetCursorPos(&mut screen);
                        }
                        // A control (not the island itself) dims while held.
                        if action.as_ref().is_some_and(|a| *a != Action::ToggleSlips) {
                            if let Some(h) = self.scene.as_ref().and_then(|s| s.hit_at(x, y)) {
                                let r = h.rect.translate(self.scene_rect.x, self.scene_rect.y);
                                let _ = self.renderer.press(Some((r, (r.h / 2.0).min(12.0))), self.reduce());
                            }
                        }
                        self.press = Some(Press { action, screen, draggable });
                        unsafe { SetCapture(self.hwnd) };
                        self.render();
                    }
                }
                zero
            }
            WM_LBUTTONUP => {
                unsafe {
                    let _ = ReleaseCapture();
                }
                let (px, py) = lparam_point(lp);
                let (x, y) = self.to_scene(px, py);
                if self.dragging {
                    self.dragging = false;
                    self.press = None;
                    self.finish_drag();
                } else if self.slider.take().is_some() {
                } else if let Some(p) = self.press.take() {
                    let _ = self.renderer.press(None, self.reduce());
                    if let Some(HitKind::Click(a)) = self.hit(x, y) {
                        if Some(&a) == p.action.as_ref() {
                            // A ripple from where it was clicked.
                            if a != Action::ToggleSlips {
                                if let Some(h) = self.scene.as_ref().and_then(|s| s.hit_at(x, y)) {
                                    let r = h.rect.translate(self.scene_rect.x, self.scene_rect.y);
                                    let at = (x + self.scene_rect.x, y + self.scene_rect.y);
                                    let _ = self.renderer.ripple(r, (r.h / 2.0).min(12.0), at, self.reduce());
                                }
                            }
                            self.dispatch(Event::Click(a));
                        }
                    }
                }
                self.after_capture();
                self.render();
                zero
            }
            WM_MBUTTONUP => {
                self.dispatch(Event::MiddleClick);
                zero
            }
            WM_RBUTTONUP => {
                self.tray_menu();
                zero
            }
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as i32;
                let shift = (wp.0 & 0x0004) != 0; // MK_SHIFT
                self.wheel += delta;
                let notches = self.wheel / 120;
                self.wheel %= 120;
                if notches != 0 {
                    self.dispatch(Event::Wheel { notches, shift });
                }
                zero
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                self.release_hold();
                if !self.dragging && self.slider.is_none() {
                    self.dispatch(Event::PointerLeave);
                }
                zero
            }
            WM_MOUSEACTIVATE => {
                if self.model.keyboard_open {
                    None
                } else {
                    Some(LRESULT(MA_NOACTIVATE as isize))
                }
            }
            WM_HOTKEY => {
                if wp.0 as i32 == HOTKEY_ID {
                    self.dispatch(Event::Hotkey);
                }
                zero
            }
            WM_KEYDOWN if self.model.input.is_some() => {
                // Typing a task: editing keys go to the field.
                let ctrl = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
                match VIRTUAL_KEY(wp.0 as u16) {
                    VK_RETURN => self.dispatch(Event::Key(Key::Enter)),
                    VK_ESCAPE => self.dispatch(Event::Key(Key::Escape)),
                    VK_BACK => self.dispatch(Event::Key(Key::Backspace)),
                    VK_V if ctrl => {
                        if let Some(text) = clipboard::read_text(self.hwnd) {
                            self.dispatch(Event::Text(text.lines().next().unwrap_or("").to_string()));
                        }
                    }
                    _ => return None,
                }
                zero
            }
            WM_CHAR if self.model.input.is_some() => {
                // UTF-16 units; a surrogate pair arrives as two messages.
                let unit = wp.0 as u16;
                if (0xD800..0xDC00).contains(&unit) {
                    self.high_surrogate = Some(unit);
                } else {
                    let units: Vec<u16> = self.high_surrogate.take().into_iter().chain(std::iter::once(unit)).collect();
                    let text = String::from_utf16_lossy(&units);
                    if text.chars().all(|c| !c.is_control()) {
                        self.dispatch(Event::Text(text));
                    }
                }
                zero
            }
            WM_KEYDOWN => {
                let vk = VIRTUAL_KEY(wp.0 as u16);
                let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
                match vk {
                    VK_ESCAPE => self.dispatch(Event::Key(Key::Escape)),
                    VK_UP => {
                        self.focus = None;
                        self.dispatch(Event::Key(Key::Up));
                    }
                    VK_DOWN => {
                        self.focus = None;
                        self.dispatch(Event::Key(Key::Down));
                    }
                    VK_TAB => self.move_focus(shift),
                    VK_RETURN | VK_SPACE => self.activate_focused(),
                    VK_LEFT => self.nudge_focused_slider(-0.05),
                    VK_RIGHT => self.nudge_focused_slider(0.05),
                    _ => return None,
                }
                zero
            }
            WM_ACTIVATE => {
                if loword(wp.0) == WA_INACTIVE {
                    self.dispatch(Event::FocusLost);
                }
                None
            }
            WM_APP_SETTINGS => {
                let (cfg, warnings) = load_config();
                for w in warnings {
                    self.dispatch(Event::Notice(w));
                }
                self.dispatch(Event::Config(cfg));
                zero
            }
            WM_APP_TRAY => {
                match loword(lp.0 as usize) {
                    WM_RBUTTONUP | WM_CONTEXTMENU => self.tray_menu(),
                    WM_LBUTTONUP => self.dispatch(Event::Click(Action::ToggleSlips)),
                    _ => {}
                }
                zero
            }
            WM_APP_APPBAR => {
                if wp.0 as u32 == ABN_FULLSCREENAPP {
                    self.check_fullscreen();
                }
                zero
            }
            WM_APP_FOREGROUND => {
                // Another window took the foreground: the user clicked elsewhere.
                if unsafe { GetForegroundWindow() } != self.hwnd {
                    self.dispatch(Event::OutsideFocus);
                }
                self.check_fullscreen();
                if self.last_fullscreen.is_none() {
                    self.assert_topmost();
                }
                zero
            }
            WM_CLIPBOARDUPDATE => {
                if self.model.enabled(ModuleId::Clipboard) {
                    if let Some(text) = clipboard::read_text(self.hwnd) {
                        self.dispatch(Event::Clipboard { text, unix_now: unix_now() });
                    }
                }
                zero
            }
            WM_DROPFILES => {
                let paths = super::extras::dropped_paths(windows::Win32::UI::Shell::HDROP(wp.0 as *mut _));
                if !paths.is_empty() {
                    self.dispatch(Event::Dropped(paths));
                }
                zero
            }
            WM_APP_NET => {
                if self.model.enabled(ModuleId::Power) || self.model.enabled(ModuleId::Weather) {
                    self.check_network();
                }
                zero
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                if msg == WM_DISPLAYCHANGE && self.displaced.is_some() {
                    // Displays came or went: wait on one that still exists.
                    self.displaced = None;
                    self.place_window();
                    self.update_displaced();
                    if self.displaced.is_none() {
                        self.dispatch(Event::Displaced(false));
                    }
                }
                self.place_window();
                self.render();
                if msg == WM_DPICHANGED {
                    zero
                } else {
                    None
                }
            }
            WM_SETTINGCHANGE => {
                self.reduce_motion = system::reduce_motion();
                if wp.0 as u32 == SPI_SETWORKAREA.0 {
                    self.place_window();
                    self.render();
                }
                None
            }
            WM_TIMECHANGE => {
                self.dispatch(clock_event());
                None
            }
            WM_POWERBROADCAST => {
                // The battery or charger changed.
                if wp.0 == 0x0A || wp.0 == 0x8013 {
                    self.dispatch(Event::Power(super::extras::power_state()));
                }
                // Back from sleep: the clock jumped and the network may have changed.
                if wp.0 == 0x12 {
                    self.dispatch(clock_event());
                    if let Some(n) = &self.workers.net {
                        n.refresh();
                    }
                }
                None
            }
            WM_ENDSESSION => {
                self.shutdown();
                None
            }
            m if m == self.taskbar_created && m != 0 => {
                if let Some(t) = &self.tray {
                    t.add();
                }
                zero
            }
            _ => None,
        }
    }
}

/// Tell the model when a process exits, within the game grace period.
fn watch_exit(pid: u32, bus: Bus) {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    let _ = std::thread::Builder::new().name("ngambang-game-exit".into()).spawn(move || unsafe {
        let h = match OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            Ok(h) => h,
            // No such process: it has already exited.
            Err(e) if e.code() == windows::Win32::Foundation::ERROR_INVALID_PARAMETER.to_hresult() => {
                bus.send(Event::GameExited);
                return;
            }
            // Protected games may not let us open them: the grace period still ends the session.
            Err(_) => return,
        };
        let wait = ngambang_core::model::GAME_GRACE_MS as u32 + 5000;
        if WaitForSingleObject(h, wait) == WAIT_OBJECT_0 {
            bus.send(Event::GameExited);
        }
        let _ = CloseHandle(h);
    });
}

/// Hand unused memory back to Windows (what Task Manager shows drops to the
/// few megabytes the island really needs while idle).
fn trim_memory() {
    use windows::Win32::System::Memory::{GetProcessHeap, HeapCompact, HEAP_NONE};
    use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
    unsafe {
        // Freed blocks go back to Windows, then the pages leave the working set.
        if let Ok(heap) = GetProcessHeap() {
            let _ = HeapCompact(heap, HEAP_NONE);
        }
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

/// Ask Windows to take the island's pages first when memory is short: it
/// idles most of the time and a page fault later costs little.
fn low_memory_priority() {
    use windows::Win32::System::Threading::{
        GetCurrentProcess, ProcessMemoryPriority, SetProcessInformation, MEMORY_PRIORITY_INFORMATION, MEMORY_PRIORITY_LOW,
    };
    let info = MEMORY_PRIORITY_INFORMATION { MemoryPriority: MEMORY_PRIORITY_LOW };
    unsafe {
        let _ = SetProcessInformation(
            GetCurrentProcess(),
            ProcessMemoryPriority,
            &info as *const _ as *const _,
            std::mem::size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        );
    }
}

/// A unit step from the island towards the edge it is docked to.
fn toward_edge(edge: Edge) -> (f32, f32) {
    match edge {
        Edge::Left => (-1.0, 0.0),
        Edge::Right => (1.0, 0.0),
        Edge::Top => (0.0, -1.0),
        Edge::Bottom => (0.0, 1.0),
    }
}

/// Open a text file in its associated editor, or Notepad when .toml has none.
pub(crate) fn open_text_file(path: &std::path::Path) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    let p = HSTRING::from(path.as_os_str());
    unsafe {
        let r = ShellExecuteW(None, w!("open"), &p, None, None, SW_SHOWNORMAL);
        if (r.0 as isize) <= 32 {
            ShellExecuteW(None, w!("open"), w!("notepad.exe"), &p, None, SW_SHOWNORMAL);
        }
    }
}
