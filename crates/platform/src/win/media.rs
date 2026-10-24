//! Now playing from every app that publishes Windows media controls (SMTC).
//! Runs on its own MTA thread; SMTC events only mark the snapshot dirty.

use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::Duration;

use ngambang_core::format::friendly_app_name;
use ngambang_core::model::{Event, MediaCmd, MediaSession};
use windows::core::{Result, HSTRING};
use windows::ApplicationModel::AppInfo;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

use super::util::{now_ms, Bus};

enum Msg {
    Dirty,
    Command(String, MediaCmd),
    Stop,
}

pub struct MediaWorker {
    tx: Sender<Msg>,
}

impl MediaWorker {
    pub fn start(bus: Bus) -> MediaWorker {
        let (tx, rx) = channel();
        let tx2 = tx.clone();
        let _ = std::thread::Builder::new().name("ngambang-media".into()).spawn(move || {
            unsafe {
                let _ = RoInitialize(RO_INIT_MULTITHREADED);
            }
            let mut state = match State::new(tx2) {
                Ok(s) => s,
                Err(e) => {
                    log_warn!("media: SMTC unavailable: {e}");
                    return;
                }
            };
            state.publish(&bus);
            loop {
                match rx.recv() {
                    Ok(Msg::Stop) | Err(_) => break,
                    Ok(Msg::Command(id, cmd)) => {
                        if let Err(e) = state.command(&id, cmd) {
                            log_warn!("media: command failed: {e}");
                        }
                    }
                    Ok(Msg::Dirty) => {
                        // Debounce bursts of SMTC events.
                        loop {
                            match rx.recv_timeout(Duration::from_millis(40)) {
                                Ok(Msg::Dirty) => continue,
                                Ok(Msg::Command(id, cmd)) => {
                                    let _ = state.command(&id, cmd);
                                }
                                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                                Err(RecvTimeoutError::Timeout) => break,
                            }
                        }
                        if let Err(e) = state.resubscribe() {
                            log_warn!("media: resubscribe failed: {e}");
                        }
                        state.publish(&bus);
                    }
                }
            }
            bus.send(Event::Media(Vec::new()));
        });
        MediaWorker { tx }
    }

    pub fn command(&self, session: String, cmd: MediaCmd) {
        let _ = self.tx.send(Msg::Command(session, cmd));
    }
}

impl Drop for MediaWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
    }
}

struct Subscribed {
    session: Session,
    tokens: [i64; 3],
}

struct State {
    manager: Manager,
    tx: Sender<Msg>,
    subs: Vec<Subscribed>,
    names: std::collections::HashMap<String, String>,
}

