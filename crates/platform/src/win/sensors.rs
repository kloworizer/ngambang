//! Who is using the camera and the microphone right now, straight from the
//! devices rather than from the privacy registry (which some Windows builds
//! only update late): Media Foundation's sensor-activity monitor reports
//! every process streaming from a camera, and the audio capture sessions show
//! every process recording from a microphone.

use std::sync::{Arc, Mutex};

use windows::core::{implement, Interface, Ref, Result};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Media::Audio::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::System::Threading::SetEvent;

/// Processes streaming from any camera, kept up to date by Media Foundation.
pub struct CameraMonitor {
    monitor: IMFSensorActivityMonitor,
    pub pids: Arc<Mutex<Vec<u32>>>,
}

#[implement(IMFSensorActivitiesReportCallback)]
struct Reports {
    pids: Arc<Mutex<Vec<u32>>>,
    wake: isize,
}

impl IMFSensorActivitiesReportCallback_Impl for Reports_Impl {
    fn OnActivitiesReport(&self, report: Ref<IMFSensorActivitiesReport>) -> Result<()> {
        let report = report.ok()?;
        let mut pids = Vec::new();
        unsafe {
            for i in 0..report.GetCount()? {
                let device = report.GetActivityReport(i)?;
                for j in 0..device.GetProcessCount()? {
                    let p = device.GetProcessActivity(j)?;
                    if p.GetStreamingState()?.as_bool() {
                        pids.push(p.GetProcessId()?);
                    }
                }
            }
        }
        pids.sort();
        pids.dedup();
        let changed = {
            let mut cur = self.pids.lock().unwrap_or_else(|e| e.into_inner());
            let changed = *cur != pids;
            *cur = pids;
            changed
        };
        if changed {
            unsafe {
                let _ = SetEvent(HANDLE(self.wake as *mut _));
            }
        }
        Ok(())
    }
}

impl CameraMonitor {
    /// Start watching; `wake` is signalled whenever the set of processes changes.
    /// None on Windows versions without the monitor (before 10 version 1903).
    pub fn start(wake: HANDLE) -> Option<CameraMonitor> {
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_LITE).ok()?;
            let pids = Arc::new(Mutex::new(Vec::new()));
            let callback: IMFSensorActivitiesReportCallback = Reports { pids: pids.clone(), wake: wake.0 as isize }.into();
            let monitor = MFCreateSensorActivityMonitor(&callback).map_err(|e| log_warn!("camera monitor: {e}")).ok()?;
            monitor.Start().map_err(|e| log_warn!("camera monitor: {e}")).ok()?;
            Some(CameraMonitor { monitor, pids })
        }
    }

    pub fn pids(&self) -> Vec<u32> {
        self.pids.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Drop for CameraMonitor {
    fn drop(&mut self) {
        unsafe {
            let _ = self.monitor.Stop();
            let _ = MFShutdown();
        }
    }
}

/// Processes recording from any microphone now (active capture sessions).
/// The calling thread must have COM initialised.
pub fn mic_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    let scan = || -> Result<Vec<u32>> {
        let mut out = Vec::new();
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let devices = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;
            for d in 0..devices.GetCount()? {
                let device = devices.Item(d)?;
                let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
                let Ok(list) = manager.GetSessionEnumerator() else { continue };
                for i in 0..list.GetCount()? {
                    let Ok(control) = list.GetSession(i) else { continue };
                    let Ok(control) = control.cast::<IAudioSessionControl2>() else { continue };
                    if control.GetState().map(|s| s == AudioSessionStateActive).unwrap_or(false) {
                        // Pid 0 is the system's own session.
                        if let Ok(pid) = control.GetProcessId() {
                            if pid != 0 {
                                out.push(pid);
                            }
                        }
                    }
                }
            }
        }
        Ok(out)
    };
    if let Ok(found) = scan() {
        pids = found;
    }
    let own = std::process::id();
    pids.retain(|p| *p != own);
    pids.sort();
    pids.dedup();
    pids
}
