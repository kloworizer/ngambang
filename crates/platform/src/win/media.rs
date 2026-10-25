//! Now playing from every app that publishes Windows media controls (SMTC).
//! Runs on its own MTA thread; SMTC events only mark the snapshot dirty.
//! Titles, artists and album art are read only when an app says they changed.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ngambang_core::format::friendly_app_name;
use ngambang_core::model::{Art, Event, MediaCmd, MediaSession};
use windows::core::{Interface, Result, HSTRING};
use windows::ApplicationModel::AppInfo;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::IRandomAccessStreamReference;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, IStream, CLSCTX_INPROC_SERVER};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::System::WinRT::{CreateStreamOverRandomAccessStream, RoInitialize, RO_INIT_MULTITHREADED};

use super::util::{now_ms, Bus};

/// Album art is decoded at this size (pixels, square).
const ART_PX: u32 = 120;
/// Apps often publish the title first and the artist and cover a moment
/// later: read the properties once more after this.
const RECHECK: Duration = Duration::from_millis(1200);

static ART_IDS: AtomicU64 = AtomicU64::new(1);

enum Msg {
    /// Play state or position changed.
    Dirty,
    /// Sessions came or went.
    Sessions,
    /// Title, artist or cover changed.
    Props,
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
            let mut recheck: Option<Instant> = None;
            loop {
                let msg = match recheck {
                    Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                        Ok(m) => Some(m),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    },
                    None => match rx.recv() {
                        Ok(m) => Some(m),
                        Err(_) => break,
                    },
                };
                match msg {
                    Some(Msg::Stop) => break,
                    Some(Msg::Command(id, cmd)) => {
                        if let Err(e) = state.command(&id, cmd) {
                            log_warn!("media: command failed: {e}");
                        }
                    }
                    None => {
                        recheck = None;
                        state.props_dirty = true;
                        state.publish(&bus);
                    }
                    Some(first @ (Msg::Dirty | Msg::Sessions | Msg::Props)) => {
                        let mut props = matches!(first, Msg::Props);
                        let mut sessions = matches!(first, Msg::Sessions);
                        // Debounce bursts of SMTC events.
                        loop {
                            match rx.recv_timeout(Duration::from_millis(40)) {
                                Ok(Msg::Dirty) => {}
                                Ok(Msg::Sessions) => sessions = true,
                                Ok(Msg::Props) => props = true,
                                Ok(Msg::Command(id, cmd)) => {
                                    let _ = state.command(&id, cmd);
                                }
                                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                                Err(RecvTimeoutError::Timeout) => break,
                            }
                        }
                        if sessions {
                            if let Err(e) = state.resubscribe() {
                                log_warn!("media: resubscribe failed: {e}");
                            }
                        }
                        if props || sessions {
                            state.props_dirty = true;
                            recheck = Some(Instant::now() + RECHECK);
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

/// What we last read from a session's media properties.
struct Props {
    title: String,
    artist: String,
    /// Title, artist and album: the art is decoded again only when this changes.
    key: String,
    art: Option<Art>,
}

struct State {
    manager: Manager,
    tx: Sender<Msg>,
    subs: Vec<Subscribed>,
    names: HashMap<String, String>,
    props: HashMap<String, Props>,
    props_dirty: bool,
    wic: Option<IWICImagingFactory>,
}

impl State {
    fn new(tx: Sender<Msg>) -> Result<State> {
        let manager = Manager::RequestAsync()?.join()?;
        let t = tx.clone();
        manager.SessionsChanged(&TypedEventHandler::new(move |_, _| {
            let _ = t.send(Msg::Sessions);
            Ok(())
        }))?;
        let t = tx.clone();
        manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
            let _ = t.send(Msg::Sessions);
            Ok(())
        }))?;
        let wic = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() };
        let mut s = State { manager, tx, subs: Vec::new(), names: Default::default(), props: HashMap::new(), props_dirty: true, wic };
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
            let a = session.MediaPropertiesChanged(&TypedEventHandler::new(notify(&self.tx, || Msg::Props)))?;
            let b = session.PlaybackInfoChanged(&TypedEventHandler::new(notify(&self.tx, || Msg::Dirty)))?;
            let c = session.TimelinePropertiesChanged(&TypedEventHandler::new(notify(&self.tx, || Msg::Dirty)))?;
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
                let slot = format!("{raw_id}#{i}");
                if this.props_dirty || !this.props.contains_key(&slot) {
                    let p = this.read_props(s, this.props.get(&slot))?;
                    this.props.insert(slot.clone(), p);
                }
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
                let p = &this.props[&slot];
                let (title, artist, art) = (p.title.clone(), p.artist.clone(), p.art.clone());
                Ok(MediaSession {
                    app: this.app_name(&raw_id),
                    id,
                    title,
                    artist,
                    art,
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
        self.props_dirty = false;
        // Forget sessions that went away (and their art).
        let live: Vec<String> =
            (0..sessions.len()).filter_map(|i| sessions[i].SourceAppUserModelId().ok().map(|id| format!("{id}#{i}"))).collect();
        self.props.retain(|k, _| live.contains(k));
        out
    }

    fn read_props(&self, s: &Session, old: Option<&Props>) -> Result<Props> {
        let props = s.TryGetMediaPropertiesAsync()?.join()?;
        let title = props.Title()?.to_string();
        // Players fill different fields: browsers often only set the album artist or a subtitle.
        let artist = [props.Artist(), props.AlbumArtist(), props.Subtitle()]
            .into_iter()
            .filter_map(|f| f.ok().map(|h| h.to_string().trim().to_string()))
            .find(|a| !a.is_empty())
            .unwrap_or_default();
        let album = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
        let key = format!("{title}\u{1}{artist}\u{1}{album}");
        // Same track with art already: keep it. New track, or art still missing: decode.
        let art = match old {
            Some(o) if o.key == key && o.art.is_some() => o.art.clone(),
            _ => props.Thumbnail().ok().and_then(|t| self.decode_art(&t).map_err(|e| log_info!("media: no art: {e}")).ok()),
        };
        Ok(Props { title, artist, key, art })
    }

    /// Decode a thumbnail into a square, centre-cropped premultiplied BGRA picture.
    fn decode_art(&self, thumb: &IRandomAccessStreamReference) -> Result<Art> {
        let wic = self.wic.as_ref().ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_NOINTERFACE))?;
        let stream = thumb.OpenReadAsync()?.join()?;
        unsafe {
            let istream: IStream = CreateStreamOverRandomAccessStream(&stream.cast::<windows::core::IUnknown>()?)?;
            let decoder = wic.CreateDecoderFromStream(&istream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
            let frame = decoder.GetFrame(0)?;
            let (mut w, mut h) = (0u32, 0u32);
            frame.GetSize(&mut w, &mut h)?;
            if w == 0 || h == 0 {
                return Err(windows::core::Error::from(windows::Win32::Foundation::E_FAIL));
            }
            let side = w.min(h);
            let clipper = wic.CreateBitmapClipper()?;
            let square = WICRect { X: ((w - side) / 2) as i32, Y: ((h - side) / 2) as i32, Width: side as i32, Height: side as i32 };
            clipper.Initialize(&frame, &square)?;
            let px = ART_PX.min(side);
            let scaler = wic.CreateBitmapScaler()?;
            scaler.Initialize(&clipper, px, px, WICBitmapInterpolationModeFant)?;
            let converter = wic.CreateFormatConverter()?;
            converter.Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )?;
            let mut pixels = vec![0u8; (px * px * 4) as usize];
            converter.CopyPixels(std::ptr::null(), px * 4, &mut pixels)?;
            Ok(Art { id: ART_IDS.fetch_add(1, Ordering::Relaxed), w: px, h: px, pixels: Arc::new(pixels) })
        }
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

fn notify<A: windows::core::RuntimeType + 'static>(
    tx: &Sender<Msg>,
    msg: fn() -> Msg,
) -> impl Fn(windows::core::Ref<Session>, windows::core::Ref<A>) -> Result<()> + Send + 'static {
    let tx = tx.clone();
    move |_, _| {
        let _ = tx.send(msg());
        Ok(())
    }
}