impl State {
    fn new(tx: Sender<Msg>) -> Result<State> {
        let manager = Manager::RequestAsync()?.join()?;
        let t = tx.clone();
        manager.SessionsChanged(&TypedEventHandler::new(move |_, _| {
            let _ = t.send(Msg::Dirty);
            Ok(())
        }))?;
        let t = tx.clone();
        manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
            let _ = t.send(Msg::Dirty);
            Ok(())
        }))?;
        let mut s = State { manager, tx, subs: Vec::new(), names: Default::default() };
        s.resubscribe()?;
        Ok(s)
    }

    fn resubscribe(&mut self) -> Result<()> {
        for s in self.subs.drain(..) {
            let _ = s.session.RemoveMediaPropertiesChanged(s.tokens[0]);
            let _ = s.session.RemovePlaybackInfoChanged(s.tokens[1]);
            let _ = s.session.RemoveTimelinePropertiesChanged(s.tokens[2]);
        }
        for session in self.manager.GetSessions()? {
            let a = session.MediaPropertiesChanged(&TypedEventHandler::new(dirty(&self.tx)))?;
            let b = session.PlaybackInfoChanged(&TypedEventHandler::new(dirty(&self.tx)))?;
            let c = session.TimelinePropertiesChanged(&TypedEventHandler::new(dirty(&self.tx)))?;
            self.subs.push(Subscribed { session, tokens: [a, b, c] });
        }
        Ok(())
    }

    fn app_name(&mut self, aumid: &str) -> String {
        if let Some(n) = self.names.get(aumid) {
            return n.clone();
        }
        let name = AppInfo::GetFromAppUserModelId(&HSTRING::from(aumid))
            .and_then(|i| i.DisplayInfo())
            .and_then(|d| d.DisplayName())
            .map(|n| n.to_string())
            .ok()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| friendly_app_name(aumid));
        self.names.insert(aumid.to_string(), name.clone());
        name
    }

    fn snapshot(&mut self) -> Vec<MediaSession> {
        let sessions: Vec<Session> = self.subs.iter().map(|s| s.session.clone()).collect();
        let now = now_ms();
        let filetime_now = unsafe {
            let ft = GetSystemTimeAsFileTime();
            ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64
        };
        let mut out = Vec::new();
        for (i, s) in sessions.iter().enumerate() {
            let read = |this: &mut State| -> Result<MediaSession> {
                let raw_id = s.SourceAppUserModelId()?.to_string();
                let props = s.TryGetMediaPropertiesAsync()?.join()?;
                let info = s.GetPlaybackInfo()?;
                let controls = info.Controls()?;
                let tl = s.GetTimelineProperties()?;
                let playing = info.PlaybackStatus()? == Status::Playing;
                let start = tl.StartTime()?.Duration;
                let end = tl.EndTime()?.Duration;
                let mut pos = tl.Position()?.Duration - start;
                if playing {
                    let delta = filetime_now - tl.LastUpdatedTime()?.UniversalTime;
                    if delta > 0 && delta < end - start {
                        pos += delta;
                    }
                }
                let duration_ms = ((end - start).max(0) / 10_000) as u64;
                let position_ms = ((pos.max(0)) / 10_000) as u64;
                // Several sessions can share an app id; keep ids unique.
                let id = if out_has(&out, &raw_id) { format!("{raw_id}#{i}") } else { raw_id.clone() };
                Ok(MediaSession {
                    app: this.app_name(&raw_id),
                    id,
                    title: props.Title()?.to_string(),
                    artist: props.Artist()?.to_string(),
                    playing,
                    position_ms: position_ms.min(duration_ms.max(position_ms)),
                    duration_ms,
                    at_ms: now,
                    can_play_pause: controls.IsPlayPauseToggleEnabled()? || controls.IsPlayEnabled()? || controls.IsPauseEnabled()?,
                    can_next: controls.IsNextEnabled()?,
                    can_prev: controls.IsPreviousEnabled()?,
                    can_seek: controls.IsPlaybackPositionEnabled()? && duration_ms > 0,
                })
            };
            match read(self) {
                Ok(m) => out.push(m),
                Err(e) => log_warn!("media: session {i} unreadable: {e}"),
            }
        }
        out
    }

    fn publish(&mut self, bus: &Bus) {
        bus.send(Event::Media(self.snapshot()));
    }

    fn command(&self, id: &str, cmd: MediaCmd) -> Result<()> {
        let (raw, index) = match id.split_once('#') {
            Some((r, i)) => (r, i.parse::<usize>().ok()),
            None => (id, None),
        };
        let found = self
            .subs
            .iter()
            .enumerate()
            .find(|(i, s)| s.session.SourceAppUserModelId().map(|a| a == raw).unwrap_or(false) && index.is_none_or(|want| want == *i));
        let Some((_, sub)) = found else { return Ok(()) };
        let s = &sub.session;
        match cmd {
            MediaCmd::PlayPause => s.TryTogglePlayPauseAsync()?.join()?,
            MediaCmd::Next => s.TrySkipNextAsync()?.join()?,
            MediaCmd::Prev => s.TrySkipPreviousAsync()?.join()?,
            MediaCmd::Seek(ms) => {
                let start = s.GetTimelineProperties()?.StartTime()?.Duration;
                s.TryChangePlaybackPositionAsync(start + ms as i64 * 10_000)?.join()?
            }
        };
        Ok(())
    }
}

fn out_has(out: &[MediaSession], id: &str) -> bool {
    out.iter().any(|m| m.id == id)
}

fn dirty<A: windows::core::RuntimeType + 'static>(
    tx: &Sender<Msg>,
) -> impl Fn(windows::core::Ref<Session>, windows::core::Ref<A>) -> Result<()> + Send + 'static {
    let tx = tx.clone();
    move |_, _| {
        let _ = tx.send(Msg::Dirty);
        Ok(())
    }
}
