//! WASAPI: master volume, a per-app mixer for every process making sound,
//! microphone mute and peak meters. COM callbacks only mark state dirty; the
//! worker thread rebuilds the snapshot.

use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::Duration;

use ngambang_core::model::{AudioApp, Event, MixerSnapshot, Volume};
use windows::core::{implement, Interface, Ref, Result, BOOL, GUID, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl, IAudioMeterInformation,
};
use windows::Win32::Media::Audio::*;
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

use super::util::{process_display_name, process_path, Bus, OwnedHandle};

/// Marks volume changes we made so our own echo can be told apart.
const CONTEXT: GUID = GUID::from_u128(0x6e67_616d_6261_6e67_0001_0000_0000_0001);

enum Msg {
    Dirty,
    DeviceChanged,
    SetMaster(f32),
    SetMasterMute(bool),
    SetApp(u32, f32),
    SetAppMute(u32, bool),
    MuteMic,
    /// Flip the default microphone's mute and report it.
    ToggleMic,
    /// Report whether the default microphone is muted.
    QueryMic,
    Peaks(bool),
    Mixer(bool),
    Stop,
}

pub struct AudioWorker {
    tx: Sender<Msg>,
}

impl AudioWorker {
    /// `mixer`: enumerate per-app sessions (the Mixer module); master volume
    /// and mic mute work either way.
    pub fn start(bus: Bus, mixer: bool) -> AudioWorker {
        let (tx, rx) = channel();
        let tx2 = tx.clone();
        let _ = std::thread::Builder::new().name("ngambang-audio".into()).spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let mut st = match State::new(tx2, mixer) {
                Ok(s) => s,
                Err(e) => {
                    log_warn!("audio: unavailable: {e}");
                    return;
                }
            };
            st.publish(&bus);
            let mut peaks = false;
            // The meters as last sent, in twentieths: nothing is sent (and
            // nothing redrawn) until one visibly moves.
            let mut shown: Vec<(u32, u8)> = Vec::new();
            loop {
                let msg = if peaks {
                    match rx.recv_timeout(Duration::from_millis(50)) {
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
                let Some(msg) = msg else {
                    let now: Vec<(u32, f32)> =
                        st.peaks().into_iter().map(|(pid, p)| (pid, (p.clamp(0.0, 1.0) * 20.0).round() / 20.0)).collect();
                    let steps: Vec<(u32, u8)> = now.iter().map(|(pid, p)| (*pid, (p * 20.0) as u8)).collect();
                    if steps != shown {
                        shown = steps;
                        bus.send(Event::Peaks(now));
                    }
                    continue;
                };
                let r = match msg {
                    Msg::Stop => break,
                    Msg::Dirty => {
                        // Debounce: sessions often change in bursts.
                        while let Ok(Msg::Dirty) = rx.recv_timeout(Duration::from_millis(30)) {}
                        st.rebuild_sessions().map(|_| st.publish(&bus))
                    }
                    Msg::DeviceChanged => st.open_device().and_then(|_| st.rebuild_sessions()).map(|_| st.publish(&bus)),
                    Msg::SetMaster(v) => unsafe { st.endpoint.SetMasterVolumeLevelScalar(v, &CONTEXT) },
                    Msg::SetMasterMute(m) => unsafe { st.endpoint.SetMute(m, &CONTEXT) },
                    Msg::SetApp(pid, v) => st.for_pid(pid, |vol| unsafe { vol.SetMasterVolume(v, &CONTEXT) }),
                    Msg::SetAppMute(pid, m) => st.for_pid(pid, |vol| unsafe { vol.SetMute(m, &CONTEXT) }),
                    Msg::MuteMic => st.mute_mic(),
                    Msg::ToggleMic => st.mic_muted().and_then(|m| st.set_mic(!m)).map(|m| bus.send(Event::MicMuted(m))),
                    Msg::QueryMic => st.mic_muted().map(|m| bus.send(Event::MicMuted(m))),
                    Msg::Peaks(on) => {
                        peaks = on;
                        Ok(())
                    }
                    Msg::Mixer(on) => {
                        st.mixer = on;
                        st.rebuild_sessions().map(|_| st.publish(&bus))
                    }
                };
                if let Err(e) = r {
                    log_warn!("audio: {e}");
                }
            }
            st.unregister();
        });
        AudioWorker { tx }
    }

