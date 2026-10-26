//! System helpers: fullscreen detection, game stats sampler (GPU, VRAM,
//! NVIDIA temperature, ping), power plans, media keys, shell actions and
//! autostart.

use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use ngambang_core::model::{Event, GpuStats};
use windows::core::{s, w, GUID, HSTRING, PCWSTR};
use windows::Win32::Foundation::{LocalFree, HLOCAL, HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL};
use windows::Win32::NetworkManagement::IpHelper::{
    GetBestRoute, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY, MIB_IPFORWARDROW,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Performance::*;
use windows::Win32::System::Power::{PowerGetActiveScheme, PowerSetActiveScheme};
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_MEDIA_PLAY_PAUSE,
};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, ShellExecuteW, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, IsZoomed, GWL_STYLE, SW_SHOWNORMAL,
    WS_CAPTION,
};

use super::util::{process_path, Bus};

/// The foreground window covers its whole monitor (and is not the desktop).
/// Returns the executable path and process id of the fullscreen app.
pub fn fullscreen_app(ours: HWND, island_monitor: windows::Win32::Graphics::Gdi::HMONITOR) -> Option<(String, u32)> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() || fg == ours {
            return None;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(fg, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n]);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return None;
        }
        // A maximized window with a title bar covers the screen only when the
        // taskbar hides itself; that is ordinary work, not a fullscreen app.
        let style = GetWindowLongW(fg, GWL_STYLE) as u32;
        if IsZoomed(fg).as_bool() && style & WS_CAPTION.0 == WS_CAPTION.0 {
            return None;
        }
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
        if mon.is_invalid() || mon != island_monitor {
            return None;
        }
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(fg, &mut r).ok()?;
        let covers =
            r.left <= mi.rcMonitor.left && r.top <= mi.rcMonitor.top && r.right >= mi.rcMonitor.right && r.bottom >= mi.rcMonitor.bottom;
        let busy = matches!(SHQueryUserNotificationState(), Ok(s) if s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_BUSY || s == QUNS_PRESENTATION_MODE);
        if !covers && !busy {
            return None;
        }
        if !covers {
            // "Busy" alone can mean another monitor; only trust it with a cover.
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        Some((process_path(pid).unwrap_or_else(|| format!("pid{pid}.exe")), pid))
    }
}

/// Windows reports an exclusive Direct3D fullscreen app (a classic game).
pub fn exclusive_d3d() -> bool {
    matches!(unsafe { SHQueryUserNotificationState() }, Ok(s) if s == QUNS_RUNNING_D3D_FULL_SCREEN)
}

/// The 3D engine load of one process, in percent, sampled over about a second.
/// Uses the same performance counters as Task Manager's GPU column.
pub fn process_gpu_3d(pid: u32) -> Option<f32> {
    unsafe {
        let mut query = PDH_HQUERY::default();
        if PdhOpenQueryW(None, 0, &mut query) != 0 {
            return None;
        }
        let path = HSTRING::from(format!("\\GPU Engine(pid_{pid}_*engtype_3D)\\Utilization Percentage"));
        let mut counter = PDH_HCOUNTER::default();
        let ok = PdhAddEnglishCounterW(query, &path, 0, &mut counter) == 0 && PdhCollectQueryData(query) == 0;
        let result = if ok {
            std::thread::sleep(Duration::from_millis(1000));
            if PdhCollectQueryData(query) == 0 {
                let mut size = 0u32;
                let mut count = 0u32;
                let _ = PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
                let mut buf = vec![0u8; size.max(1) as usize];
                if size > 0
                    && PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(buf.as_mut_ptr().cast())) == 0
                {
                    let items = std::slice::from_raw_parts(buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W, count as usize);
                    Some(items.iter().map(|i| i.FmtValue.Anonymous.doubleValue).filter(|v| v.is_finite()).sum::<f64>().clamp(0.0, 100.0)
                        as f32)
                } else {
                    Some(0.0)
                }
            } else {
                None
            }
        } else {
            None
        };
        PdhCloseQuery(query);
        result
    }
}

