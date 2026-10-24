//! The island window and the loop that connects OS input, workers, the core
//! model and the renderer.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use ngambang_core::config::{set_toml_value, Config, ModuleId, DEFAULT_TOML};
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
use super::comp::Renderer;
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
    action: Action,
    x: f32,
    y: f32,
    orb: bool,
}

struct Workers {
    media: Option<MediaWorker>,
    audio: Option<AudioWorker>,
    audio_mixer: bool,
    stats: Option<StatsSampler>,
    captures: Option<Stopper>,
    privacy: Option<Stopper>,
    _config: Option<Stopper>,
    clipboard: bool,
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
    bx: Rect,
    work: Rect,
    scale: f32,
    monitor: HMONITOR,
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
    taskbar_created: u32,
    reduce_motion: bool,
    hook: HWINEVENTHOOK,
    hotkey: bool,
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
    unsafe extern "system" fn collect(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows::core::BOOL {
        let v = &mut *(data.0 as *mut Vec<HMONITOR>);
        v.push(m);
        true.into()
    }
    let mut all: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut all as *mut _ as isize));
    }
    all.into_iter().find(|m| monitor_name(*m).is_some_and(|n| n.eq_ignore_ascii_case(name))).unwrap_or(primary)
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
            bx: Rect::default(),
            work: Rect::default(),
            scale: 1.0,
            monitor: HMONITOR::default(),
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
                privacy: None,
                _config: None,
                clipboard: false,
            },
            tray: None,
            accepting: Arc::new(AtomicBool::new(true)),
            state,
            last_fullscreen: None,
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            reduce_motion: system::reduce_motion(),
            hook: HWINEVENTHOOK::default(),
            hotkey: false,
        })
    }

    fn start(&mut self) {
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
        match watch::config(self.bus.clone(), crate::paths::config_file()) {
            Ok(s) => self.workers._config = Some(s),
            Err(e) => log_warn!("config watch: {e}"),
        }
        self.apply_config(true);
        self.dispatch(Event::Day(today()));
        self.check_fullscreen();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
    }

    fn shutdown(&mut self) {
        if let Some(g) = self.state.power_restore.take().and_then(|s| system::guid_from_str(&s)) {
            system::set_power_plan(&g);
        }
        self.state.playlog = self.model.playlog.clone();
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
        self.monitor = pick_monitor(&dock.monitor);
        let (scale, work) = monitor_metrics(self.monitor);
        self.scale = scale;
        self.work = work;
        self.bx = layout::window_box(dock.edge, dock.offset, work);
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
        let _ = self.renderer.set_scale(scale);
        let _ = self.renderer.set_box(self.bx.w, self.bx.h);
        self.renderer.reset_ring();
        self.scene = None;
    }

    fn render(&mut self) {
        let edge = self.model.config.dock.edge;
        let mut scene = view::build(&self.model, edge, &self.gfx);
        if let Some(i) = self.focus.filter(|_| self.model.keyboard_open) {
            if let Some(h) = scene.hits.get(i) {
                scene.prims.push(Prim::RoundRect { rect: h.rect.inset(-2.0), radius: 10.0, fill: None, stroke: Some((theme::TEXT, 2.0)) });
            }
        }
        let screen = layout::place(edge, self.model.config.dock.offset, self.work, self.bx, scene.size, scene.flush);
        let local = layout::local(screen, self.bx);
        let now = now_ms();
        if self.scene.as_ref() == Some(&scene) && self.scene_rect == local {
            let _ = self.renderer.ring(scene.ring, local, now);
            return;
        }
        let animate = self.scene.as_ref().is_some_and(|s| s.kind != scene.kind);
        let reduce = self.reduce_motion || self.model.config.motion.reduce;
        if let Err(e) = self.renderer.show(&self.gfx, &scene, local, animate, reduce) {
            log_warn!("render: {e}; recreating device");
            if self.recreate_device().is_err() || self.renderer.show(&self.gfx, &scene, local, false, true).is_err() {
                return;
            }
        }
        if self.renderer.has_outgoing() {
            unsafe { SetTimer(Some(self.hwnd), TIMER_CLEANUP, 450, None) };
        }
        let _ = self.renderer.ring(scene.ring, local, now);
        self.set_region(&scene, local);
        self.scene = Some(scene);
        self.scene_rect = local;
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
                    let r = match self.model.config.dock.edge {
                        Edge::Left => Rect::new(local.x, local.y, local.w + pad, local.h),
                        Edge::Right => Rect::new(local.x - pad, local.y, local.w + pad, local.h),
                        Edge::Top => Rect::new(local.x, local.y, local.w, local.h + pad),
                        Edge::Bottom => Rect::new(local.x, local.y - pad, local.w, local.h + pad),
                    };
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
        if want_audio != self.workers.audio.is_some() {
            self.workers.audio = want_audio.then(|| AudioWorker::start(self.bus.clone(), on(ModuleId::Mixer)));
            self.workers.audio_mixer = on(ModuleId::Mixer);
        } else if let Some(a) = &self.workers.audio {
            if self.workers.audio_mixer != on(ModuleId::Mixer) {
                a.mixer(on(ModuleId::Mixer));
                self.workers.audio_mixer = on(ModuleId::Mixer);
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
            if on(ModuleId::Captures) {
                let dirs = watch::capture_dirs(&self.model.config.captures.extra_dirs);
                self.workers.captures = watch::captures(self.bus.clone(), dirs).map_err(|e| log_warn!("captures: {e}")).ok();
            }
        }
        if on(ModuleId::Privacy) != self.workers.privacy.is_some() {
            self.workers.privacy =
                if on(ModuleId::Privacy) { watch::privacy(self.bus.clone()).map_err(|e| log_warn!("privacy: {e}")).ok() } else { None };
        }
        self.accepting.store(on(ModuleId::Commands), Ordering::SeqCst);
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

    fn refresh_ports(&self) {
        let bus = self.bus.clone();
        let _ = std::thread::Builder::new().name("ngambang-ports".into()).spawn(move || bus.send(Event::Ports(ports::scan())));
    }

    fn assert_topmost(&self) {
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    fn check_fullscreen(&mut self) {
        let app = system::fullscreen_app(self.hwnd, self.monitor);
        if app != self.last_fullscreen {
            self.last_fullscreen = app.clone();
            self.dispatch(Event::Fullscreen { active: app.is_some(), exe: app.unwrap_or_default() });
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

    fn finish_drag(&mut self) {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let mon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
        let (scale, work) = monitor_metrics(mon);
        let (edge, offset) = layout::nearest_edge(work, pt.x as f32 / scale, pt.y as f32 / scale);
        let monitor = if is_primary(mon) { "primary".to_string() } else { monitor_name(mon).unwrap_or_else(|| "primary".into()) };
        edit_config(&[
            ("dock", "edge", format!("\"{}\"", edge.name())),
            ("dock", "offset", format!("{offset}")),
            ("dock", "monitor", format!("\"{}\"", monitor.replace('\\', "\\\\"))),
        ]);
        let mut cfg = self.model.config.clone();
        cfg.dock.edge = edge;
        cfg.dock.offset = offset;
        cfg.dock.monitor = monitor;
        self.dispatch(Event::Config(cfg));
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
        let picked = tray.show_menu(&self.model.config, self.model.paused, system::autostart_enabled());
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
                edit_config(&[("dock", "edge", format!("\"{}\"", edge.name()))]);
                let mut cfg = self.model.config.clone();
                cfg.dock.edge = edge;
                self.dispatch(Event::Config(cfg));
            }
            tray::CMD_SETTINGS => super::settings::open(self.hwnd, &self.model.config),
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
                } else if id == TIMER_MIDNIGHT {
                    unsafe { SetTimer(Some(self.hwnd), TIMER_MIDNIGHT, ms_until_midnight() as u32, None) };
                    self.dispatch(Event::Day(today()));
                }
                zero
            }
            WM_MOUSEMOVE => {
                let (px, py) = lparam_point(lp);
                let (x, y) = self.to_scene(px, py);
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
                } else if let Some(p) = &self.press {
                    if p.orb && !self.dragging && ((x - p.x).powi(2) + (y - p.y).powi(2)).sqrt() > DRAG_THRESHOLD {
                        self.dragging = true;
                        unsafe { SetCursor(LoadCursorW(None, IDC_SIZEALL).ok()) };
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
                    Some(HitKind::Click(action)) => {
                        let orb = self.model.state == IslandState::Orb;
                        self.press = Some(Press { action, x, y, orb });
                        unsafe { SetCapture(self.hwnd) };
                    }
                    None => {}
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
                    if let Some(HitKind::Click(a)) = self.hit(x, y) {
                        if a == p.action {
                            self.dispatch(Event::Click(a));
                        }
                    }
                }
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
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
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