    pub fn set_master(&self, v: f32) {
        let _ = self.tx.send(Msg::SetMaster(v));
    }
    pub fn set_master_mute(&self, m: bool) {
        let _ = self.tx.send(Msg::SetMasterMute(m));
    }
    pub fn set_app(&self, pid: u32, v: f32) {
        let _ = self.tx.send(Msg::SetApp(pid, v));
    }
    pub fn set_app_mute(&self, pid: u32, m: bool) {
        let _ = self.tx.send(Msg::SetAppMute(pid, m));
    }
    pub fn toggle_mic(&self) {
        let _ = self.tx.send(Msg::ToggleMic);
    }

    pub fn query_mic(&self) {
        let _ = self.tx.send(Msg::QueryMic);
    }

    pub fn mute_mic(&self) {
        let _ = self.tx.send(Msg::MuteMic);
    }
    pub fn peaks(&self, on: bool) {
        let _ = self.tx.send(Msg::Peaks(on));
    }
    pub fn mixer(&self, on: bool) {
        let _ = self.tx.send(Msg::Mixer(on));
    }
}

impl Drop for AudioWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
    }
}

#[implement(IAudioSessionNotification, IAudioSessionEvents, IAudioEndpointVolumeCallback, IMMNotificationClient)]
struct Notify {
    tx: Sender<Msg>,
}

impl Notify {
    fn dirty(&self) -> Result<()> {
        let _ = self.tx.send(Msg::Dirty);
        Ok(())
    }
}

impl IAudioSessionNotification_Impl for Notify_Impl {
    fn OnSessionCreated(&self, _: Ref<IAudioSessionControl>) -> Result<()> {
        self.dirty()
    }
}

impl IAudioSessionEvents_Impl for Notify_Impl {
    fn OnDisplayNameChanged(&self, _: &PCWSTR, _: *const GUID) -> Result<()> {
        Ok(())
    }
    fn OnIconPathChanged(&self, _: &PCWSTR, _: *const GUID) -> Result<()> {
        Ok(())
    }
    fn OnSimpleVolumeChanged(&self, _: f32, _: BOOL, ctx: *const GUID) -> Result<()> {
        if !ctx.is_null() && unsafe { *ctx } == CONTEXT {
            return Ok(());
        }
        self.dirty()
    }
    fn OnChannelVolumeChanged(&self, _: u32, _: *const f32, _: u32, _: *const GUID) -> Result<()> {
        Ok(())
    }
    fn OnGroupingParamChanged(&self, _: *const GUID, _: *const GUID) -> Result<()> {
        Ok(())
    }
    fn OnStateChanged(&self, _: AudioSessionState) -> Result<()> {
        self.dirty()
    }
    fn OnSessionDisconnected(&self, _: AudioSessionDisconnectReason) -> Result<()> {
        self.dirty()
    }
}

impl IAudioEndpointVolumeCallback_Impl for Notify_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        if !data.is_null() && unsafe { (*data).guidEventContext } == CONTEXT {
            return Ok(());
        }
        self.dirty()
    }
}