/// Simulate the play/pause media key.
pub fn media_key() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_MEDIA_PLAY_PAUSE, dwFlags: flags, ..Default::default() } },
    };
    unsafe {
        SendInput(&[key(Default::default()), key(KEYEVENTF_KEYUP)], std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn shell_open(target: &str) {
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(target), None, None, SW_SHOWNORMAL);
    }
}

pub fn show_in_folder(path: &str) {
    unsafe {
        ShellExecuteW(None, w!("open"), w!("explorer.exe"), &HSTRING::from(format!("/select,\"{path}\"")), None, SW_SHOWNORMAL);
    }
}

// ---- Power plans -----------------------------------------------------------

pub fn active_power_plan() -> Option<GUID> {
    unsafe {
        let mut p: *mut GUID = std::ptr::null_mut();
        if PowerGetActiveScheme(None, &mut p).is_err() || p.is_null() {
            return None;
        }
        let g = *p;
        let _ = LocalFree(Some(HLOCAL(p.cast())));
        Some(g)
    }
}

pub fn set_power_plan(g: &GUID) -> bool {
    unsafe { PowerSetActiveScheme(None, Some(g)).is_ok() }
}

pub fn guid_from_str(s: &str) -> Option<GUID> {
    let (d1, d2, d3, d4) = ngambang_core::format::parse_guid(s)?;
    Some(GUID { data1: d1, data2: d2, data3: d3, data4: d4 })
}

pub fn guid_to_string(g: &GUID) -> String {
    format!("{g:?}")
}

// ---- Autostart -------------------------------------------------------------

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Ngambang");

pub fn autostart_enabled() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE, RRF_RT_REG_SZ, None, None, None).is_ok() }
}

pub fn set_autostart(on: bool) {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE, &mut key).is_err() {
            return;
        }
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let v: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain(std::iter::once(0)).collect();
                let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
                let _ = RegSetValueExW(key, RUN_VALUE, None, REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(key, RUN_VALUE);
        }
        let _ = RegCloseKey(key);
    }
}

// ---- Game stats sampler ----------------------------------------------------

enum Msg {
    Gpu(bool),
    Ping(bool),
    Host(String),
    Stop,
}

/// Samples GPU, VRAM, temperature and ping while asked to; idle otherwise.
pub struct StatsSampler {
    tx: Sender<Msg>,
}

impl StatsSampler {
    pub fn start(bus: Bus, ping_host: String) -> StatsSampler {
        let (tx, rx) = channel();
        let _ = std::thread::Builder::new().name("ngambang-stats".into()).spawn(move || {
            let (mut gpu_on, mut ping_on) = (false, false);
            let mut host = ping_host;
            let mut pdh: Option<Pdh> = None;
            let mut nvml: Option<Option<Nvml>> = None;
            let mut last_ping = Instant::now() - Duration::from_secs(10);
            loop {
                let msg = if gpu_on || ping_on {
                    match rx.recv_timeout(Duration::from_millis(1000)) {
                        Ok(m) => Some(m),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                } else {
                    match rx.recv() {
                        Ok(m) => Some(m),
                        Err(_) => break,
                    }
                };
                match msg {
                    Some(Msg::Stop) => break,
                    Some(Msg::Gpu(on)) => gpu_on = on,
                    Some(Msg::Ping(on)) => ping_on = on,
                    Some(Msg::Host(h)) => host = h,
                    None => {}
                }
                let mut stats = GpuStats::default();
                if gpu_on {
                    if pdh.is_none() {
                        pdh = Pdh::open();
                    }
                    if let Some(p) = &pdh {
                        let (util, vram) = p.sample();
                        stats.gpu_pct = util;
                        stats.vram_mb = vram;
                    }
                    let n = nvml.get_or_insert_with(Nvml::load);
                    if let Some(n) = n {
                        stats.temp_c = n.temperature();
                    }
                }
                if ping_on && last_ping.elapsed() >= Duration::from_millis(1900) {
                    last_ping = Instant::now();
                    stats.ping_ms = ping(&host);
                }
                if stats != GpuStats::default() {
                    bus.send(Event::Stats(stats));
                }
            }
        });
        StatsSampler { tx }
    }

    pub fn gpu(&self, on: bool) {
        let _ = self.tx.send(Msg::Gpu(on));
    }
    pub fn ping(&self, on: bool) {
        let _ = self.tx.send(Msg::Ping(on));
    }
    pub fn host(&self, h: String) {
        let _ = self.tx.send(Msg::Host(h));
    }
}

impl Drop for StatsSampler {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
    }
}

