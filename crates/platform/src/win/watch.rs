//! Event-driven watchers (no polling): new captures in folders, mic/camera
//! use from the registry, and config.toml changes. Each owns a stop event.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ngambang_core::config::Config;
use ngambang_core::model::{Capture, CaptureKind, Device, Event};
use windows::core::{w, Result, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HANDLE, WAIT_FAILED, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED, FILE_ACTION_RENAMED_NEW_NAME, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_INFORMATION,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects, INFINITE};
use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
use windows::Win32::UI::Shell::{FOLDERID_Downloads, FOLDERID_Pictures, FOLDERID_Videos, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

use super::util::{now_ms, Bus, OwnedHandle};

/// Signals a watcher thread to finish when dropped.
pub struct Stopper(Arc<OwnedHandle>);

impl Stopper {
    /// The stopper plus the handle the thread waits on (shared, so it stays open).
    fn new() -> Result<(Stopper, Arc<OwnedHandle>)> {
        let h = Arc::new(OwnedHandle(unsafe { CreateEventW(None, true, false, None)? }));
        Ok((Stopper(h.clone()), h))
    }
}

impl Drop for Stopper {
    fn drop(&mut self) {
        unsafe {
            let _ = SetEvent(self.0 .0);
        }
    }
}

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let p: PWSTR = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

pub fn capture_dirs(extra: &[String]) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = known_folder(&FOLDERID_Videos) {
        v.push(d.join("Captures"));
    }
    if let Some(d) = known_folder(&FOLDERID_Pictures) {
        v.push(d.join("Screenshots"));
    }
    v.extend(extra.iter().map(PathBuf::from));
    v.retain(|d| d.is_dir());
    v.dedup();
    v
}

struct DirWatch {
    dir: PathBuf,
    handle: OwnedHandle,
    event: OwnedHandle,
    overlapped: Box<OVERLAPPED>,
    buf: Box<[u32; 4096]>,
    filter: windows::Win32::Storage::FileSystem::FILE_NOTIFY_CHANGE,
}