impl IMMNotificationClient_Impl for Notify_Impl {
    fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> Result<()> {
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _: &PCWSTR) -> Result<()> {
        if flow == eRender && role == eMultimedia {
            let _ = self.tx.send(Msg::DeviceChanged);
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> Result<()> {
        Ok(())
    }
}

struct SessionRef {
    pid: u32,
    control: IAudioSessionControl2,
    volume: ISimpleAudioVolume,
    meter: Option<IAudioMeterInformation>,
}

struct State {
    enumerator: IMMDeviceEnumerator,
    endpoint: IAudioEndpointVolume,
    manager: Option<IAudioSessionManager2>,
    device_name: String,
    sessions: Vec<SessionRef>,
    notify_session: IAudioSessionNotification,
    notify_events: IAudioSessionEvents,
    notify_endpoint: IAudioEndpointVolumeCallback,
    /// Registered with the enumerator; kept so it outlives the registration.
    #[allow(dead_code)]
    notify_device: IMMNotificationClient,
    names: std::collections::HashMap<u32, (String, String, Option<String>)>,
    mixer: bool,
}

impl State {
    fn new(tx: Sender<Msg>, mixer: bool) -> Result<State> {
        let notify: windows::core::IUnknown = Notify { tx }.into();
        let enumerator: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        let notify_device: IMMNotificationClient = notify.cast()?;
        unsafe { enumerator.RegisterEndpointNotificationCallback(&notify_device)? };
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)? };
        let endpoint: IAudioEndpointVolume = unsafe { device.Activate(CLSCTX_ALL, None)? };
        let mut st = State {
            enumerator,
            endpoint,
            manager: None,
            device_name: String::new(),
            sessions: Vec::new(),
            notify_session: notify.cast()?,
            notify_events: notify.cast()?,
            notify_endpoint: notify.cast()?,
            notify_device,
            names: Default::default(),
            mixer,
        };
        st.attach(&device)?;
        st.rebuild_sessions()?;
        Ok(st)
    }