struct Pdh {
    query: PDH_HQUERY,
    util: PDH_HCOUNTER,
    vram: PDH_HCOUNTER,
}

impl Pdh {
    fn open() -> Option<Pdh> {
        unsafe {
            let mut query = PDH_HQUERY::default();
            if PdhOpenQueryW(None, 0, &mut query) != 0 {
                return None;
            }
            let mut util = PDH_HCOUNTER::default();
            let mut vram = PDH_HCOUNTER::default();
            let a = PdhAddEnglishCounterW(query, w!("\\GPU Engine(*engtype_3D)\\Utilization Percentage"), 0, &mut util);
            let b = PdhAddEnglishCounterW(query, w!("\\GPU Adapter Memory(*)\\Dedicated Usage"), 0, &mut vram);
            if a != 0 && b != 0 {
                PdhCloseQuery(query);
                return None;
            }
            PdhCollectQueryData(query);
            Some(Pdh { query, util, vram })
        }
    }

    fn sum(&self, counter: PDH_HCOUNTER) -> Option<f64> {
        unsafe {
            let mut size = 0u32;
            let mut count = 0u32;
            let _ = PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
            if size == 0 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            if PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(buf.as_mut_ptr().cast())) != 0 {
                return None;
            }
            let items = std::slice::from_raw_parts(buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W, count as usize);
            Some(items.iter().map(|i| i.FmtValue.Anonymous.doubleValue).filter(|v| v.is_finite()).sum())
        }
    }

    fn sample(&self) -> (Option<f32>, Option<u64>) {
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return (None, None);
            }
        }
        let util = self.sum(self.util).map(|v| v.clamp(0.0, 100.0) as f32);
        let vram = self.sum(self.vram).map(|b| (b / (1024.0 * 1024.0)) as u64);
        (util, vram)
    }
}

impl Drop for Pdh {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

type NvmlInit = unsafe extern "C" fn() -> i32;
type NvmlHandle = unsafe extern "C" fn(u32, *mut *mut core::ffi::c_void) -> i32;
type NvmlTemp = unsafe extern "C" fn(*mut core::ffi::c_void, u32, *mut u32) -> i32;

/// NVIDIA temperature through nvml.dll, which ships with the driver.
struct Nvml {
    device: *mut core::ffi::c_void,
    temp: NvmlTemp,
}

unsafe impl Send for Nvml {}

impl Nvml {
    fn load() -> Option<Nvml> {
        unsafe {
            let lib = LoadLibraryW(w!("nvml.dll"))
                .or_else(|_| LoadLibraryW(w!("C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvml.dll")))
                .ok()?;
            let init: NvmlInit = std::mem::transmute(GetProcAddress(lib, s!("nvmlInit_v2"))?);
            let handle: NvmlHandle = std::mem::transmute(GetProcAddress(lib, s!("nvmlDeviceGetHandleByIndex_v2"))?);
            let temp: NvmlTemp = std::mem::transmute(GetProcAddress(lib, s!("nvmlDeviceGetTemperature"))?);
            if init() != 0 {
                return None;
            }
            let mut device = std::ptr::null_mut();
            if handle(0, &mut device) != 0 {
                return None;
            }
            Some(Nvml { device, temp })
        }
    }