impl DirWatch {
    fn open(dir: &Path, filter: windows::Win32::Storage::FileSystem::FILE_NOTIFY_CHANGE) -> Result<DirWatch> {
        unsafe {
            let handle = CreateFileW(
                &HSTRING::from(dir.as_os_str()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                None,
            )?;
            let event = CreateEventW(None, true, false, None)?;
            let mut w = DirWatch {
                dir: dir.to_path_buf(),
                handle: OwnedHandle(handle),
                event: OwnedHandle(event),
                overlapped: Box::new(OVERLAPPED { hEvent: event, ..Default::default() }),
                buf: Box::new([0; 4096]),
                filter,
            };
            w.arm()?;
            Ok(w)
        }
    }

    fn arm(&mut self) -> Result<()> {
        unsafe {
            *self.overlapped = OVERLAPPED { hEvent: self.event.0, ..Default::default() };
            ReadDirectoryChangesW(
                self.handle.0,
                self.buf.as_mut_ptr().cast(),
                (self.buf.len() * 4) as u32,
                false,
                self.filter,
                None,
                Some(&mut *self.overlapped),
                None,
            )
        }
    }

    /// Changed file names since the last call, then re-arm.
    fn take(&mut self) -> Vec<(u32, PathBuf)> {
        let mut out = Vec::new();
        unsafe {
            let mut n = 0u32;
            if GetOverlappedResult(self.handle.0, &*self.overlapped, &mut n, false).is_ok() && n > 0 {
                let base = self.buf.as_ptr() as *const u8;
                let mut off = 0usize;
                loop {
                    let info = &*(base.add(off) as *const FILE_NOTIFY_INFORMATION);
                    let name = std::slice::from_raw_parts(info.FileName.as_ptr(), info.FileNameLength as usize / 2);
                    out.push((info.Action.0, self.dir.join(String::from_utf16_lossy(name))));
                    if info.NextEntryOffset == 0 {
                        break;
                    }
                    off += info.NextEntryOffset as usize;
                }
            }
            windows::Win32::System::Threading::ResetEvent(self.event.0).ok();
        }
        if let Err(e) = self.arm() {
            log_warn!("watch: re-arm {} failed: {e}", self.dir.display());
        }
        out
    }
}

unsafe impl Send for DirWatch {}

/// New screenshots and clips.
pub fn captures(bus: Bus, dirs: Vec<PathBuf>) -> Result<Stopper> {
    let (stop, stop_handle) = Stopper::new()?;
    let mut watches: Vec<DirWatch> = dirs.iter().filter_map(|d| DirWatch::open(d, FILE_NOTIFY_CHANGE_FILE_NAME).ok()).collect();
    let _ = std::thread::Builder::new().name("ngambang-captures".into()).spawn(move || loop {
        let mut handles: Vec<HANDLE> = watches.iter().map(|w| w.event.0).collect();
        handles.push(stop_handle.0);
        let r = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
        let i = r.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
        if i >= watches.len() {
            return;
        }
        for (action, path) in watches[i].take() {
            if action != FILE_ACTION_ADDED.0 && action != FILE_ACTION_RENAMED_NEW_NAME.0 {
                continue;
            }
            let Some(kind) = CaptureKind::from_path(&path.to_string_lossy()) else { continue };
            let bus = bus.clone();
            // Wait until the writer is done before announcing it.
            let _ = std::thread::Builder::new().name("ngambang-capture-wait".into()).spawn(move || {
                use std::os::windows::fs::OpenOptionsExt;
                for _ in 0..20 {
                    if std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).is_ok() {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        bus.send(Event::Capture(Capture { path: path.to_string_lossy().into_owned(), name, kind, at_ms: now_ms() }));
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            });
        }
    });
    Ok(stop)
}

/// Browsers download into a temporary file next to the final one.
const PARTIAL: [&str; 5] = ["crdownload", "part", "partial", "download", "opdownload"];

/// The final name of a browser's temporary download file, if it is one.
fn partial_name(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    PARTIAL.contains(&ext.as_str()).then(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
}

/// Downloads in the Downloads folder: progress while a browser's temporary
/// file grows (checked every second only then), and the finished file.
pub fn downloads(bus: Bus) -> Result<Stopper> {
    let (stop, stop_handle) = Stopper::new()?;
    let Some(dir) = known_folder(&FOLDERID_Downloads).filter(|d| d.is_dir()) else { return Ok(stop) };
    let mut watch = DirWatch::open(&dir, FILE_NOTIFY_CHANGE_FILE_NAME)?;
    let _ = std::thread::Builder::new().name("ngambang-downloads".into()).spawn(move || {
        // Temporary file → the final name it will get.
        let mut tracked: std::collections::BTreeMap<PathBuf, String> = std::collections::BTreeMap::new();
        loop {
            let handles = [watch.event.0, stop_handle.0];
            let timeout = if tracked.is_empty() { INFINITE } else { 1000 };
            let r = unsafe { WaitForMultipleObjects(&handles, false, timeout) };
            if r.0 == WAIT_OBJECT_0.0 + 1 || r == WAIT_FAILED {
                return;
            }
            if r == WAIT_OBJECT_0 {
                for (action, path) in watch.take() {
                    if action == FILE_ACTION_ADDED.0 || action == FILE_ACTION_RENAMED_NEW_NAME.0 {
                        if let Some(name) = partial_name(&path) {
                            tracked.insert(path, name);
                        }
                    }
                }
            }
            tracked.retain(|tmp, name| match std::fs::metadata(tmp) {
                Ok(meta) => {
                    bus.send(Event::Download { name: name.clone(), bytes: meta.len(), done: None });
                    true
                }
                Err(_) => {
                    // Gone: finished when the final file is there, else cancelled.
                    let fin = dir.join(&*name);
                    let done = if fin.is_file() { fin.to_string_lossy().into_owned() } else { String::new() };
                    bus.send(Event::Download { name: name.clone(), bytes: 0, done: Some(done) });
                    false
                }
            });
        }
    });
    Ok(stop)
}

/// Reloads config.toml when it is saved.
pub fn config(bus: Bus, file: PathBuf) -> Result<Stopper> {
    let (stop, stop_handle) = Stopper::new()?;
    let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut watch = DirWatch::open(&dir, FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME)?;
    let _ = std::thread::Builder::new().name("ngambang-config".into()).spawn(move || {
        let mut last = String::new();
        loop {
            let handles = [watch.event.0, stop_handle.0];
            let r = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
            if r != WAIT_OBJECT_0 {
                return;
            }
            let touched = watch.take().iter().any(|(a, p)| {
                p == &file && (*a == FILE_ACTION_MODIFIED.0 || *a == FILE_ACTION_ADDED.0 || *a == FILE_ACTION_RENAMED_NEW_NAME.0)
            });
            if !touched {
                continue;
            }
            // Editors save in several steps.
            std::thread::sleep(Duration::from_millis(150));
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            if text == last {
                continue;
            }
            last = text.clone();
            let (cfg, warnings): (Config, Vec<String>) = Config::parse(&text);
            for w in warnings {
                bus.send(Event::Notice(w));
            }
            bus.send(Event::Config(cfg));
        }
    });
    Ok(stop)
}

const CONSENT: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore");

struct RegKey(HKEY);

impl Drop for RegKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

unsafe impl Send for RegKey {}

fn open_key(parent: HKEY, path: &HSTRING) -> Option<RegKey> {
    let mut k = HKEY::default();
    unsafe { RegOpenKeyExW(parent, path, None, KEY_READ | KEY_NOTIFY, &mut k).ok().ok()? };
    Some(RegKey(k))
}

fn subkeys(k: HKEY) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut name = [0u16; 512];
        let mut len = name.len() as u32;
        let r = unsafe { RegEnumKeyExW(k, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None) };
        if r.is_err() {
            break;
        }
        out.push(String::from_utf16_lossy(&name[..len as usize]));
    }
    out
}

fn qword(k: HKEY, sub: &str, value: PCWSTR) -> u64 {
    let mut v = 0u64;
    let mut size = 8u32;
    let r =
        unsafe { RegGetValueW(k, &HSTRING::from(sub), value, RRF_RT_REG_QWORD, None, Some(&mut v as *mut u64 as *mut _), Some(&mut size)) };
    if r.is_ok() {
        v
    } else {
        0
    }
}

fn in_use(k: HKEY, sub: &str) -> bool {
    qword(k, sub, w!("LastUsedTimeStart")) != 0 && qword(k, sub, w!("LastUsedTimeStop")) == 0
}

fn app_label(key: &str, packaged: bool) -> String {
    if packaged {
        ngambang_core::format::friendly_app_name(key)
    } else {
        let path = key.replace('#', "\\");
        super::util::process_display_name(&path)
    }
}

fn scan(device_key: HKEY) -> Vec<String> {
    let mut apps = Vec::new();
    for sub in subkeys(device_key) {
        if sub == "NonPackaged" {
            if let Some(np) = open_key(device_key, &HSTRING::from("NonPackaged")) {
                for s in subkeys(np.0) {
                    if in_use(np.0, &s) {
                        apps.push(app_label(&s, false));
                    }
                }
            }
        } else if in_use(device_key, &sub) {
            apps.push(app_label(&sub, true));
        }
    }
    apps.sort();
    apps.dedup();
    apps
}

/// Microphone and camera use, from the consent store Windows keeps up to date.
pub fn privacy(bus: Bus) -> Result<Stopper> {
    let (stop, stop_handle) = Stopper::new()?;
    let root = unsafe { CONSENT.to_string().unwrap_or_default() };
    // Windows records use per user, and for some apps machine-wide too.
    let mut sources = Vec::new();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        sources.push((Device::Microphone, open_key(hive, &HSTRING::from(format!("{root}\\microphone")))));
        sources.push((Device::Camera, open_key(hive, &HSTRING::from(format!("{root}\\webcam")))));
    }
    let _ = std::thread::Builder::new().name("ngambang-privacy".into()).spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let keys: Vec<(Device, RegKey, OwnedHandle)> = sources
            .into_iter()
            .filter_map(|(d, k)| Some((d, k?, OwnedHandle(unsafe { CreateEventW(None, false, false, None).ok()? }))))
            .collect();
        // The camera itself reports who streams from it; the registry is the fallback.
        let Ok(camera_event) = (unsafe { CreateEventW(None, false, false, None) }).map(OwnedHandle) else { return };
        let camera = super::sensors::CameraMonitor::start(camera_event.0);
        let name = |pid: u32| super::util::process_path(pid).map(|p| super::util::process_display_name(&p));
        let mut last: Vec<(Device, String)> = Vec::new();
        loop {
            for (_, k, ev) in &keys {
                unsafe {
                    let _ = RegNotifyChangeKeyValue(k.0, true, REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME, Some(ev.0), true);
                }
            }
            let registry =
                |device: Device| -> Vec<String> { keys.iter().filter(|(d, _, _)| *d == device).flat_map(|(_, k, _)| scan(k.0)).collect() };
            let direct_mic: Vec<String> = super::sensors::mic_pids().into_iter().filter_map(name).collect();
            let direct_cam: Vec<String> = camera.as_ref().map(|c| c.pids()).unwrap_or_default().into_iter().filter_map(name).collect();
            let mut now: Vec<(Device, String)> = Vec::new();
            for (device, direct) in [(Device::Microphone, direct_mic), (Device::Camera, direct_cam)] {
                let apps = if direct.is_empty() { registry(device) } else { direct };
                now.extend(apps.into_iter().map(|a| (device, a)));
            }
            now.sort();
            now.dedup();
            if now != last {
                last = now.clone();
                bus.send(Event::Privacy(now));
            }
            let mut handles: Vec<HANDLE> = keys.iter().map(|(_, _, e)| e.0).collect();
            handles.push(camera_event.0);
            handles.push(stop_handle.0);
            // Also look again every few seconds: microphones have no change
            // event of their own, and registry notifications can be missed.
            let r = unsafe { WaitForMultipleObjects(&handles, false, 3000) };
            let index = r.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            if r == WAIT_FAILED || index == handles.len() - 1 {
                return;
            }
            if index < handles.len() {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    });
    Ok(stop)
}