    fn attach(&mut self, device: &IMMDevice) -> Result<()> {
        unsafe {
            self.endpoint = device.Activate(CLSCTX_ALL, None)?;
            self.endpoint.RegisterControlChangeNotify(&self.notify_endpoint)?;
            let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
            // Enumerate once before registering, or new-session callbacks never fire.
            let _ = manager.GetSessionEnumerator()?;
            manager.RegisterSessionNotification(&self.notify_session)?;
            self.manager = Some(manager);
            self.device_name = device
                .OpenPropertyStore(STGM_READ)
                .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName))
                .and_then(|pv| {
                    let s = PropVariantToStringAlloc(&pv)?;
                    let out = s.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(s.0 as *const _));
                    Ok(out)
                })
                .unwrap_or_default();
        }
        Ok(())
    }

    fn open_device(&mut self) -> Result<()> {
        self.unregister();
        let device = unsafe { self.enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)? };
        self.attach(&device)
    }

    fn unregister(&mut self) {
        unsafe {
            for s in self.sessions.drain(..) {
                let _ = s.control.UnregisterAudioSessionNotification(&self.notify_events);
            }
            if let Some(m) = self.manager.take() {
                let _ = m.UnregisterSessionNotification(&self.notify_session);
            }
            let _ = self.endpoint.UnregisterControlChangeNotify(&self.notify_endpoint);
        }
    }

    fn rebuild_sessions(&mut self) -> Result<()> {
        unsafe {
            for s in self.sessions.drain(..) {
                let _ = s.control.UnregisterAudioSessionNotification(&self.notify_events);
            }
            if !self.mixer {
                return Ok(());
            }
            let Some(manager) = &self.manager else { return Ok(()) };
            let list = manager.GetSessionEnumerator()?;
            for i in 0..list.GetCount()? {
                let Ok(control) = list.GetSession(i) else { continue };
                let Ok(control) = control.cast::<IAudioSessionControl2>() else { continue };
                if control.GetState().map(|s| s == AudioSessionStateExpired).unwrap_or(true) {
                    continue;
                }
                if control.IsSystemSoundsSession() == windows::Win32::Foundation::S_OK {
                    continue;
                }
                let Ok(pid) = control.GetProcessId() else { continue };
                if pid == 0 {
                    continue;
                }
                let Ok(volume) = control.cast::<ISimpleAudioVolume>() else { continue };
                let meter = control.cast::<IAudioMeterInformation>().ok();
                let _ = control.RegisterAudioSessionNotification(&self.notify_events);
                self.sessions.push(SessionRef { pid, control, volume, meter });
            }
        }
        Ok(())
    }

    fn identity(&mut self, pid: u32) -> (String, String, Option<String>) {
        if let Some(n) = self.names.get(&pid) {
            return n.clone();
        }
        let path = process_path(pid).unwrap_or_default();
        let name = if path.is_empty() { format!("PID {pid}") } else { process_display_name(&path) };
        let exe = ngambang_core::format::exe_name(&path);
        let aumid = package_aumid(pid);
        let v = (name, exe, aumid);
        self.names.insert(pid, v.clone());
        v
    }

    fn snapshot(&mut self) -> MixerSnapshot {
        let master = unsafe {
            Volume {
                level: self.endpoint.GetMasterVolumeLevelScalar().unwrap_or(0.0),
                muted: self.endpoint.GetMute().map(|b| b.as_bool()).unwrap_or(false),
            }
        };
        let mut apps: Vec<AudioApp> = Vec::new();
        let pids: Vec<(u32, f32, bool)> = self
            .sessions
            .iter()
            .map(|s| unsafe {
                (s.pid, s.volume.GetMasterVolume().unwrap_or(1.0), s.volume.GetMute().map(|b| b.as_bool()).unwrap_or(false))
            })
            .collect();
        for (pid, level, muted) in pids {
            if apps.iter().any(|a| a.pid == pid) {
                continue;
            }
            let (name, exe, aumid) = self.identity(pid);
            apps.push(AudioApp { pid, name, exe, aumid, volume: Volume { level, muted }, peak: 0.0, has_controls: false });
        }
        MixerSnapshot { device: self.device_name.clone(), master, apps }
    }

    fn publish(&mut self, bus: &Bus) {
        bus.send(Event::Mixer(self.snapshot()));
    }

    fn peaks(&self) -> Vec<(u32, f32)> {
        let mut out: Vec<(u32, f32)> = Vec::new();
        for s in &self.sessions {
            let p = s.meter.as_ref().and_then(|m| unsafe { m.GetPeakValue().ok() }).unwrap_or(0.0);
            match out.iter_mut().find(|(pid, _)| *pid == s.pid) {
                Some(e) => e.1 = e.1.max(p),
                None => out.push((s.pid, p)),
            }
        }
        out
    }

    fn for_pid(&self, pid: u32, f: impl Fn(&ISimpleAudioVolume) -> Result<()>) -> Result<()> {
        for s in self.sessions.iter().filter(|s| s.pid == pid) {
            f(&s.volume)?;
        }
        Ok(())
    }

    fn mute_mic(&self) -> Result<()> {
        self.set_mic(true).map(|_| ())
    }

    /// Mute or unmute the default microphones (console and communications).
    fn set_mic(&self, muted: bool) -> Result<bool> {
        unsafe {
            for role in [eConsole, eCommunications] {
                if let Ok(dev) = self.enumerator.GetDefaultAudioEndpoint(eCapture, role) {
                    let ep: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None)?;
                    ep.SetMute(muted, &CONTEXT)?;
                }
            }
        }
        Ok(muted)
    }

    fn mic_muted(&self) -> Result<bool> {
        unsafe {
            let dev = self.enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications)?;
            let ep: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None)?;
            Ok(ep.GetMute()?.as_bool())
        }
    }
}

fn package_aumid(pid: u32) -> Option<String> {
    unsafe {
        let h = OwnedHandle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
        let mut len = 0u32;
        let _ = GetApplicationUserModelId(h.0, &mut len, None);
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize];
        if GetApplicationUserModelId(h.0, &mut len, Some(windows::core::PWSTR(buf.as_mut_ptr()))).is_err() {
            return None;
        }
        Some(super::util::from_wide(&buf))
    }
}