    fn temperature(&self) -> Option<u32> {
        let mut t = 0u32;
        // 0 = NVML_TEMPERATURE_GPU
        (unsafe { (self.temp)(self.device, 0, &mut t) } == 0).then_some(t)
    }
}

fn ipv4_be(ip: std::net::Ipv4Addr) -> u32 {
    u32::from_ne_bytes(ip.octets())
}

fn resolve(host: &str) -> Option<u32> {
    if host == "gateway" {
        let mut row = MIB_IPFORWARDROW::default();
        let dest = ipv4_be(std::net::Ipv4Addr::new(1, 1, 1, 1));
        if unsafe { GetBestRoute(dest, None, &mut row) } == 0 && row.dwForwardNextHop != 0 {
            return Some(row.dwForwardNextHop);
        }
        return Some(dest);
    }
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        return Some(ipv4_be(ip));
    }
    use std::net::ToSocketAddrs;
    (host, 0).to_socket_addrs().ok()?.find_map(|a| match a.ip() {
        std::net::IpAddr::V4(v4) => Some(ipv4_be(v4)),
        _ => None,
    })
}

fn ping(host: &str) -> Option<u32> {
    let ip = resolve(host)?;
    unsafe {
        let h = IcmpCreateFile().ok()?;
        let data = [0x6eu8; 32];
        let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + data.len() + 8];
        let n = IcmpSendEcho(h, ip, data.as_ptr().cast(), data.len() as u16, None, reply.as_mut_ptr().cast(), reply.len() as u32, 1000);
        let _ = IcmpCloseHandle(h);
        if n == 0 {
            return None;
        }
        let r = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);
        (r.Status == 0).then_some(r.RoundTripTime)
    }
}

pub fn reduce_motion() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut on = windows::core::BOOL(1);
    unsafe {
        let _ =
            SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut on as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
    !on.as_bool()
}

// ---- Lists for the Settings window ----------------------------------------

/// Power plans as (GUID, friendly name).
pub fn power_plans() -> Vec<(String, String)> {
    use windows::Win32::System::Power::{PowerEnumerate, PowerReadFriendlyName, ACCESS_SCHEME};
    let mut out = Vec::new();
    for index in 0..32u32 {
        let mut guid = GUID::zeroed();
        let mut size = std::mem::size_of::<GUID>() as u32;
        let r = unsafe { PowerEnumerate(None, None, None, ACCESS_SCHEME, index, Some(&mut guid as *mut GUID as *mut u8), &mut size) };
        if r.is_err() {
            break;
        }
        let mut name = [0u16; 128];
        let mut bytes = (name.len() * 2) as u32;
        let ok = unsafe { PowerReadFriendlyName(None, Some(&guid), None, None, Some(name.as_mut_ptr() as *mut u8), &mut bytes) }.is_ok();
        let label = if ok { super::util::from_wide(&name) } else { guid_to_string(&guid) };
        out.push((guid_to_string(&guid), label));
    }
    out
}

/// Device names of the monitors that are not the primary one.
pub fn secondary_monitors() -> Vec<String> {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR, MONITORINFOEXW};
    use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
    unsafe extern "system" fn collect(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows::core::BOOL {
        let v = &mut *(data.0 as *mut Vec<String>);
        let mut mi = MONITORINFOEXW {
            monitorInfo: MONITORINFO { cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32, ..Default::default() },
            ..Default::default()
        };
        if GetMonitorInfoW(m, &mut mi.monitorInfo).as_bool() && mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY == 0 {
            v.push(super::util::from_wide(&mi.szDevice));
        }
        true.into()
    }
    let mut v: Vec<String> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut v as *mut _ as isize));
    }
    v
}