/// Every app Windows has recorded for the microphone and camera, with the
/// raw start/stop stamps, plus what the devices report right now (for
/// `ngambang doctor`).
pub fn privacy_report() -> Vec<String> {
    let root = unsafe { CONSENT.to_string().unwrap_or_default() };
    let mut out = Vec::new();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let describe = |pids: Vec<u32>| -> String {
        if pids.is_empty() {
            return "nobody".into();
        }
        pids.iter()
            .map(|p| {
                format!(
                    "{} (pid {p})",
                    super::util::process_path(*p).map(|x| super::util::process_display_name(&x)).unwrap_or_else(|| "?".into())
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    out.push(format!("Microphone now (capture sessions): {}", describe(super::sensors::mic_pids())));
    if let Some(ev) = unsafe { CreateEventW(None, false, false, None) }.ok().map(OwnedHandle) {
        match super::sensors::CameraMonitor::start(ev.0) {
            Some(monitor) => {
                // The first report arrives shortly after starting.
                std::thread::sleep(Duration::from_millis(1500));
                out.push(format!("Camera now (sensor monitor): {}", describe(monitor.pids())));
            }
            None => out.push("Camera now (sensor monitor): not available on this Windows".into()),
        }
    }
    for (hive, hive_name) in [(HKEY_CURRENT_USER, "HKCU"), (HKEY_LOCAL_MACHINE, "HKLM")] {
        for device in ["microphone", "webcam"] {
            let Some(k) = open_key(hive, &HSTRING::from(format!("{root}\\{device}"))) else {
                out.push(format!("{hive_name}\\{device}: not readable"));
                continue;
            };
            let mut line = |key: HKEY, name: &str| {
                let (start, stop) = (qword(key, name, w!("LastUsedTimeStart")), qword(key, name, w!("LastUsedTimeStop")));
                if start != 0 {
                    let state = if stop == 0 { "IN USE" } else { "idle" };
                    out.push(format!("{hive_name}\\{device}\\{name}: {state} (start {start}, stop {stop})"));
                }
            };
            for sub in subkeys(k.0) {
                if sub == "NonPackaged" {
                    if let Some(np) = open_key(k.0, &HSTRING::from("NonPackaged")) {
                        for s in subkeys(np.0) {
                            line(np.0, &s);
                        }
                    }
                } else {
                    line(k.0, &sub);
                }
            }
        }
    }
    out
}
