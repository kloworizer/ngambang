//! The island's state machine. The platform turns OS callbacks into [`Event`]s,
//! calls [`Model::update`], and carries out the returned [`Effect`]s. Nothing in
//! here touches the OS, so every behaviour is unit-tested.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::clipboard::{self, ClipAction, ClipKind};
use crate::config::{Config, ModuleId};
use crate::games::{classify, FullscreenKind, Signals};
use crate::protocol::{Level, Request};

pub const HOVER_DELAY_MS: u64 = 120;
pub const SEAM_PEEK_MS: u64 = 300;
pub const COLLAPSE_MS: u64 = 600;
pub const TAG_MS: u64 = 4000;
pub const TAG_FAIL_MS: u64 = 8000;
pub const GAME_GRACE_MS: u64 = 60_000;
pub const CONFIRM_MS: u64 = 3000;
pub const VOLUME_FLASH_MS: u64 = 1500;
pub const SNOOZE_MS: u64 = 15 * 60_000;
pub const BREAK_REPEAT_MS: u64 = 30 * 60_000;
/// Slips opened without the pointer on them (tray, hotkey) close after this.
pub const UNATTENDED_MS: u64 = 4000;
pub const MAX_JOBS: usize = 8;
pub const MAX_CAPTURES: usize = 5;
/// The notch shows each glance this long.
pub const CAROUSEL_MS: u64 = 6000;

/// What the notch shows at rest when nothing is playing, in turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glance {
    Weather,
    /// A watchlist entry, by index.
    Quote(usize),
    Todo,
    Calendar,
    Prayer,
}
/// A calendar reminder may show this early (timer rounding) or this late
/// (the computer slept through it), in seconds.
const CALENDAR_EARLY_S: i64 = 5;
const CALENDAR_LATE_S: i64 = 15 * 60;
/// "Waktu Dzuhur" still shows this many minutes late.
const PRAYER_LATE_MIN: f64 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IslandState {
    Hidden,
    Seam,
    Orb,
    Tag,
    Slips,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TimerId {
    HoverDelay,
    SeamPeek,
    Collapse,
    TagHide,
    FocusDone,
    Break,
    GameGrace,
    ConfirmExpire,
    VolumeFlash,
    Prayer,
    Calendar,
    /// The notch turns to its next glance.
    Carousel,
}

impl TimerId {
    pub const ALL: [TimerId; 12] = [
        TimerId::HoverDelay,
        TimerId::SeamPeek,
        TimerId::Collapse,
        TimerId::TagHide,
        TimerId::FocusDone,
        TimerId::Break,
        TimerId::GameGrace,
        TimerId::ConfirmExpire,
        TimerId::VolumeFlash,
        TimerId::Prayer,
        TimerId::Calendar,
        TimerId::Carousel,
    ];
}

/// Periodic work the platform runs only while asked to (SPEC §10.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sampler {
    /// Redraw once a second (clocks, elapsed times).
    Second,
    /// GPU load, VRAM, temperature.
    Gpu,
    Ping,
    /// Audio peak meters, ~30 Hz.
    Peaks,
    /// Refresh the listening-ports table every few seconds.
    Ports,
    /// Market quotes, while the watchlist is open.
    Finance,
}

/// Album art, already scaled and converted to premultiplied BGRA by the platform.
#[derive(Clone)]
pub struct Art {
    /// Changes whenever the picture does; equality compares only this.
    pub id: u64,
    pub w: u32,
    pub h: u32,
    pub pixels: std::sync::Arc<Vec<u8>>,
}

impl PartialEq for Art {
    fn eq(&self, other: &Art) -> bool {
        self.id == other.id
    }
}

impl std::fmt::Debug for Art {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Art({:x}, {}×{})", self.id, self.w, self.h)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaSession {
    pub id: String,
    pub app: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    /// Monotonic time at which `position_ms` was true.
    pub at_ms: u64,
    pub can_play_pause: bool,
    pub can_next: bool,
    pub can_prev: bool,
    pub can_seek: bool,
    pub art: Option<Art>,
}

impl MediaSession {
    pub fn position_at(&self, now_ms: u64) -> u64 {
        let p = if self.playing { self.position_ms + now_ms.saturating_sub(self.at_ms) } else { self.position_ms };
        if self.duration_ms > 0 {
            p.min(self.duration_ms)
        } else {
            p
        }
    }

    pub fn progress_at(&self, now_ms: u64) -> Option<f32> {
        (self.duration_ms > 0).then(|| self.position_at(now_ms) as f32 / self.duration_ms as f32)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Volume {
    pub level: f32,
    pub muted: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioApp {
    pub pid: u32,
    pub name: String,
    /// Lower-case executable name, e.g. "spotify.exe".
    pub exe: String,
    /// App user model id for packaged apps.
    pub aumid: Option<String>,
    pub volume: Volume,
    pub peak: f32,
    /// Set by the model: this app also has media controls.
    pub has_controls: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MixerSnapshot {
    pub device: String,
    pub master: Volume,
    pub apps: Vec<AudioApp>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum JobStatus {
    Running { progress: Option<f32> },
    Done { ok: bool, code: Option<i32>, duration_s: Option<f64> },
    Note { level: Level },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: String,
    pub title: String,
    pub body: Option<String>,
    pub status: JobStatus,
    pub started_ms: u64,
    pub updated_ms: u64,
    pub hwnd: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortEntry {
    pub port: u16,
    pub pid: u32,
    pub process: String,
    pub can_stop: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipState {
    pub text: String,
    pub kind: ClipKind,
    pub unix_at: i64,
}

/// One copied text in the clipboard history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipEntry {
    pub id: u64,
    pub text: String,
    /// Unix seconds when it was copied.
    pub at: i64,
    #[serde(default)]
    pub pinned: bool,
}

/// How many unpinned clipboard entries are kept.
pub const CLIP_HISTORY: usize = 20;

/// A quick note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    pub text: String,
    /// Unix seconds when it was written.
    pub at: i64,
}

pub const MAX_NOTES: usize = 50;

/// Battery and charger.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PowerState {
    /// None without a battery (a desktop).
    pub percent: Option<u8>,
    pub on_ac: bool,
    pub saver: bool,
    /// Estimated seconds left on battery.
    pub secs_left: Option<u32>,
}

/// The connection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetState {
    pub online: bool,
    /// The VPN adapter's name while one is connected.
    pub vpn: Option<String>,
    /// The Wi-Fi network's name.
    pub wifi: Option<String>,
}

/// A download in progress (a browser's temporary file growing).
#[derive(Clone, Debug, PartialEq)]
pub struct Download {
    /// The final file name (without .crdownload / .part).
    pub name: String,
    pub bytes: u64,
    pub started_ms: u64,
}

/// What a slip's text field is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputFor {
    Todo,
    /// Filter the clipboard history.
    ClipSearch,
    Note,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub to: InputFor,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

pub const MAX_TODOS: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTimer {
    Idle,
    Running { ends_ms: u64, total_ms: u64 },
    Paused { remaining_ms: u64, total_ms: u64 },
}

impl FocusTimer {
    pub fn remaining(&self, now_ms: u64) -> Option<u64> {
        match *self {
            FocusTimer::Idle => None,
            FocusTimer::Running { ends_ms, .. } => Some(ends_ms.saturating_sub(now_ms)),
            FocusTimer::Paused { remaining_ms, .. } => Some(remaining_ms),
        }
    }

    pub fn progress(&self, now_ms: u64) -> Option<f32> {
        let total = match *self {
            FocusTimer::Idle => return None,
            FocusTimer::Running { total_ms, .. } | FocusTimer::Paused { total_ms, .. } => total_ms,
        };
        let rem = self.remaining(now_ms)?;
        Some(1.0 - rem as f32 / total.max(1) as f32)
    }

    pub fn is_running(&self) -> bool {
        matches!(self, FocusTimer::Running { .. })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuStats {
    pub gpu_pct: Option<f32>,
    pub vram_mb: Option<u64>,
    pub temp_c: Option<u32>,
    pub ping_ms: Option<u32>,
}

impl GpuStats {
    fn merge(&mut self, o: GpuStats) {
        if o.gpu_pct.is_some() {
            self.gpu_pct = o.gpu_pct;
        }
        if o.vram_mb.is_some() {
            self.vram_mb = o.vram_mb;
        }
        if o.temp_c.is_some() {
            self.temp_c = o.temp_c;
        }
        if o.ping_ms.is_some() {
            self.ping_ms = o.ping_ms;
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameSession {
    pub exe: String,
    pub started_ms: u64,
}

/// Minutes played per game, today and persisted in `state.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayLog {
    pub day: String,
    pub minutes: BTreeMap<String, u64>,
}

impl PlayLog {
    pub fn roll(&mut self, day: &str) {
        if self.day != day {
            self.day = day.to_string();
            self.minutes.clear();
        }
    }

    pub fn total(&self) -> u64 {
        self.minutes.values().sum()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureKind {
    Screenshot,
    Clip,
    /// A finished download.
    Download,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    pub path: String,
    pub name: String,
    pub kind: CaptureKind,
    pub at_ms: u64,
}

impl CaptureKind {
    pub fn from_path(path: &str) -> Option<CaptureKind> {
        let ext = path.rsplit('.').next()?.to_ascii_lowercase();
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "webp" | "bmp" | "jxr" => Some(CaptureKind::Screenshot),
            "mp4" | "mkv" | "mov" | "webm" | "gif" => Some(CaptureKind::Clip),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Device {
    Microphone,
    Camera,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrivacyUse {
    pub device: Device,
    pub app: String,
    pub since_ms: u64,
}

/// Icons the view can request; drawn from primitives in `icons.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Note,
    Play,
    Pause,
    Prev,
    Next,
    Speaker,
    SpeakerMuted,
    Mic,
    Camera,
    Check,
    Cross,
    Clock,
    Open,
    Stop,
    Terminal,
    Game,
    Folder,
    Copy,
    Chevron,
    Image,
    Plug,
    Clipboard,
    Moon,
    Download,
    Sun,
    Cloud,
    Rain,
    Storm,
    Warning,
    Todo,
    Plus,
    Minus,
    Trash,
    Gear,
    Pin,
    Grid,
    Calendar,
    Chart,
    Refresh,
    Drop,
    Wind,
    Thermo,
    Pencil,
    Battery,
    Wifi,
}

/// Something the user clicked.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    ToggleSlips,
    Collapse,
    SelectSlip(ModuleId),
    MediaSource(usize),
    MediaPlayPause,
    MediaNext,
    MediaPrev,
    AppMute(u32),
    MasterMute,
    JobOpenTerminal(String),
    JobsClear,
    PortOpen(u16),
    PortStop(u16),
    Clip(ClipAction),
    TimerStart(u32),
    TimerPause,
    TimerResume,
    TimerAdd(u32),
    TimerStop,
    /// End the break early.
    TimerSkipBreak,
    /// Make the custom round shorter (-1) or longer (+1).
    TimerCustom(i32),
    BreakSnooze,
    CaptureOpen(usize),
    CaptureCopy(usize),
    CaptureFolder(usize),
    MicMute,
    PrivacySettings(Device),
    InstallUpdate,
    OpenUrl(String),
    RefreshWeather,
    TodoAdd,
    /// Add what was typed in the slip's field.
    TodoSubmit,
    TodoCancel,
    TodoToggle(u64),
    TodoRemove(u64),
    TodoClearDone,
    /// Open the Settings window at a page (by module).
    OpenSettings(ModuleId),
    /// Paste (or copy, while typing) a clipboard history entry.
    ClipPaste(u64),
    ClipPin(u64),
    ClipRemove(u64),
    ClipSearch,
    ClipClear,
    NoteAdd,
    NoteCopy(u64),
    NoteRemove(u64),
    SetPowerPlan(usize),
    /// Open a Quick launch item, or remove it while editing.
    Launch(usize),
    LaunchRemove(usize),
    LaunchEdit,
    MicToggle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliderTarget {
    Master,
    App(u32),
    MediaSeek,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    pub text: String,
    pub detail: Option<String>,
    pub color: crate::geometry::Color,
    pub icon: Icon,
    /// 1 = most important (SPEC §4.4).
    pub priority: u8,
    pub until_ms: u64,
    pub action: Option<(String, Action)>,
    /// A now-playing tag: drawn live from the front media session, with controls.
    pub media: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Up,
    Down,
    Enter,
    Backspace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fullscreen {
    None,
    Media,
    Game,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)] // short-lived; boxing would only add noise
pub enum Event {
    PointerEnter,
    PointerLeave,
    Click(Action),
    Slider(SliderTarget, f32),
    Wheel {
        notches: i32,
        shift: bool,
    },
    MiddleClick,
    Hotkey,
    Key(Key),
    FocusLost,
    Timer(TimerId),
    Sample(Sampler),
    Media(Vec<MediaSession>),
    Mixer(MixerSnapshot),
    Peaks(Vec<(u32, f32)>),
    Pipe(Request),
    Ports(Vec<PortEntry>),
    Clipboard {
        text: String,
        unix_now: i64,
    },
    /// Pinned clipboard entries and notes, restored at start.
    ClipPins(Vec<ClipEntry>),
    Notes(Vec<Note>),
    Power(PowerState),
    Network(NetState),
    /// Power plans (GUID, name) and the active one.
    PowerPlans(Vec<(String, String)>, Option<String>),
    /// Files dropped on the island.
    Dropped(Vec<String>),
    /// An icon for a Quick launch item, by path.
    LaunchIcon(String, Art),
    /// A download started or grew (`done` = None) or finished (the final path).
    Download {
        name: String,
        bytes: u64,
        done: Option<String>,
    },
    /// The default microphone is muted or not.
    MicMuted(bool),
    /// `exe` is the full executable path of the fullscreen app.
    Fullscreen {
        active: bool,
        exe: String,
        signals: Signals,
    },
    /// The island moved to another display while a fullscreen app covers
    /// its own (true), or came back (false).
    Displaced(bool),
    /// Headlines about the watchlist.
    News(Vec<crate::finance::NewsItem>),
    /// Another window became the foreground (the user clicked elsewhere).
    OutsideFocus,
    /// The game's process has exited: end the session now, not after the grace period.
    GameExited,
    /// Local wall clock, sent at start, at midnight and after time changes.
    Clock {
        year: i32,
        month: u32,
        day: u32,
        minutes: f64,
        utc_offset_min: i32,
        unix: i64,
    },
    Weather(crate::weather::Weather),
    /// Current early warnings from all feeds.
    Alerts(Vec<crate::weather::Alert>),
    /// Internet reachable or not, as Windows reports it.
    Online(bool),
    /// A newer release exists.
    UpdateAvailable {
        version: String,
    },
    Stats(GpuStats),
    Capture(Capture),
    Privacy(Vec<(Device, String)>),
    Day(String),
    Config(Config),
    SetModule(ModuleId, bool),
    Pause(bool),
    /// A message from the island itself (config warnings and the like).
    Notice(String),
    /// The saved to-do list, at start.
    Todos(Vec<TodoItem>),
    /// A task to add (from the command line or tests).
    TodoAdded(String),
    /// Characters typed or pasted while a text field has focus.
    Text(String),
    /// Upcoming events from every linked calendar.
    Calendar(Vec<crate::calendar::CalEvent>),
    Quotes(Vec<crate::finance::Quote>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCmd {
    PlayPause,
    Next,
    Prev,
    Seek(u64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Render,
    SetTimer(TimerId, u64),
    KillTimer(TimerId),
    StartSampler(Sampler),
    StopSampler(Sampler),
    Media {
        session: String,
        cmd: MediaCmd,
    },
    /// Send the play/pause media key (no session to target).
    MediaKey,
    SetMasterVolume(f32),
    SetMasterMute(bool),
    SetAppVolume(u32, f32),
    SetAppMute(u32, bool),
    MuteMic,
    Open(String),
    ShowInFolder(String),
    CopyText(String),
    CopyFile(String),
    Kill(u32),
    FocusWindow(u64),
    RefreshPorts,
    GameStarted(String),
    GameEnded,
    SavePlayLog,
    SaveModule(ModuleId, bool),
    TakeFocus,
    ReleaseFocus,
    /// Download and install the latest release, then restart.
    InstallUpdate,
    /// Fetch weather and warnings now.
    RefreshWeather,
    SaveTodos,
    /// Open the Settings window at the page for this module.
    OpenSettings(ModuleId),
    /// Write one value to config.toml: table, key, TOML literal.
    SaveValue(&'static str, &'static str, String),
    /// Windows' focus session (Do not disturb) on or off.
    FocusSession(bool),
    /// Put text on the clipboard and, when `paste`, paste it into the
    /// window that has the keyboard.
    PasteText(String, bool),
    SaveClipPins,
    SaveNotes,
    SetPowerPlan(String),
    /// Flip the default microphone's mute.
    MicToggle,
}

/// The local wall clock, anchored to the monotonic clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// Local minutes after midnight at `at_ms`.
    pub minutes: f64,
    pub at_ms: u64,
    pub utc_offset_min: i32,
    /// Unix seconds at `at_ms`.
    pub unix: i64,
}

pub struct Model {
    pub config: Config,
    pub state: IslandState,
    pub front: Option<ModuleId>,
    pub hover: bool,
    /// The tag is a hover summary rather than a queued event.
    pub hover_tag: bool,
    pub keyboard_open: bool,
    pub paused: bool,
    pub fullscreen: Fullscreen,
    /// Moved to another display, away from the fullscreen app.
    pub displaced: bool,
    pub now_ms: u64,

    pub media: Vec<MediaSession>,
    pub media_selected: Option<String>,
    pub mixer: MixerSnapshot,
    pub jobs: Vec<Job>,
    pub ports: Vec<PortEntry>,
    pub port_confirm: BTreeMap<u16, u64>,
    pub clip: Option<ClipState>,
    pub timer: FocusTimer,
    pub game: Option<GameSession>,
    pub stats: GpuStats,
    pub playlog: PlayLog,
    pub captures: Vec<Capture>,
    pub privacy: Vec<PrivacyUse>,
    pub tags: Vec<Tag>,
    pub volume_flash_until: u64,
    /// Whose volume the flash shows: an app (pid) or, when None, the system.
    pub volume_target: Option<u32>,
    /// When the next break reminder fires (monotonic ms).
    pub break_due_ms: Option<u64>,
    pub clock: Option<Clock>,
    pub prayer_day: Option<crate::prayer::Day>,
    /// A newer version on GitHub, if one was found.
    pub update: Option<String>,
    pub weather: Option<crate::weather::Weather>,
    pub alerts: Vec<crate::weather::Alert>,
    alerts_seen: BTreeSet<String>,
    /// None until Windows has told us.
    pub online: Option<bool>,
    pub todos: Vec<TodoItem>,
    /// The "Add task" field while it is open.
    pub input: Option<Input>,
    pub calendar: Vec<crate::calendar::CalEvent>,
    /// Reminders already shown: (start, title, lead in seconds).
    calendar_reminded: BTreeSet<(i64, String, i64)>,
    /// Prayer moments already announced today: (date, minute, at the time).
    prayer_announced: BTreeSet<((i32, u32, u32), i64, bool)>,
    pub quotes: Vec<crate::finance::Quote>,
    /// Latest headlines about the watchlist, newest first.
    pub news: Vec<crate::finance::NewsItem>,
    news_seen: BTreeSet<String>,
    /// Steps of `move_pct` already announced today, per symbol.
    moves: BTreeMap<String, i32>,

    samplers: BTreeSet<Sampler>,
    rng: u64,
    /// Which glance the notch shows (counts up; taken modulo the glances).
    pub carousel: usize,
    carousel_armed: bool,
    /// The running timer is a break, not a focus round.
    pub on_break: bool,
    /// Focus rounds finished today.
    pub focus_rounds: u32,
    /// The last round's length, offered again after a break.
    focus_last_min: u32,
    /// Windows' focus session is on (we turned it on).
    focus_session_on: bool,
    pub clip_history: Vec<ClipEntry>,
    pub notes: Vec<Note>,
    pub power: Option<PowerState>,
    pub net: Option<NetState>,
    pub power_plans: Vec<(String, String)>,
    pub active_plan: Option<String>,
    /// Battery warnings already given this discharge (the levels).
    battery_warned: BTreeSet<u8>,
    pub launch_icons: BTreeMap<String, Art>,
    pub launch_edit: bool,
    pub downloads: Vec<Download>,
    pub mic_muted: Option<bool>,
    /// Price levels already crossed, per symbol: (above, below).
    levels_hit: BTreeMap<String, (bool, bool)>,
    next_id: u64,
}

/// A media session (SMTC app id) and a mixer row belong to the same app.
fn session_matches(session_id: &str, app: &AudioApp) -> bool {
    let id = session_id.to_ascii_lowercase();
    let id = id.split('#').next().unwrap_or(&id);
    let exe = app.exe.to_ascii_lowercase();
    let stem = exe.trim_end_matches(".exe");
    (!exe.is_empty() && (id == exe || id == stem))
        || app.aumid.as_ref().is_some_and(|a| a.eq_ignore_ascii_case(id))
        || (!stem.is_empty() && id.ends_with(&format!("!{stem}")))
}

impl Model {
    pub fn new(config: Config, seed: u64, now_ms: u64) -> Model {
        let mut m = Model {
            config,
            state: IslandState::Orb,
            front: None,
            hover: false,
            hover_tag: false,
            keyboard_open: false,
            paused: false,
            fullscreen: Fullscreen::None,
            displaced: false,
            now_ms,
            media: Vec::new(),
            media_selected: None,
            mixer: MixerSnapshot::default(),
            jobs: Vec::new(),
            ports: Vec::new(),
            port_confirm: BTreeMap::new(),
            clip: None,
            timer: FocusTimer::Idle,
            game: None,
            stats: GpuStats::default(),
            playlog: PlayLog::default(),
            captures: Vec::new(),
            privacy: Vec::new(),
            tags: Vec::new(),
            volume_flash_until: 0,
            volume_target: None,
            break_due_ms: None,
            clock: None,
            prayer_day: None,
            update: None,
            weather: None,
            alerts: Vec::new(),
            alerts_seen: BTreeSet::new(),
            online: None,
            todos: Vec::new(),
            input: None,
            calendar: Vec::new(),
            calendar_reminded: BTreeSet::new(),
            prayer_announced: BTreeSet::new(),
            quotes: Vec::new(),
            news: Vec::new(),
            news_seen: BTreeSet::new(),
            moves: BTreeMap::new(),
            samplers: BTreeSet::new(),
            rng: seed | 1,
            carousel: 0,
            carousel_armed: false,
            on_break: false,
            focus_rounds: 0,
            focus_last_min: 25,
            focus_session_on: false,
            clip_history: Vec::new(),
            notes: Vec::new(),
            power: None,
            net: None,
            power_plans: Vec::new(),
            active_plan: None,
            battery_warned: BTreeSet::new(),
            launch_icons: BTreeMap::new(),
            launch_edit: false,
            downloads: Vec::new(),
            mic_muted: None,
            levels_hit: BTreeMap::new(),
            next_id: 1,
        };
        m.state = m.rest_state();
        m
    }

    pub fn enabled(&self, m: ModuleId) -> bool {
        self.config.modules.get(m)
    }

    /// The media session shown in front: the selected one, else the first playing, else the first.
    pub fn front_media(&self) -> Option<&MediaSession> {
        if !self.enabled(ModuleId::Media) {
            return None;
        }
        self.media_selected
            .as_ref()
            .and_then(|id| self.media.iter().find(|s| &s.id == id))
            .or_else(|| self.media.iter().find(|s| s.playing))
            .or_else(|| self.media.first())
    }

    /// The tag currently shown: the most important queued tag.
    pub fn current_tag(&self) -> Option<&Tag> {
        self.tags.iter().min_by_key(|t| t.priority)
    }

    /// Slips that have something to show, in the user's tab order.
    pub fn available_slips(&self) -> Vec<ModuleId> {
        self.slip_ranks().into_iter().map(|(_, m)| m).collect()
    }

    /// The slip that most needs attention, to open first.
    pub fn most_urgent_slip(&self) -> Option<ModuleId> {
        self.slip_ranks().into_iter().min_by_key(|(rank, _)| *rank).map(|(_, m)| m)
    }

    /// Available slips with their urgency (lower is more urgent), in tab order.
    fn slip_ranks(&self) -> Vec<(u8, ModuleId)> {
        self.config
            .modules
            .ordered()
            .into_iter()
            .filter(|m| self.enabled(*m))
            .filter_map(|m| {
                let rank = match m {
                    ModuleId::Privacy if !self.privacy.is_empty() => 0,
                    ModuleId::Privacy => return None,
                    ModuleId::Timer if self.timer != FocusTimer::Idle => 1,
                    ModuleId::Game if self.game.is_some() => 2,
                    ModuleId::Commands if self.jobs.iter().any(|j| matches!(j.status, JobStatus::Done { ok: false, .. })) => 3,
                    ModuleId::Commands if self.jobs.iter().any(|j| matches!(j.status, JobStatus::Running { .. })) => 4,
                    ModuleId::Media if self.media.iter().any(|s| s.playing) => 5,
                    ModuleId::Media if !self.media.is_empty() => 6,
                    ModuleId::Media => return None,
                    ModuleId::Commands if !self.jobs.is_empty() => 7,
                    ModuleId::Commands => return None,
                    ModuleId::Clipboard if self.typing(InputFor::ClipSearch).is_some() => 0,
                    ModuleId::Clipboard if self.clip.is_some() => 8,
                    ModuleId::Clipboard if !self.clip_history.is_empty() => 12,
                    ModuleId::Clipboard => return None,
                    ModuleId::Captures if !self.downloads.is_empty() => 4,
                    ModuleId::Captures if !self.captures.is_empty() => 9,
                    ModuleId::Captures => return None,
                    ModuleId::Prayer if self.prayer_soon(15.0) => 3,
                    ModuleId::Prayer if self.prayer_day.is_some() => 11,
                    ModuleId::Prayer => return None,
                    ModuleId::Weather if self.urgent_alert() => 0,
                    ModuleId::Weather if self.weather.is_some() || !self.alerts.is_empty() || self.online == Some(false) => 10,
                    ModuleId::Weather => return None,
                    ModuleId::Todo if self.typing(InputFor::Todo).is_some() => 0,
                    ModuleId::Todo if self.todos.iter().any(|t| !t.done) => 8,
                    ModuleId::Todo => 13,
                    ModuleId::Calendar if self.next_event_within(15 * 60).is_some() => 2,
                    ModuleId::Calendar if !self.calendar.is_empty() => 9,
                    ModuleId::Calendar => 14,
                    ModuleId::Finance => 14,
                    ModuleId::Notes if self.typing(InputFor::Note).is_some() => 0,
                    ModuleId::Notes => 13,
                    ModuleId::Power
                        if self.power.is_some_and(|p| !p.on_ac && p.percent.is_some_and(|v| v <= self.config.power.low_battery as u8)) =>
                    {
                        3
                    }
                    ModuleId::Power => 12,
                    ModuleId::Launch => 14,
                    ModuleId::Game if self.playlog.total() > 0 => 12,
                    ModuleId::Game => return None,
                    // Always available: mixer, ports, timer.
                    _ => 10,
                };
                Some((rank, m))
            })
            .collect()
    }

    fn rest_state(&self) -> IslandState {
        if self.paused {
            IslandState::Hidden
        } else if self.fullscreen != Fullscreen::None && self.config.dock.hide_in_fullscreen && !self.displaced {
            IslandState::Seam
        } else {
            IslandState::Orb
        }
    }

    fn quiet(&self) -> bool {
        (self.fullscreen != Fullscreen::None && !self.displaced)
            || (self.config.timer.quiet_while_running && self.timer.is_running() && !self.on_break)
    }

    /// Windows' Do not disturb follows focus rounds (when asked to).
    fn focus_session(&mut self, on: bool, fx: &mut Vec<Effect>) {
        let on = on && self.config.timer.focus_assist;
        if on != self.focus_session_on {
            self.focus_session_on = on;
            fx.push(Effect::FocusSession(on));
        }
    }

    fn tag_allowed(&self, priority: u8) -> bool {
        !self.paused && (priority <= 2 || !self.quiet())
    }

    fn next_random(&mut self) -> [u8; 16] {
        // splitmix64; good enough for "New UUID" in a developer tool.
        let mut out = [0u8; 16];
        for half in out.chunks_mut(8) {
            self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.rng;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            half.copy_from_slice(&(z ^ (z >> 31)).to_le_bytes());
        }
        out
    }

    /// Apply one event. Returns the effects for the platform to perform.
    pub fn update(&mut self, now_ms: u64, event: Event) -> Vec<Effect> {
        self.now_ms = now_ms.max(self.now_ms);
        let mut fx = Vec::new();
        self.handle(event, &mut fx);
        self.reconcile_samplers(&mut fx);
        self.reconcile_carousel(&mut fx);
        // One render per update, at the end.
        fx.retain(|e| *e != Effect::Render);
        fx.push(Effect::Render);
        fx
    }

    fn go(&mut self, state: IslandState, fx: &mut Vec<Effect>) {
        if self.state == state {
            return;
        }
        if self.state == IslandState::Slips {
            fx.push(Effect::KillTimer(TimerId::Collapse));
            self.input = None;
            if self.keyboard_open {
                self.keyboard_open = false;
                fx.push(Effect::ReleaseFocus);
            }
        }
        if state != IslandState::Tag {
            self.hover_tag = false;
        }
        if state == IslandState::Slips {
            let avail = self.available_slips();
            if avail.is_empty() {
                return;
            }
            if self.front.is_none_or(|f| !avail.contains(&f)) {
                self.front = self.most_urgent_slip();
            }
            self.front_opened(fx);
            // Opened from the tray or a click that left: don't stay open forever.
            if !self.hover && !self.keyboard_open {
                fx.push(Effect::SetTimer(TimerId::Collapse, UNATTENDED_MS));
            }
        }
        self.state = state;
    }

    /// Go back to rest, or to the next queued tag.
    fn settle(&mut self, fx: &mut Vec<Effect>) {
        self.expire_tags();
        if self.current_tag().is_some() {
            self.hover_tag = false;
            self.go(IslandState::Tag, fx);
            self.arm_tag_timer(fx);
        } else {
            let rest = self.rest_state();
            self.go(rest, fx);
        }
    }

    fn expire_tags(&mut self) {
        let now = self.now_ms;
        self.tags.retain(|t| t.until_ms > now);
    }

    fn arm_tag_timer(&self, fx: &mut Vec<Effect>) {
        if let Some(next) = self.tags.iter().map(|t| t.until_ms).min() {
            fx.push(Effect::SetTimer(TimerId::TagHide, next.saturating_sub(self.now_ms).max(1)));
        }
    }

    fn push_tag(&mut self, tag: Tag, fx: &mut Vec<Effect>) {
        if !self.tag_allowed(tag.priority) {
            return;
        }
        // One tag per text: a repeated event refreshes instead of stacking.
        self.tags.retain(|t| t.text != tag.text && !(t.media && tag.media));
        self.tags.push(tag);
        self.tags.sort_by_key(|t| t.priority);
        self.tags.truncate(6);
        if matches!(self.state, IslandState::Orb | IslandState::Seam | IslandState::Tag) {
            self.hover_tag = false;
            self.go(IslandState::Tag, fx);
        }
        self.arm_tag_timer(fx);
    }

    fn tag(&self, text: String, color: crate::geometry::Color, icon: Icon, priority: u8, ms: u64) -> Tag {
        Tag { text, detail: None, color, icon, priority, until_ms: self.now_ms + ms, action: None, media: false }
    }

    fn handle(&mut self, event: Event, fx: &mut Vec<Effect>) {
        use crate::theme;
        match event {
            Event::PointerEnter => {
                self.hover = true;
                match self.state {
                    IslandState::Orb => fx.push(Effect::SetTimer(TimerId::HoverDelay, HOVER_DELAY_MS)),
                    IslandState::Seam => fx.push(Effect::SetTimer(TimerId::SeamPeek, SEAM_PEEK_MS)),
                    IslandState::Slips => fx.push(Effect::KillTimer(TimerId::Collapse)),
                    _ => {}
                }
            }
            Event::PointerLeave => {
                self.hover = false;
                fx.push(Effect::KillTimer(TimerId::HoverDelay));
                fx.push(Effect::KillTimer(TimerId::SeamPeek));
                match self.state {
                    IslandState::Slips if !self.keyboard_open => fx.push(Effect::SetTimer(TimerId::Collapse, COLLAPSE_MS)),
                    IslandState::Tag if self.hover_tag => self.settle(fx),
                    _ => {}
                }
            }
            Event::Timer(TimerId::HoverDelay) | Event::Timer(TimerId::SeamPeek) => {
                if self.hover && matches!(self.state, IslandState::Orb | IslandState::Seam) && self.has_summary() {
                    self.go(IslandState::Tag, fx);
                    self.hover_tag = true;
                }
            }
            Event::Timer(TimerId::Collapse) => {
                if self.state == IslandState::Slips && !self.hover && !self.keyboard_open {
                    self.settle(fx);
                }
            }
            Event::Timer(TimerId::TagHide) => {
                self.expire_tags();
                if self.state == IslandState::Tag && !self.hover_tag {
                    if self.hover && self.has_summary() {
                        // Keep the tag open as a summary while the pointer is on it.
                        self.hover_tag = self.current_tag().is_none();
                        self.arm_tag_timer(fx);
                    } else {
                        self.settle(fx);
                    }
                } else {
                    self.arm_tag_timer(fx);
                }
            }
            Event::Timer(TimerId::VolumeFlash) | Event::Timer(TimerId::ConfirmExpire) => {
                let now = self.now_ms;
                self.port_confirm.retain(|_, until| *until > now);
            }
            Event::Timer(TimerId::FocusDone) => {
                if self.timer.is_running() {
                    self.timer = FocusTimer::Idle;
                    self.focus_session(false, fx);
                    let t = &self.config.timer;
                    if self.on_break {
                        // Break over: offer the next round.
                        self.on_break = false;
                        let min = self.focus_last_min.max(1);
                        let mut tag = self.tag("Break over · ready to focus".into(), theme::GAME, Icon::Clock, 2, 15_000);
                        tag.action = Some((format!("{min} min"), Action::TimerStart(min)));
                        self.push_tag(tag, fx);
                    } else {
                        self.focus_rounds += 1;
                        let long = self.focus_rounds % t.long_every.max(1) == 0;
                        let pause = if long { t.long_break_min } else { t.break_min };
                        let text = if pause > 0 {
                            format!("Round {} done · {pause} min break", self.focus_rounds)
                        } else {
                            format!("Round {} done", self.focus_rounds)
                        };
                        let tag = self.tag(text, theme::GAME, Icon::Clock, 2, TAG_FAIL_MS);
                        self.push_tag(tag, fx);
                        if pause > 0 {
                            let total = pause as u64 * 60_000;
                            self.on_break = true;
                            self.timer = FocusTimer::Running { ends_ms: self.now_ms + total, total_ms: total };
                            fx.push(Effect::SetTimer(TimerId::FocusDone, total));
                        }
                    }
                }
            }
            Event::Timer(TimerId::Break) => {
                if let Some(s) = &self.game {
                    let played = crate::format::hours_minutes(self.now_ms.saturating_sub(s.started_ms) / 60_000);
                    let mut t = self.tag(format!("{played} played · time for a break"), theme::MEDIA, Icon::Clock, 2, 8000);
                    t.action = Some(("Snooze".into(), Action::BreakSnooze));
                    self.push_tag(t, fx);
                    self.break_due_ms = Some(self.now_ms + BREAK_REPEAT_MS);
                    fx.push(Effect::SetTimer(TimerId::Break, BREAK_REPEAT_MS));
                }
            }
            Event::Timer(TimerId::GameGrace) => self.end_game(fx),
            Event::GameExited => {
                if self.game.is_some() && self.fullscreen != Fullscreen::Game {
                    self.end_game(fx);
                }
            }
            Event::Timer(TimerId::Prayer) => self.prayer_alerts(fx),
            Event::Timer(TimerId::Carousel) => {
                self.carousel_armed = false;
                self.carousel = self.carousel.wrapping_add(1);
            }
            Event::Sample(_) => {}

            Event::Click(action) => self.click(action, fx),
            Event::Slider(target, value) => {
                let v = value.clamp(0.0, 1.0);
                match target {
                    SliderTarget::Master => {
                        self.mixer.master.level = v;
                        fx.push(Effect::SetMasterVolume(v));
                        self.volume_target = None;
                        self.flash_volume(fx);
                    }
                    SliderTarget::App(pid) => {
                        if let Some(a) = self.mixer.apps.iter_mut().find(|a| a.pid == pid) {
                            a.volume.level = v;
                        }
                        fx.push(Effect::SetAppVolume(pid, v));
                    }
                    SliderTarget::MediaSeek => {
                        if let Some(s) = self.front_media().filter(|s| s.can_seek && s.duration_ms > 0) {
                            let ms = (v as f64 * s.duration_ms as f64) as u64;
                            fx.push(Effect::Media { session: s.id.clone(), cmd: MediaCmd::Seek(ms) });
                        }
                    }
                }
            }
            Event::Wheel { notches, shift } => match self.state {
                IslandState::Slips => self.flip(-notches.signum(), fx),
                IslandState::Orb | IslandState::Tag if shift => {
                    let cmd = if notches < 0 { MediaCmd::Next } else { MediaCmd::Prev };
                    if let Some(s) = self.front_media() {
                        fx.push(Effect::Media { session: s.id.clone(), cmd });
                    }
                }
                IslandState::Orb | IslandState::Tag if self.enabled(ModuleId::Mixer) || self.enabled(ModuleId::Media) => {
                    let step = self.config.media.volume_step as f32 / 100.0;
                    // The player's own volume; the system volume when no player can be matched.
                    match self.player_app().map(|a| (a.pid, a.volume.level)) {
                        Some((pid, level)) => {
                            let v = (level + notches as f32 * step).clamp(0.0, 1.0);
                            if let Some(a) = self.mixer.apps.iter_mut().find(|a| a.pid == pid) {
                                a.volume.level = v;
                            }
                            self.volume_target = Some(pid);
                            fx.push(Effect::SetAppVolume(pid, v));
                        }
                        None => {
                            let v = (self.mixer.master.level + notches as f32 * step).clamp(0.0, 1.0);
                            self.mixer.master.level = v;
                            self.volume_target = None;
                            fx.push(Effect::SetMasterVolume(v));
                        }
                    }
                    self.flash_volume(fx);
                }
                _ => {}
            },
            Event::MiddleClick => match self.front_media() {
                Some(s) => fx.push(Effect::Media { session: s.id.clone(), cmd: MediaCmd::PlayPause }),
                None => fx.push(Effect::MediaKey),
            },
            Event::Hotkey => {
                if self.state == IslandState::Slips {
                    self.settle(fx);
                } else if !self.paused {
                    self.go(IslandState::Slips, fx);
                    if self.state == IslandState::Slips {
                        self.keyboard_open = true;
                        fx.push(Effect::TakeFocus);
                    }
                }
            }
            Event::Key(Key::Escape) if self.input.is_some() => self.end_input(false, fx),
            Event::Key(Key::Enter) if self.input.is_some() => self.end_input(true, fx),
            Event::Key(Key::Backspace) => {
                if let Some(i) = self.input.as_mut() {
                    i.text.pop();
                }
            }
            Event::Key(Key::Enter) => {}
            Event::Key(_) if self.input.is_some() => {}
            Event::Key(Key::Escape) => {
                if self.state == IslandState::Slips {
                    self.settle(fx);
                }
            }
            Event::Key(Key::Up) => self.flip(-1, fx),
            Event::Key(Key::Down) => self.flip(1, fx),
            Event::Text(text) => {
                if let Some(t) = self.input.as_mut().map(|i| &mut i.text) {
                    // One line, printable characters, a sensible length.
                    let room = 200usize.saturating_sub(t.chars().count());
                    t.extend(text.chars().map(|c| if c == '\n' || c == '\t' { ' ' } else { c }).filter(|c| !c.is_control()).take(room));
                }
            }
            Event::Calendar(list) => {
                if self.enabled(ModuleId::Calendar) {
                    self.calendar = list;
                    self.calendar_alerts(fx);
                }
            }
            Event::Quotes(list) => {
                if self.enabled(ModuleId::Finance) {
                    self.price_moves(&list, fx);
                    self.price_levels(&list, fx);
                    self.quotes = list;
                }
            }
            Event::News(list) => {
                if self.enabled(ModuleId::Finance) {
                    self.new_headlines(list, fx);
                }
            }
            Event::Timer(TimerId::Calendar) => self.calendar_alerts(fx),
            Event::OutsideFocus => {
                if !self.hover {
                    match self.state {
                        IslandState::Slips if !self.keyboard_open => self.settle(fx),
                        IslandState::Tag if self.hover_tag => self.settle(fx),
                        _ => {}
                    }
                }
            }
            Event::Clock { year, month, day, minutes, utc_offset_min, unix } => {
                self.clock = Some(Clock { year, month, day, minutes, at_ms: self.now_ms, utc_offset_min, unix });
                self.compute_prayer(fx);
                self.calendar_alerts(fx);
            }
            Event::Weather(w) => {
                if self.enabled(ModuleId::Weather) {
                    // A change of sky (not the first reading) gets a short pop-up.
                    let changed = self.weather.as_ref().is_some_and(|old| old.condition != w.condition && old.place == w.place);
                    if changed {
                        let text = format!("{} · {:.0}°", w.description, w.temp_c);
                        let t = self.tag(text, theme::PORTS, crate::view::condition_icon(w.condition), 6, TAG_MS + 1000);
                        self.push_tag(t, fx);
                    }
                    self.weather = Some(w);
                }
            }
            Event::Alerts(list) => {
                if self.enabled(ModuleId::Weather) {
                    self.new_alerts(list, fx);
                }
            }
            Event::Online(on) => self.online = Some(on),
            Event::UpdateAvailable { version } => {
                if self.update.as_deref() != Some(version.as_str()) {
                    let mut t = self.tag(format!("Ngambang {version} is available"), theme::OK, Icon::Download, 5, 12_000);
                    t.action = Some(("Install".into(), Action::InstallUpdate));
                    self.update = Some(version);
                    self.push_tag(t, fx);
                }
            }
            Event::FocusLost => {
                // Clicking away keeps what was typed.
                if self.input.is_some() {
                    self.end_input(true, fx);
                }
                if self.state == IslandState::Slips && self.keyboard_open {
                    self.keyboard_open = false;
                    self.settle(fx);
                }
            }

            Event::Media(sessions) => {
                if !self.enabled(ModuleId::Media) {
                    return;
                }
                let before = self.front_media().map(|s| (s.id.clone(), s.title.clone()));
                self.media = sessions;
                if self.media_selected.as_ref().is_some_and(|id| !self.media.iter().any(|s| &s.id == id)) {
                    self.media_selected = None;
                }
                self.match_controls();
                let after = self
                    .front_media()
                    .filter(|s| s.playing && !s.title.is_empty())
                    .map(|s| (s.id.clone(), s.title.clone(), s.artist.clone()));
                if let Some((id, title, artist)) = after {
                    let changed = before.as_ref().is_none_or(|(bid, bt)| *bid != id || *bt != title);
                    if changed && self.config.media.track_tags && self.state != IslandState::Slips {
                        let text = if artist.is_empty() { title } else { format!("{title} · {artist}") };
                        let mut t = self.tag(crate::format::ellipsize(&text, 48), theme::MEDIA, Icon::Note, 6, TAG_MS + 1000);
                        t.media = true;
                        self.push_tag(t, fx);
                    }
                }
            }
            Event::Mixer(mut snap) => {
                for app in &mut snap.apps {
                    if let Some(old) = self.mixer.apps.iter().find(|a| a.pid == app.pid) {
                        app.peak = old.peak;
                    }
                }
                self.mixer = snap;
                self.match_controls();
            }
            Event::Peaks(peaks) => {
                for (pid, p) in peaks {
                    if let Some(a) = self.mixer.apps.iter_mut().find(|a| a.pid == pid) {
                        a.peak = p.clamp(0.0, 1.0);
                    }
                }
            }
            Event::Pipe(Request::Note { text }) => {
                if self.enabled(ModuleId::Notes) {
                    self.add_note(&text, fx);
                    let t = self.tag(format!("Note · {}", crate::format::ellipsize(&text, 40)), theme::OK, Icon::Todo, 5, TAG_MS);
                    self.push_tag(t, fx);
                }
            }
            Event::Pipe(req) => self.pipe(req, fx),
            Event::ClipPins(list) => {
                for e in list.into_iter().filter(|e| e.pinned) {
                    if !self.clip_history.iter().any(|c| c.text == e.text) {
                        self.next_id = self.next_id.max(e.id + 1);
                        self.clip_history.push(e);
                    }
                }
            }
            Event::Notes(list) => {
                self.next_id = self.next_id.max(list.iter().map(|n| n.id + 1).max().unwrap_or(1));
                self.notes = list;
            }
            Event::Power(p) => self.power_changed(p, fx),
            Event::Network(n) => self.network_changed(n, fx),
            Event::PowerPlans(plans, active) => {
                self.power_plans = plans;
                self.active_plan = active;
            }
            Event::Dropped(paths) => self.pin_paths(paths, fx),
            Event::LaunchIcon(path, art) => {
                self.launch_icons.insert(path, art);
            }
            Event::Download { name, bytes, done } => self.download(name, bytes, done, fx),
            Event::MicMuted(m) => self.mic_muted = Some(m),
            Event::Ports(list) => {
                let hide = &self.config.ports.hide_processes;
                let min = self.config.ports.min_port;
                let mut list: Vec<PortEntry> = list
                    .into_iter()
                    .filter(|p| p.port >= min && p.pid > 4 && !hide.iter().any(|h| h.eq_ignore_ascii_case(&p.process)))
                    .collect();
                list.sort_by_key(|p| p.port);
                list.dedup_by_key(|p| p.port);
                self.ports = list;
            }
            Event::Clipboard { text, unix_now } => {
                if !self.enabled(ModuleId::Clipboard) {
                    return;
                }
                self.remember_clip(&text, unix_now);
                match clipboard::detect(&text) {
                    Some(kind) => {
                        if self.config.clipboard.tags {
                            let label = kind.label();
                            let text = match clipboard::status(&kind, unix_now) {
                                Some(s) => format!("{label} · {s}"),
                                None => label.to_string(),
                            };
                            let t = self.tag(text, theme::CODER, Icon::Clipboard, 5, 2500);
                            self.push_tag(t, fx);
                        }
                        self.clip = Some(ClipState { text, kind, unix_at: unix_now });
                    }
                    None => self.clip = None,
                }
            }
            Event::Fullscreen { active, exe, signals } => self.fullscreen_changed(active, exe, signals, fx),
            Event::Displaced(on) => {
                self.displaced = on;
                if matches!(self.state, IslandState::Orb | IslandState::Seam) {
                    let rest = self.rest_state();
                    self.go(rest, fx);
                }
            }
            Event::Stats(s) => self.stats.merge(s),
            Event::Capture(c) => {
                if !self.enabled(ModuleId::Captures) {
                    return;
                }
                let text = match c.kind {
                    CaptureKind::Screenshot => "Screenshot saved",
                    CaptureKind::Clip => "Clip saved",
                    CaptureKind::Download => "Downloaded",
                };
                self.captures.insert(0, c);
                self.captures.truncate(MAX_CAPTURES);
                let t = self.tag(text.into(), theme::PORTS, Icon::Image, 5, TAG_MS);
                self.push_tag(t, fx);
            }
            Event::Privacy(list) => {
                if !self.enabled(ModuleId::Privacy) {
                    return;
                }
                let now = self.now_ms;
                let mut next = Vec::new();
                for (device, app) in list {
                    match self.privacy.iter().find(|p| p.device == device && p.app == app) {
                        Some(p) => next.push(p.clone()),
                        None => {
                            let what = match device {
                                Device::Microphone => "Microphone",
                                Device::Camera => "Camera",
                            };
                            let icon = if device == Device::Camera { Icon::Camera } else { Icon::Mic };
                            let t = self.tag(format!("{what} · {app}"), theme::PRIVACY, icon, 1, 6000);
                            self.push_tag(t, fx);
                            next.push(PrivacyUse { device, app, since_ms: now });
                        }
                    }
                }
                self.privacy = next;
            }
            Event::Day(day) => {
                self.playlog.roll(&day);
                self.focus_rounds = 0;
                // A new trading day: moves count from the new close.
                self.moves.clear();
            }
            Event::Config(cfg) => {
                let modules_before = self.config.modules.clone();
                self.config = cfg;
                for m in ModuleId::ALL {
                    if modules_before.get(m) && !self.enabled(m) {
                        self.clear_module(m, fx);
                    }
                }
                self.compute_prayer(fx);
                self.calendar_alerts(fx);
                if !matches!(self.state, IslandState::Slips | IslandState::Tag) {
                    let rest = self.rest_state();
                    self.go(rest, fx);
                }
            }
            Event::SetModule(m, on) => {
                self.config.modules.set(m, on);
                fx.push(Effect::SaveModule(m, on));
                if on && m == ModuleId::Prayer {
                    self.compute_prayer(fx);
                }
                if !on {
                    self.clear_module(m, fx);
                }
            }
            Event::Notice(text) => {
                let t = self.tag(crate::format::ellipsize(&text, 60), theme::MEDIA, Icon::Terminal, 4, TAG_FAIL_MS);
                self.push_tag(t, fx);
            }
            Event::Todos(list) => self.todos = list,
            Event::TodoAdded(text) => self.add_todo(&text, fx),
            Event::Pause(p) => {
                self.paused = p;
                self.tags.clear();
                self.settle(fx);
            }
        }
    }

    fn has_summary(&self) -> bool {
        crate::view::summary(self).is_some()
    }

    fn flip(&mut self, dir: i32, fx: &mut Vec<Effect>) {
        if self.state != IslandState::Slips || dir == 0 {
            return;
        }
        let avail = self.available_slips();
        if avail.is_empty() {
            return;
        }
        let i = self.front.and_then(|f| avail.iter().position(|m| *m == f)).unwrap_or(0) as i32;
        let n = avail.len() as i32;
        self.front = Some(avail[((i + dir) % n + n) as usize % n as usize]);
        self.front_opened(fx);
    }

    /// The slip in front changed: fetch what it shows if that is old.
    fn front_opened(&self, fx: &mut Vec<Effect>) {
        match self.front {
            Some(ModuleId::Ports) => fx.push(Effect::RefreshPorts),
            Some(ModuleId::Weather) => {
                let stale = match (&self.weather, self.unix_now()) {
                    (Some(w), Some(now)) => now - w.fetched > crate::weather::WEATHER_EVERY_MS as i64 / 1000,
                    _ => true,
                };
                if stale && self.online != Some(false) {
                    fx.push(Effect::RefreshWeather);
                }
            }
            _ => {}
        }
    }

    fn clear_module(&mut self, m: ModuleId, fx: &mut Vec<Effect>) {
        match m {
            ModuleId::Media => {
                self.media.clear();
                self.media_selected = None;
            }
            ModuleId::Mixer => self.mixer = MixerSnapshot::default(),
            ModuleId::Commands => self.jobs.clear(),
            ModuleId::Ports => self.ports.clear(),
            ModuleId::Clipboard => {
                self.clip = None;
                self.clip_history.retain(|e| e.pinned);
                if self.typing(InputFor::ClipSearch).is_some() {
                    self.input = None;
                }
            }
            ModuleId::Timer => {
                self.timer = FocusTimer::Idle;
                fx.push(Effect::KillTimer(TimerId::FocusDone));
            }
            ModuleId::Game => {
                if self.game.is_some() {
                    self.end_game(fx);
                }
                self.stats = GpuStats::default();
            }
            ModuleId::Captures => {
                self.captures.clear();
                self.downloads.clear();
            }
            ModuleId::Privacy => self.privacy.clear(),
            ModuleId::Prayer => {
                self.prayer_day = None;
                fx.push(Effect::KillTimer(TimerId::Prayer));
            }
            ModuleId::Weather => {
                self.weather = None;
                self.alerts.clear();
            }
            // The list itself stays saved for when the module comes back.
            ModuleId::Todo => {
                if self.typing(InputFor::Todo).is_some() {
                    self.input = None;
                }
            }
            ModuleId::Calendar => {
                self.calendar.clear();
                fx.push(Effect::KillTimer(TimerId::Calendar));
            }
            ModuleId::Finance => {
                self.quotes.clear();
                self.news.clear();
            }
            // Notes and pins stay saved for when the module comes back.
            ModuleId::Notes => {
                if self.typing(InputFor::Note).is_some() {
                    self.input = None;
                }
            }
            ModuleId::Power => {}
            ModuleId::Launch => self.launch_edit = false,
        }
        if self.front == Some(m) {
            self.front = None;
            if self.state == IslandState::Slips {
                self.front = self.available_slips().first().copied();
                if self.front.is_none() {
                    self.settle(fx);
                }
            }
        }
    }

    /// Mark mixer rows whose app also publishes media controls.
    fn match_controls(&mut self) {
        let ids: Vec<String> = self.media.iter().map(|s| s.id.clone()).collect();
        for app in &mut self.mixer.apps {
            app.has_controls = ids.iter().any(|id| session_matches(id, app));
        }
    }

    /// The mixer row of the app playing in front, if it is making sound.
    pub fn player_app(&self) -> Option<&AudioApp> {
        let s = self.front_media()?;
        self.mixer.apps.iter().find(|a| session_matches(&s.id, a))
    }

    fn click(&mut self, action: Action, fx: &mut Vec<Effect>) {
        use crate::theme;
        let now = self.now_ms;
        match action {
            Action::ToggleSlips => {
                if self.state == IslandState::Slips {
                    self.settle(fx);
                } else {
                    self.go(IslandState::Slips, fx);
                }
            }
            Action::Collapse => self.settle(fx),
            Action::SelectSlip(m) => {
                if self.available_slips().contains(&m) && self.front != Some(m) {
                    self.front = Some(m);
                    if self.state == IslandState::Slips {
                        self.front_opened(fx);
                    }
                }
                // From a tag ("Open"): open the slips at that module.
                if self.state != IslandState::Slips {
                    self.go(IslandState::Slips, fx);
                }
            }
            Action::MediaSource(i) => {
                if let Some(s) = self.media.get(i) {
                    self.media_selected = Some(s.id.clone());
                }
            }
            Action::MediaPlayPause | Action::MediaNext | Action::MediaPrev => {
                let cmd = match action {
                    Action::MediaPlayPause => MediaCmd::PlayPause,
                    Action::MediaNext => MediaCmd::Next,
                    _ => MediaCmd::Prev,
                };
                match self.front_media() {
                    Some(s) => {
                        let id = s.id.clone();
                        if cmd == MediaCmd::PlayPause {
                            // Optimistic: flip now, the next SMTC update confirms.
                            if let Some(s) = self.media.iter_mut().find(|s| s.id == id) {
                                s.position_ms = s.position_at(now);
                                s.at_ms = now;
                                s.playing = !s.playing;
                            }
                        }
                        fx.push(Effect::Media { session: id, cmd });
                    }
                    None if cmd == MediaCmd::PlayPause => fx.push(Effect::MediaKey),
                    None => {}
                }
            }
            Action::AppMute(pid) => {
                if let Some(a) = self.mixer.apps.iter_mut().find(|a| a.pid == pid) {
                    a.volume.muted = !a.volume.muted;
                    fx.push(Effect::SetAppMute(pid, a.volume.muted));
                }
            }
            Action::MasterMute => {
                self.mixer.master.muted = !self.mixer.master.muted;
                fx.push(Effect::SetMasterMute(self.mixer.master.muted));
                self.volume_target = None;
                self.flash_volume(fx);
            }
            Action::JobOpenTerminal(id) => {
                if let Some(h) = self.jobs.iter().find(|j| j.id == id).and_then(|j| j.hwnd) {
                    fx.push(Effect::FocusWindow(h));
                }
            }
            Action::JobsClear => self.jobs.retain(|j| matches!(j.status, JobStatus::Running { .. })),
            Action::PortOpen(port) => fx.push(Effect::Open(format!("http://localhost:{port}"))),
            Action::PortStop(port) => {
                let Some(entry) = self.ports.iter().find(|p| p.port == port && p.can_stop) else { return };
                if self.port_confirm.get(&port).is_some_and(|until| *until > now) {
                    fx.push(Effect::Kill(entry.pid));
                    fx.push(Effect::RefreshPorts);
                    self.port_confirm.remove(&port);
                } else {
                    self.port_confirm.insert(port, now + CONFIRM_MS);
                    fx.push(Effect::SetTimer(TimerId::ConfirmExpire, CONFIRM_MS));
                }
            }
            Action::Clip(a) => {
                let random = self.next_random();
                if let Some(c) = &self.clip {
                    if let Some(out) = clipboard::apply(a, &c.text, &c.kind, random) {
                        fx.push(Effect::CopyText(out));
                        let t = self.tag(format!("Copied · {}", a.label()), theme::CODER, Icon::Check, 5, 1500);
                        self.push_tag(t, fx);
                    }
                }
            }
            Action::TimerStart(min) => {
                let total = min as u64 * 60_000;
                self.timer = FocusTimer::Running { ends_ms: now + total, total_ms: total };
                self.on_break = false;
                self.focus_last_min = min;
                fx.push(Effect::SetTimer(TimerId::FocusDone, total));
                self.focus_session(true, fx);
            }
            Action::TimerSkipBreak => {
                if self.on_break {
                    self.on_break = false;
                    self.timer = FocusTimer::Idle;
                    fx.push(Effect::KillTimer(TimerId::FocusDone));
                }
            }
            Action::TimerPause => {
                if let FocusTimer::Running { ends_ms, total_ms } = self.timer {
                    self.timer = FocusTimer::Paused { remaining_ms: ends_ms.saturating_sub(now), total_ms };
                    fx.push(Effect::KillTimer(TimerId::FocusDone));
                }
            }
            Action::TimerResume => {
                if let FocusTimer::Paused { remaining_ms, total_ms } = self.timer {
                    self.timer = FocusTimer::Running { ends_ms: now + remaining_ms, total_ms };
                    fx.push(Effect::SetTimer(TimerId::FocusDone, remaining_ms.max(1)));
                }
            }
            Action::TimerAdd(min) => {
                let add = min as u64 * 60_000;
                match &mut self.timer {
                    FocusTimer::Running { ends_ms, total_ms } => {
                        *ends_ms += add;
                        *total_ms += add;
                        fx.push(Effect::SetTimer(TimerId::FocusDone, ends_ms.saturating_sub(now)));
                    }
                    FocusTimer::Paused { remaining_ms, total_ms } => {
                        *remaining_ms += add;
                        *total_ms += add;
                    }
                    FocusTimer::Idle => {}
                }
            }
            Action::TimerStop => {
                self.timer = FocusTimer::Idle;
                self.on_break = false;
                fx.push(Effect::KillTimer(TimerId::FocusDone));
                self.focus_session(false, fx);
            }
            Action::TimerCustom(dir) => {
                let cur = self.config.timer.custom_min as i32;
                // One-minute steps up to ten minutes, five-minute steps above.
                let step = if cur > 10 || (cur == 10 && dir > 0) { 5 } else { 1 };
                let next = (cur + dir.signum() * step).clamp(1, 240) as u32;
                if next != self.config.timer.custom_min {
                    self.config.timer.custom_min = next;
                    fx.push(Effect::SaveValue("timer", "custom_min", next.to_string()));
                }
            }
            Action::BreakSnooze => {
                if self.game.is_some() {
                    self.break_due_ms = Some(now + SNOOZE_MS);
                    fx.push(Effect::SetTimer(TimerId::Break, SNOOZE_MS));
                    self.tags.retain(|t| !t.text.contains("time for a break"));
                    let t = self.tag("Break snoozed · 15 min".into(), theme::GAME, Icon::Clock, 2, 2500);
                    self.push_tag(t, fx);
                }
            }
            Action::InstallUpdate => fx.push(Effect::InstallUpdate),
            Action::OpenUrl(url) => fx.push(Effect::Open(url)),
            Action::RefreshWeather => fx.push(Effect::RefreshWeather),
            Action::TodoAdd => {
                if self.enabled(ModuleId::Todo) {
                    self.start_input(InputFor::Todo, ModuleId::Todo, fx);
                }
            }
            Action::TodoSubmit => self.end_input(true, fx),
            Action::TodoCancel => self.end_input(false, fx),
            Action::OpenSettings(m) => fx.push(Effect::OpenSettings(m)),
            Action::ClipPaste(id) => {
                if let Some(e) = self.clip_history.iter().find(|e| e.id == id) {
                    // While the search field has the keyboard there is nowhere to paste: copy.
                    let paste = !self.keyboard_open;
                    fx.push(Effect::PasteText(e.text.clone(), paste));
                    if paste {
                        self.settle(fx);
                    } else {
                        let t = self.tag("Copied".into(), theme::CODER, Icon::Check, 5, 1500);
                        self.push_tag(t, fx);
                    }
                }
            }
            Action::ClipPin(id) => {
                if let Some(e) = self.clip_history.iter_mut().find(|e| e.id == id) {
                    e.pinned = !e.pinned;
                    fx.push(Effect::SaveClipPins);
                }
            }
            Action::ClipRemove(id) => {
                let pinned = self.clip_history.iter().any(|e| e.id == id && e.pinned);
                self.clip_history.retain(|e| e.id != id);
                if pinned {
                    fx.push(Effect::SaveClipPins);
                }
            }
            Action::ClipClear => self.clip_history.retain(|e| e.pinned),
            Action::ClipSearch => {
                if self.enabled(ModuleId::Clipboard) {
                    self.start_input(InputFor::ClipSearch, ModuleId::Clipboard, fx);
                }
            }
            Action::NoteAdd => {
                if self.enabled(ModuleId::Notes) {
                    self.start_input(InputFor::Note, ModuleId::Notes, fx);
                }
            }
            Action::NoteCopy(id) => {
                if let Some(n) = self.notes.iter().find(|n| n.id == id) {
                    fx.push(Effect::CopyText(n.text.clone()));
                    let t = self.tag("Note copied".into(), theme::OK, Icon::Check, 5, 1500);
                    self.push_tag(t, fx);
                }
            }
            Action::NoteRemove(id) => {
                self.notes.retain(|n| n.id != id);
                fx.push(Effect::SaveNotes);
            }
            Action::SetPowerPlan(i) => {
                if let Some((guid, _)) = self.power_plans.get(i) {
                    self.active_plan = Some(guid.clone());
                    fx.push(Effect::SetPowerPlan(guid.clone()));
                }
            }
            Action::Launch(i) => {
                if self.launch_edit {
                    self.unpin(i, fx);
                } else if let Some(item) = self.config.launch.items.get(i) {
                    fx.push(Effect::Open(item.path.clone()));
                    self.settle(fx);
                }
            }
            Action::LaunchRemove(i) => self.unpin(i, fx),
            Action::LaunchEdit => self.launch_edit = !self.launch_edit,
            Action::MicToggle => fx.push(Effect::MicToggle),
            Action::TodoToggle(id) => {
                if let Some(t) = self.todos.iter_mut().find(|t| t.id == id) {
                    t.done = !t.done;
                    fx.push(Effect::SaveTodos);
                }
            }
            Action::TodoRemove(id) => {
                self.todos.retain(|t| t.id != id);
                fx.push(Effect::SaveTodos);
            }
            Action::TodoClearDone => {
                self.todos.retain(|t| !t.done);
                fx.push(Effect::SaveTodos);
            }
            Action::CaptureOpen(i) | Action::CaptureCopy(i) | Action::CaptureFolder(i) => {
                if let Some(c) = self.captures.get(i) {
                    let p = c.path.clone();
                    fx.push(match action {
                        Action::CaptureOpen(_) => Effect::Open(p),
                        Action::CaptureCopy(_) => Effect::CopyFile(p),
                        _ => Effect::ShowInFolder(p),
                    });
                }
            }
            Action::MicMute => fx.push(Effect::MuteMic),
            Action::PrivacySettings(d) => fx.push(Effect::Open(
                match d {
                    Device::Microphone => "ms-settings:privacy-microphone",
                    Device::Camera => "ms-settings:privacy-webcam",
                }
                .into(),
            )),
        }
    }

    fn pipe(&mut self, req: Request, fx: &mut Vec<Effect>) {
        use crate::theme;
        if !self.enabled(ModuleId::Commands) {
            return;
        }
        let now = self.now_ms;
        let Some(id) = req.job_id() else { return };
        match req {
            Request::Notify { title, body, level, progress, sticky, ttl_ms, hwnd, .. } => {
                let running = progress.is_some() || sticky;
                let status = if running { JobStatus::Running { progress } } else { JobStatus::Note { level } };
                let hwnd = hwnd.as_deref().and_then(crate::protocol::parse_hwnd);
                let is_new = !self.jobs.iter().any(|j| j.id == id);
                let started = self.jobs.iter().find(|j| j.id == id).map(|j| j.started_ms).unwrap_or(now);
                self.jobs.retain(|j| j.id != id);
                self.jobs.insert(0, Job { id: id.clone(), title: title.clone(), body, status, started_ms: started, updated_ms: now, hwnd });
                self.trim_jobs();
                if running {
                    if is_new {
                        let t = self.tag(title, theme::CODER, Icon::Terminal, 4, 3000);
                        self.push_tag(t, fx);
                    }
                } else {
                    let (color, icon, prio, ms) = match level {
                        Level::Fail => (theme::PRIVACY, Icon::Cross, 3, TAG_FAIL_MS),
                        Level::Warn => (theme::MEDIA, Icon::Terminal, 4, TAG_MS),
                        Level::Ok => (theme::OK, Icon::Check, 5, TAG_MS),
                        Level::Info => (theme::CODER, Icon::Terminal, 5, TAG_MS),
                    };
                    let t = self.tag(title, color, icon, prio, ttl_ms.unwrap_or(ms));
                    self.push_tag(t, fx);
                }
            }
            Request::Done { title, ok, code, duration_s, hwnd, .. } => {
                let existing = self.jobs.iter().position(|j| j.id == id);
                let mut job = match existing {
                    Some(i) => self.jobs.remove(i),
                    None => Job {
                        id: id.clone(),
                        title: title.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| id.clone()),
                        body: None,
                        status: JobStatus::Note { level: Level::Info },
                        started_ms: now,
                        updated_ms: now,
                        hwnd: None,
                    },
                };
                let duration_s = duration_s.or_else(|| existing.map(|_| now.saturating_sub(job.started_ms) as f64 / 1000.0));
                job.status = JobStatus::Done { ok, code, duration_s };
                job.updated_ms = now;
                if let Some(h) = hwnd.as_deref().and_then(crate::protocol::parse_hwnd) {
                    job.hwnd = Some(h);
                }
                let mut t = if ok {
                    let mut t = self.tag(job.title.clone(), theme::OK, Icon::Check, 5, TAG_MS);
                    t.detail = duration_s.map(|d| crate::format::duration_short(d.round() as u64));
                    t
                } else {
                    let mut t = self.tag(job.title.clone(), theme::PRIVACY, Icon::Cross, 3, TAG_FAIL_MS);
                    t.detail = Some(match code {
                        Some(c) => format!("exit {c}"),
                        None => "failed".into(),
                    });
                    t
                };
                if !ok && job.hwnd.is_some() {
                    t.action = Some(("Open terminal".into(), Action::JobOpenTerminal(id.clone())));
                }
                self.jobs.insert(0, job);
                self.trim_jobs();
                self.push_tag(t, fx);
            }
            Request::Clear { .. } => self.jobs.retain(|j| j.id != id),
            Request::Ping | Request::Note { .. } => {}
        }
    }

    fn trim_jobs(&mut self) {
        while self.jobs.len() > MAX_JOBS {
            match self.jobs.iter().rposition(|j| !matches!(j.status, JobStatus::Running { .. })) {
                Some(i) => {
                    self.jobs.remove(i);
                }
                None => {
                    self.jobs.pop();
                }
            }
        }
    }

    fn fullscreen_changed(&mut self, active: bool, exe: String, signals: Signals, fx: &mut Vec<Effect>) {
        if active {
            let kind = match classify(&exe, signals, &self.config.game) {
                FullscreenKind::Game if !self.enabled(ModuleId::Game) => FullscreenKind::Media,
                k => k,
            };
            self.fullscreen = match kind {
                // Overlays (snipping, Start, magnifier) change nothing.
                FullscreenKind::Ignore => return,
                FullscreenKind::Game => Fullscreen::Game,
                FullscreenKind::Media => Fullscreen::Media,
            };
            if kind == FullscreenKind::Game {
                let exe = crate::format::exe_name(&exe);
                match &self.game {
                    Some(s) if s.exe == exe => fx.push(Effect::KillTimer(TimerId::GameGrace)),
                    _ => {
                        if self.game.is_some() {
                            self.end_game(fx);
                        }
                        self.game = Some(GameSession { exe: exe.clone(), started_ms: self.now_ms });
                        fx.push(Effect::GameStarted(exe));
                        if self.config.game.break_after_min > 0 {
                            let ms = self.config.game.break_after_min as u64 * 60_000;
                            self.break_due_ms = Some(self.now_ms + ms);
                            fx.push(Effect::SetTimer(TimerId::Break, ms));
                        }
                    }
                }
            }
        } else {
            self.fullscreen = Fullscreen::None;
            if self.game.is_some() {
                fx.push(Effect::SetTimer(TimerId::GameGrace, GAME_GRACE_MS));
            }
        }
        // Games drop open tags of lower priority immediately.
        if self.quiet() {
            self.tags.retain(|t| t.priority <= 2);
        }
        if self.state != IslandState::Slips {
            self.settle(fx);
        }
    }

    fn end_game(&mut self, fx: &mut Vec<Effect>) {
        if let Some(s) = self.game.take() {
            let mins = self.now_ms.saturating_sub(s.started_ms) / 60_000;
            if mins > 0 {
                *self.playlog.minutes.entry(s.exe).or_default() += mins;
                fx.push(Effect::SavePlayLog);
            }
            fx.push(Effect::GameEnded);
            fx.push(Effect::KillTimer(TimerId::Break));
            fx.push(Effect::KillTimer(TimerId::GameGrace));
            self.break_due_ms = None;
            self.stats = GpuStats::default();
        }
    }

    /// Show the volume level for a moment, in whatever form the island is in.
    fn flash_volume(&mut self, fx: &mut Vec<Effect>) {
        self.volume_flash_until = self.now_ms + VOLUME_FLASH_MS;
        fx.push(Effect::SetTimer(TimerId::VolumeFlash, VOLUME_FLASH_MS));
    }

    /// The volume level is being shown.
    pub fn volume_flashing(&self) -> bool {
        self.now_ms < self.volume_flash_until
    }

    /// What the volume flash shows: a name and that volume.
    pub fn flashed_volume(&self) -> (String, Volume) {
        match self.volume_target.and_then(|pid| self.mixer.apps.iter().find(|a| a.pid == pid)) {
            Some(a) => (a.name.clone(), a.volume),
            None => ("System".to_string(), self.mixer.master),
        }
    }

    fn add_todo(&mut self, text: &str, fx: &mut Vec<Effect>) {
        let text: String = text.trim().chars().take(200).collect();
        if !text.is_empty() && self.enabled(ModuleId::Todo) {
            let id = self.todos.iter().map(|t| t.id).max().unwrap_or(0) + 1;
            self.todos.insert(0, TodoItem { id, text, done: false });
            self.todos.truncate(MAX_TODOS);
            fx.push(Effect::SaveTodos);
            if self.state == IslandState::Slips {
                self.front = Some(ModuleId::Todo);
            }
        }
    }

    /// Close the "Add task" field, adding its text when `keep`.
    /// Open a text field in a slip; typing needs the keyboard.
    fn start_input(&mut self, to: InputFor, module: ModuleId, fx: &mut Vec<Effect>) {
        self.input = Some(Input { to, text: String::new() });
        self.front = Some(module);
        fx.push(Effect::KillTimer(TimerId::Collapse));
        if !self.keyboard_open {
            self.keyboard_open = true;
            fx.push(Effect::TakeFocus);
        }
    }

    /// What is being typed for `to`, if its field is open.
    pub fn typing(&self, to: InputFor) -> Option<&str> {
        self.input.as_ref().filter(|i| i.to == to).map(|i| i.text.as_str())
    }

    fn end_input(&mut self, keep: bool, fx: &mut Vec<Effect>) {
        let Some(input) = self.input.take() else { return };
        if keep {
            self.input_done(input, fx);
        }
        if self.keyboard_open {
            self.keyboard_open = false;
            fx.push(Effect::ReleaseFocus);
            if !self.hover && self.state == IslandState::Slips {
                fx.push(Effect::SetTimer(TimerId::Collapse, UNATTENDED_MS));
            }
        }
    }

    /// A field's Enter (or a click away for notes and tasks).
    fn input_done(&mut self, input: Input, fx: &mut Vec<Effect>) {
        match input.to {
            InputFor::Todo => self.add_todo(&input.text, fx),
            InputFor::Note => {
                self.add_note(&input.text, fx);
            }
            // Enter copies the first match.
            InputFor::ClipSearch => {
                if let Some(e) = self.clip_matches(&input.text).first() {
                    fx.push(Effect::PasteText(e.text.clone(), false));
                    let t = self.tag("Copied".into(), crate::theme::CODER, Icon::Check, 5, 1500);
                    self.push_tag(t, fx);
                }
            }
        }
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id - 1
    }

    fn remember_clip(&mut self, text: &str, unix: i64) {
        let text: String = text.chars().take(4000).collect();
        if text.trim().is_empty() {
            return;
        }
        let pinned = self.clip_history.iter().any(|e| e.text == text && e.pinned);
        self.clip_history.retain(|e| e.text != text);
        let id = self.fresh_id();
        self.clip_history.insert(0, ClipEntry { id, text, at: unix, pinned });
        // Keep every pinned entry and the newest unpinned ones.
        let mut unpinned = 0;
        self.clip_history.retain(|e| {
            if !e.pinned {
                unpinned += 1;
            }
            e.pinned || unpinned <= CLIP_HISTORY
        });
    }

    /// History entries matching a search (all without one): pinned first,
    /// then the newest.
    pub fn clip_matches(&self, query: &str) -> Vec<&ClipEntry> {
        let q = query.trim().to_lowercase();
        let mut v: Vec<&ClipEntry> = self.clip_history.iter().filter(|e| q.is_empty() || e.text.to_lowercase().contains(&q)).collect();
        v.sort_by_key(|e| (!e.pinned, std::cmp::Reverse(e.at), std::cmp::Reverse(e.id)));
        v
    }

    fn add_note(&mut self, text: &str, fx: &mut Vec<Effect>) {
        let text: String = text.trim().chars().take(500).collect();
        if text.is_empty() || !self.enabled(ModuleId::Notes) {
            return;
        }
        let id = self.fresh_id();
        let at = self.unix_now().unwrap_or(0);
        self.notes.insert(0, Note { id, text, at });
        self.notes.truncate(MAX_NOTES);
        fx.push(Effect::SaveNotes);
    }

    fn power_changed(&mut self, p: PowerState, fx: &mut Vec<Effect>) {
        let before = self.power;
        self.power = Some(p);
        if !self.enabled(ModuleId::Power) {
            return;
        }
        // Plugged in or out: the plan for that, and fresh warnings next time.
        if before.is_some_and(|b| b.on_ac != p.on_ac) || (before.is_none() && p.percent.is_some()) {
            let c = &self.config.power;
            let plan = if p.on_ac { &c.plugged_plan } else { &c.battery_plan };
            if !plan.is_empty() && self.active_plan.as_deref() != Some(plan.as_str()) {
                self.active_plan = Some(plan.clone());
                fx.push(Effect::SetPowerPlan(plan.clone()));
            }
            if p.on_ac {
                self.battery_warned.clear();
            }
            if before.is_some() && p.percent.is_some() {
                let text = if p.on_ac { "Charging" } else { "On battery" };
                let pct = p.percent.map(|v| format!(" · {v}%")).unwrap_or_default();
                let t = self.tag(format!("{text}{pct}"), crate::theme::OK, Icon::Plug, 6, 2500);
                self.push_tag(t, fx);
            }
        }
        if let (Some(pct), false) = (p.percent, p.on_ac) {
            let low = self.config.power.low_battery.clamp(5, 50) as u8;
            for level in [low, 10] {
                if pct <= level && self.battery_warned.insert(level) {
                    let left = p.secs_left.map(|s| format!(" · {} left", crate::format::hours_minutes(s as u64 / 60))).unwrap_or_default();
                    let prio = if level <= 10 { 1 } else { 3 };
                    let t = self.tag(format!("Battery {pct}%{left}"), crate::theme::PRIVACY, Icon::Plug, prio, 10_000);
                    self.push_tag(t, fx);
                }
            }
        }
    }

    fn network_changed(&mut self, n: NetState, fx: &mut Vec<Effect>) {
        let before = self.net.replace(n.clone());
        self.online = Some(n.online);
        let Some(b) = before.filter(|_| self.enabled(ModuleId::Power) && self.config.power.network_tags) else { return };
        let mut say = |m: &mut Model, text: String, color, prio| {
            let t = m.tag(text, color, Icon::Plug, prio, 4000);
            m.push_tag(t, fx);
        };
        if b.online != n.online {
            if n.online {
                let via = n.wifi.as_ref().map(|w| format!(" · {w}")).unwrap_or_default();
                say(self, format!("Back online{via}"), crate::theme::OK, 6);
            } else {
                say(self, "Offline · no connection".into(), crate::theme::PRIVACY, 4);
            }
        }
        match (&b.vpn, &n.vpn) {
            (None, Some(v)) => say(self, format!("VPN connected · {v}"), crate::theme::OK, 5),
            (Some(v), None) => say(self, format!("VPN disconnected · {v}"), crate::theme::MEDIA, 4),
            _ => {}
        }
    }

    /// Pin dropped files, folders and shortcuts to Quick launch.
    fn pin_paths(&mut self, paths: Vec<String>, fx: &mut Vec<Effect>) {
        if !self.enabled(ModuleId::Launch) {
            return;
        }
        let mut items = self.config.launch.items.clone();
        for p in paths {
            if items.iter().any(|i| i.path.eq_ignore_ascii_case(&p)) || items.len() >= 24 {
                continue;
            }
            let file = p.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or(&p).to_string();
            let name = match file.rsplit_once('.') {
                Some((stem, ext)) if ["exe", "lnk", "url", "appref-ms"].contains(&ext.to_ascii_lowercase().as_str()) => stem.to_string(),
                _ => file,
            };
            items.push(crate::config::LaunchItem { name, path: p });
        }
        if items != self.config.launch.items {
            let added = items.len() - self.config.launch.items.len();
            self.config.launch.items = items;
            fx.push(Effect::SaveValue("launch", "items", crate::settings::launch_literal(&self.config.launch.items)));
            self.front = Some(ModuleId::Launch);
            let t = self.tag(format!("Pinned {added} to Quick launch"), crate::theme::PORTS, Icon::Pin, 5, TAG_MS);
            self.push_tag(t, fx);
        }
    }

    fn unpin(&mut self, i: usize, fx: &mut Vec<Effect>) {
        if i < self.config.launch.items.len() {
            self.config.launch.items.remove(i);
            fx.push(Effect::SaveValue("launch", "items", crate::settings::launch_literal(&self.config.launch.items)));
            if self.config.launch.items.is_empty() {
                self.launch_edit = false;
            }
        }
    }

    fn download(&mut self, name: String, bytes: u64, done: Option<String>, fx: &mut Vec<Effect>) {
        if !self.enabled(ModuleId::Captures) {
            return;
        }
        match done {
            None => match self.downloads.iter_mut().find(|d| d.name == name) {
                Some(d) => d.bytes = bytes,
                None => self.downloads.push(Download { name, bytes, started_ms: self.now_ms }),
            },
            Some(path) => {
                let was = self.downloads.iter().any(|d| d.name == name);
                self.downloads.retain(|d| d.name != name);
                // An empty path: cancelled.
                if was && !path.is_empty() {
                    self.captures
                        .insert(0, Capture { path: path.clone(), name: name.clone(), kind: CaptureKind::Download, at_ms: self.now_ms });
                    self.captures.truncate(MAX_CAPTURES);
                    let mut t = self.tag(
                        format!("Downloaded · {}", crate::format::ellipsize(&name, 36)),
                        crate::theme::PORTS,
                        Icon::Download,
                        5,
                        8000,
                    );
                    t.action = Some(("Open".into(), Action::CaptureOpen(0)));
                    self.push_tag(t, fx);
                }
            }
        }
    }

    /// The event going on now that has a meeting link.
    pub fn meeting(&self) -> Option<&crate::calendar::CalEvent> {
        let now = self.unix_now()?;
        self.calendar.iter().find(|e| !e.all_day && !e.link.is_empty() && e.start - 120 <= now && now < e.end)
    }

    /// Price levels crossed, once per crossing.
    fn price_levels(&mut self, list: &[crate::finance::Quote], fx: &mut Vec<Effect>) {
        let watch = self.config.finance.watchlist.clone();
        for q in list {
            let Some(w) = watch.iter().find(|w| w.symbol.eq_ignore_ascii_case(&q.symbol)) else { continue };
            let hit = self.levels_hit.entry(q.symbol.clone()).or_insert((false, false));
            let mut say = Vec::new();
            if let Some(a) = w.above {
                let now_above = q.price >= a;
                if now_above && !hit.0 {
                    say.push(format!("above {}", crate::finance::format_level(a)));
                }
                hit.0 = now_above;
            }
            if let Some(b) = w.below {
                let now_below = q.price <= b;
                if now_below && !hit.1 {
                    say.push(format!("below {}", crate::finance::format_level(b)));
                }
                hit.1 = now_below;
            }
            for s in say {
                let name = if w.name.is_empty() { w.symbol.clone() } else { w.name.clone() };
                let mut t =
                    self.tag(format!("{name} {s} · {}", crate::finance::format_price(q)), crate::theme::MEDIA, Icon::Chart, 3, 15_000);
                t.action = Some(("Open".into(), Action::SelectSlip(ModuleId::Finance)));
                self.push_tag(t, fx);
            }
        }
    }

    /// The next timed event starting within `secs`, if any.
    pub fn next_event_within(&self, secs: i64) -> Option<&crate::calendar::CalEvent> {
        let now = self.unix_now()?;
        self.calendar.iter().find(|e| !e.all_day && e.start >= now && e.start - now <= secs)
    }

    /// Every reminder of the linked events: (event, lead in seconds before its start).
    fn calendar_reminders(&self) -> Vec<(&crate::calendar::CalEvent, i64)> {
        let lead = self.config.calendar.reminder_min as i64 * 60;
        let mut out = Vec::new();
        for e in &self.calendar {
            let mut leads = e.alarms.clone();
            if lead > 0 && !e.all_day && !leads.contains(&lead) {
                leads.push(lead);
            }
            out.extend(leads.into_iter().map(|l| (e, l)));
        }
        out
    }

    fn schedule_calendar(&self, fx: &mut Vec<Effect>) {
        fx.push(Effect::KillTimer(TimerId::Calendar));
        let Some(now) = self.unix_now() else { return };
        let next = self
            .calendar_reminders()
            .into_iter()
            .filter(|(e, lead)| !self.calendar_reminded.contains(&(e.start, e.title.clone(), *lead)))
            .map(|(e, lead)| e.start - lead)
            .filter(|at| *at > now + CALENDAR_EARLY_S)
            .min();
        if let Some(at) = next {
            fx.push(Effect::SetTimer(TimerId::Calendar, ((at - now) as u64 * 1000).max(1)));
        }
    }

    /// Shows reminders that are due, including ones a late timer (sleep,
    /// a clock change) would otherwise skip, then waits for the next one.
    fn calendar_alerts(&mut self, fx: &mut Vec<Effect>) {
        let Some(now) = self.unix_now() else { return };
        let offset = self.clock.map(|c| c.utc_offset_min as i64 * 60).unwrap_or(0);
        let due: Vec<(crate::calendar::CalEvent, i64)> = self
            .calendar_reminders()
            .into_iter()
            .filter(|(e, lead)| {
                let at = e.start - lead;
                let pending = if e.all_day { e.end > now } else { e.start > now - 120 };
                at <= now + CALENDAR_EARLY_S && at > now - CALENDAR_LATE_S && pending
            })
            .filter(|(e, lead)| !self.calendar_reminded.contains(&(e.start, e.title.clone(), *lead)))
            .map(|(e, lead)| (e.clone(), lead))
            .collect();
        for (e, lead) in due {
            self.calendar_reminded.insert((e.start, e.title.clone(), lead));
            let when = if e.all_day {
                let day = |t: i64| (t + offset).div_euclid(86_400);
                crate::calendar::day_label(day(e.start), day(now)).to_lowercase()
            } else {
                let mins = ((e.start - now) as f64 / 60.0).round().max(0.0) as i64;
                match mins {
                    0 => "now".to_string(),
                    1..=90 => format!("in {mins} min"),
                    _ if mins < 36 * 60 => format!("in {} h", (mins as f64 / 60.0).round()),
                    _ => format!("in {} days", (mins as f64 / 1440.0).round()),
                }
            };
            // Reminders the user asked for show even over a fullscreen app.
            let mut t =
                self.tag(format!("{} · {when}", crate::format::ellipsize(&e.title, 40)), crate::theme::CODER, Icon::Calendar, 2, 15_000);
            if !e.link.is_empty() {
                t.action = Some(("Join".into(), Action::OpenUrl(e.link.clone())));
            }
            self.push_tag(t, fx);
        }
        self.schedule_calendar(fx);
    }

    /// A tag when a symbol crosses another step of `move_pct` since the previous close.
    fn price_moves(&mut self, list: &[crate::finance::Quote], fx: &mut Vec<Effect>) {
        let f = self.config.finance.clone();
        for q in list {
            let steps = crate::finance::move_steps(q.change_pct, f.move_pct as f64);
            let before = self.moves.get(&q.symbol).copied().unwrap_or(0);
            // Back near the close: a later move counts again.
            if steps == 0 {
                if q.change_pct.abs() < f.move_pct as f64 / 2.0 {
                    self.moves.remove(&q.symbol);
                }
                continue;
            }
            let further = steps.signum() != before.signum() || steps.abs() > before.abs();
            if !further {
                continue;
            }
            self.moves.insert(q.symbol.clone(), steps);
            if !f.price_alerts {
                continue;
            }
            let name = f
                .watchlist
                .iter()
                .find(|w| w.symbol.eq_ignore_ascii_case(&q.symbol) && !w.name.is_empty())
                .map(|w| w.name.clone())
                .unwrap_or_else(|| if q.name.is_empty() { q.symbol.clone() } else { q.name.clone() });
            let up = q.change_pct > 0.0;
            let text = format!(
                "{} {} {:.1}% · {}",
                crate::format::ellipsize(&name, 24),
                if up { "▲" } else { "▼" },
                q.change_pct.abs(),
                crate::finance::format_price(q)
            );
            let color = if up { crate::theme::OK } else { crate::theme::PRIVACY };
            let mut t = self.tag(text, color, Icon::Chart, 4, TAG_MS + 4000);
            t.action = Some(("Open".into(), Action::SelectSlip(ModuleId::Finance)));
            self.push_tag(t, fx);
        }
    }

    /// Keep the newest headlines; pop up the freshest one not seen before.
    fn new_headlines(&mut self, list: Vec<crate::finance::NewsItem>, fx: &mut Vec<Effect>) {
        let now = self.unix_now();
        let mut fresh: Vec<&crate::finance::NewsItem> = Vec::new();
        for n in &list {
            if self.news_seen.insert(n.id.clone()) && now.is_some_and(|now| now - n.time < crate::finance::NEWS_FRESH_S) {
                fresh.push(n);
            }
        }
        fresh.sort_by_key(|n| std::cmp::Reverse(n.time));
        if let (Some(n), true) = (fresh.first(), self.config.finance.news) {
            let more = fresh.len() - 1;
            let mut t = self.tag(
                format!("{} · {}", n.symbol.trim_start_matches('^'), crate::format::ellipsize(&n.title, 56)),
                crate::theme::GAME,
                Icon::Chart,
                5,
                12_000,
            );
            if more > 0 {
                t.detail = Some(format!("+{more}"));
            }
            t.action = Some(("Read".into(), Action::OpenUrl(n.link.clone())));
            self.push_tag(t, fx);
        }
        let mut list = list;
        list.sort_by_key(|n| std::cmp::Reverse(n.time));
        list.dedup_by(|a, b| a.id == b.id);
        list.truncate(8);
        self.news = list;
    }

    /// Unix seconds now, from the last clock sync.
    pub fn unix_now(&self) -> Option<i64> {
        self.clock.map(|c| c.unix + (self.now_ms.saturating_sub(c.at_ms) / 1000) as i64)
    }

    /// Alerts to list for the configured location.
    pub fn relevant_alerts(&self, limit: usize) -> Vec<crate::weather::Alert> {
        let l = &self.config.location;
        crate::weather::relevant(&self.alerts, l.latitude, l.longitude, self.config.weather.quake_radius_km as f64, limit)
    }

    fn urgent_alert(&self) -> bool {
        let (l, w) = (&self.config.location, &self.config.weather);
        self.alerts
            .iter()
            .any(|a| crate::weather::alert_priority(a, l.latitude, l.longitude, w.quake_radius_km as f64, w.quake_min_magnitude) == Some(1))
    }

    fn new_alerts(&mut self, list: Vec<crate::weather::Alert>, fx: &mut Vec<Effect>) {
        use crate::weather::Level;
        let unix = self.unix_now();
        let (l, w) = (self.config.location.clone(), self.config.weather.clone());
        for a in &list {
            if !self.alerts_seen.insert(a.id.clone()) {
                continue;
            }
            // Only announce what is recent; older items stay in the slip.
            let recent = unix.is_some_and(|now| now - a.time < 6 * 3600);
            if !w.alerts || !recent {
                continue;
            }
            if let Some(prio) = crate::weather::alert_priority(a, l.latitude, l.longitude, w.quake_radius_km as f64, w.quake_min_magnitude)
            {
                let km = crate::weather::distance_km(a.lat, a.lon, l.latitude, l.longitude);
                let lead = if a.tsunami { "Tsunami warning".to_string() } else { a.hazard.label().to_string() };
                let text = format!("{lead} · {} · {:.0} km", crate::format::ellipsize(&a.title, 40), km);
                let color = if prio == 1 || a.level == Level::Red { crate::theme::PRIVACY } else { crate::theme::MEDIA };
                let mut t = self.tag(text, color, Icon::Warning, prio, 15_000);
                t.action = Some(("Details".into(), Action::OpenUrl(a.url.clone())));
                self.push_tag(t, fx);
            }
        }
        self.alerts = list;
    }

    /// Local minutes after midnight now, from the last clock sync.
    pub fn local_minute(&self) -> Option<f64> {
        self.clock.map(|c| c.minutes + self.now_ms.saturating_sub(c.at_ms) as f64 / 60_000.0)
    }

    /// The next prayer is within `minutes`.
    fn prayer_soon(&self, minutes: f64) -> bool {
        match (self.prayer_day, self.local_minute()) {
            (Some(d), Some(now)) => d.next(now).is_some_and(|(_, t)| t - now <= minutes),
            _ => false,
        }
    }

    fn compute_prayer(&mut self, fx: &mut Vec<Effect>) {
        fx.push(Effect::KillTimer(TimerId::Prayer));
        let Some(c) = self.clock.filter(|_| self.enabled(ModuleId::Prayer)) else {
            self.prayer_day = None;
            return;
        };
        let (p, l) = (&self.config.prayer, &self.config.location);
        let day = crate::prayer::compute(c.year, c.month, c.day, l.latitude, l.longitude, c.utc_offset_min as f64 / 60.0, p.method, p.asr);
        self.prayer_announced.retain(|(date, _, _)| *date == (c.year, c.month, c.day));
        self.prayer_day = Some(day);
        // Catch up on a moment a late timer skipped, then wait for the next.
        self.prayer_alerts(fx);
    }

    /// Today's Hijri date (year, month, day).
    pub fn hijri(&self) -> Option<(i64, u32, u32)> {
        let c = self.clock?;
        Some(crate::prayer::hijri(c.year, c.month, c.day, self.config.prayer.hijri_adjust))
    }

    /// The day of Ramadan, while it is Ramadan.
    pub fn ramadan_day(&self) -> Option<u32> {
        self.hijri().filter(|h| h.1 == 9).map(|h| h.2)
    }

    /// In Ramadan: what the fast waits for next — "Imsak" before dawn,
    /// "Buka" (Maghrib) during the day — and when, in local minutes (past
    /// midnight for tomorrow's imsak).
    pub fn fasting(&self) -> Option<(&'static str, f64)> {
        use crate::prayer::Prayer;
        let day_n = self.ramadan_day()?;
        let (day, now) = (self.prayer_day?, self.local_minute()?);
        let (imsak, maghrib) = (day.get(Prayer::Imsak)?, day.get(Prayer::Maghrib)?);
        if now < imsak {
            Some(("Imsak", imsak))
        } else if now < maghrib {
            Some(("Buka", maghrib))
        } else if day_n < 29 {
            Some(("Imsak", imsak + 1440.0))
        } else {
            None
        }
    }

    /// Reminder and prayer moments today, in local minutes (with imsak in Ramadan).
    fn prayer_moments(&self) -> Vec<(f64, crate::prayer::Prayer, bool)> {
        let Some(day) = self.prayer_day else { return Vec::new() };
        let before = self.config.prayer.reminder_min as f64;
        let mut v = Vec::new();
        let mut prayers = crate::prayer::Prayer::FIVE.to_vec();
        if self.ramadan_day().is_some() {
            prayers.insert(0, crate::prayer::Prayer::Imsak);
        }
        for p in prayers {
            if let Some(t) = day.get(p) {
                v.push((t.ceil(), p, true));
                if before > 0.0 {
                    v.push((t.ceil() - before, p, false));
                }
            }
        }
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v
    }

    fn schedule_prayer(&self, fx: &mut Vec<Effect>) {
        let Some(now) = self.local_minute() else { return };
        if let Some((t, _, _)) = self.prayer_moments().into_iter().find(|(t, _, _)| *t > now + 0.05) {
            fx.push(Effect::SetTimer(TimerId::Prayer, ((t - now) * 60_000.0).ceil().max(1.0) as u64));
        }
    }

    fn prayer_alerts(&mut self, fx: &mut Vec<Effect>) {
        fx.push(Effect::KillTimer(TimerId::Prayer));
        let (Some(now), Some(c)) = (self.local_minute(), self.clock) else { return };
        let date = (c.year, c.month, c.day);
        let before = self.config.prayer.reminder_min as f64;
        // Due now, or missed by a timer that woke late: the time itself is
        // still worth a tag for a while, a reminder only before the prayer.
        let due: Vec<_> = self
            .prayer_moments()
            .into_iter()
            .filter(|(t, _, at_time)| *t <= now + 0.05 && if *at_time { now - t < PRAYER_LATE_MIN } else { t + before > now })
            .filter(|(t, _, at_time)| !self.prayer_announced.contains(&(date, *t as i64, *at_time)))
            .collect();
        for (t, p, at_time) in due {
            self.prayer_announced.insert((date, t as i64, at_time));
            let ramadan = self.ramadan_day().is_some();
            let text = if at_time && ramadan && p == crate::prayer::Prayer::Maghrib {
                format!("Waktu berbuka · {}", crate::prayer::hhmm(t))
            } else if at_time && p == crate::prayer::Prayer::Imsak {
                format!("Imsak · {}", crate::prayer::hhmm(t))
            } else if at_time {
                format!("Waktu {} · {}", p.name(), crate::prayer::hhmm(t))
            } else {
                let left = (t + before - now).round().max(1.0);
                format!("{} in {left:.0} min", p.name())
            };
            // Prayer times show even over a fullscreen app or a focus round.
            let tag = self.tag(text, crate::theme::OK, Icon::Moon, 2, if at_time { 15_000 } else { 10_000 });
            self.push_tag(tag, fx);
        }
        self.schedule_prayer(fx);
    }

    fn desired_samplers(&self) -> BTreeSet<Sampler> {
        let mut s = BTreeSet::new();
        let visible = matches!(self.state, IslandState::Tag | IslandState::Slips);
        if visible {
            s.insert(Sampler::Second);
        }
        let slip = |m| self.state == IslandState::Slips && self.front == Some(m);
        let game_peek = self.state == IslandState::Tag && self.hover_tag && self.game.is_some();
        if self.enabled(ModuleId::Game) && (slip(ModuleId::Game) || game_peek) && self.game.is_some() {
            s.insert(Sampler::Gpu);
            s.insert(Sampler::Ping);
        }
        if slip(ModuleId::Mixer) {
            s.insert(Sampler::Peaks);
        }
        if slip(ModuleId::Ports) {
            s.insert(Sampler::Ports);
        }
        if slip(ModuleId::Finance) && !self.config.finance.watchlist.is_empty() {
            s.insert(Sampler::Finance);
        }
        s
    }

    /// Something is playing (the notch shows it instead of the glances).
    pub fn playing(&self) -> bool {
        self.front_media().is_some_and(|s| s.playing)
    }

    /// The glances that have something to show, in order: weather, the
    /// markets, the to-do list, the calendar, prayer times.
    pub fn glances(&self) -> Vec<Glance> {
        let mut v = Vec::new();
        if self.weather.is_some() {
            v.push(Glance::Weather);
        }
        if self.enabled(ModuleId::Finance) {
            let quoted = self
                .config
                .finance
                .watchlist
                .iter()
                .enumerate()
                .filter(|(_, w)| self.quotes.iter().any(|q| q.symbol.eq_ignore_ascii_case(&w.symbol)))
                .map(|(i, _)| Glance::Quote(i))
                .take(3);
            v.extend(quoted);
        }
        if self.enabled(ModuleId::Todo) && self.todos.iter().any(|t| !t.done) {
            v.push(Glance::Todo);
        }
        if self.next_event_within(36 * 3600).is_some() {
            v.push(Glance::Calendar);
        }
        // After Isya there is no prayer left today: nothing to show until midnight.
        let next_prayer = self.prayer_day.zip(self.local_minute()).is_some_and(|(d, t)| d.next(t).is_some());
        if self.fasting().is_some() || next_prayer {
            v.push(Glance::Prayer);
        }
        v
    }

    /// The glance the notch shows now.
    pub fn glance(&self) -> Option<Glance> {
        let g = self.glances();
        (!g.is_empty()).then(|| g[self.carousel % g.len()])
    }

    /// The notch turns through its glances while it rests and nothing plays.
    fn reconcile_carousel(&mut self, fx: &mut Vec<Effect>) {
        let want = self.config.dock.style == crate::config::Style::Notch
            && self.state == IslandState::Orb
            && !self.playing()
            && self.glances().len() > 1;
        if want && !self.carousel_armed {
            fx.push(Effect::SetTimer(TimerId::Carousel, CAROUSEL_MS));
        } else if !want && self.carousel_armed {
            fx.push(Effect::KillTimer(TimerId::Carousel));
        }
        self.carousel_armed = want;
    }

    fn reconcile_samplers(&mut self, fx: &mut Vec<Effect>) {
        let want = self.desired_samplers();
        for gone in self.samplers.difference(&want) {
            fx.push(Effect::StopSampler(*gone));
        }
        for new in want.difference(&self.samplers) {
            fx.push(Effect::StartSampler(*new));
        }
        self.samplers = want;
    }

    /// Samplers currently running (for tests and the platform's sanity checks).
    pub fn samplers(&self) -> &BTreeSet<Sampler> {
        &self.samplers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPU: Signals = Signals { exclusive_d3d: false, gpu_3d_pct: Some(60.0) };

    fn model() -> Model {
        Model::new(Config::default(), 7, 0)
    }

    fn session(id: &str, title: &str, playing: bool) -> MediaSession {
        MediaSession {
            id: id.into(),
            app: id.into(),
            title: title.into(),
            artist: "Artist".into(),
            playing,
            duration_ms: 200_000,
            can_play_pause: true,
            can_next: true,
            can_prev: true,
            ..Default::default()
        }
    }

    fn has_timer(fx: &[Effect], id: TimerId) -> bool {
        fx.iter().any(|e| matches!(e, Effect::SetTimer(t, _) if *t == id))
    }

    #[test]
    fn hover_shows_summary_tag_then_click_opens_slips() {
        let mut m = model();
        m.update(0, Event::Media(vec![session("Spotify.exe", "Song", true)]));
        m.update(10, Event::Timer(TimerId::TagHide)); // let the track tag settle
        m.update(5000, Event::Timer(TimerId::TagHide));
        assert_eq!(m.state, IslandState::Orb);
        let fx = m.update(5000, Event::PointerEnter);
        assert!(has_timer(&fx, TimerId::HoverDelay));
        m.update(5120, Event::Timer(TimerId::HoverDelay));
        assert_eq!(m.state, IslandState::Tag);
        assert!(m.hover_tag);
        m.update(5200, Event::Click(Action::ToggleSlips));
        assert_eq!(m.state, IslandState::Slips);
        assert_eq!(m.front, Some(ModuleId::Media));
        let fx = m.update(5300, Event::PointerLeave);
        assert!(has_timer(&fx, TimerId::Collapse));
        m.update(5900, Event::Timer(TimerId::Collapse));
        assert_eq!(m.state, IslandState::Orb);
    }

    #[test]
    fn todo_add_toggle_remove() {
        let mut m = model();
        assert!(m.enabled(ModuleId::Todo), "on by default");
        m.update(0, Event::Click(Action::ToggleSlips));
        let fx = m.update(0, Event::Click(Action::TodoAdd));
        assert!(fx.contains(&Effect::TakeFocus), "typing needs the keyboard");
        assert_eq!(m.front, Some(ModuleId::Todo));
        m.update(0, Event::Text("  write release notes\u{7}".into()));
        m.update(0, Event::Text("x".into()));
        m.update(0, Event::Key(Key::Backspace));
        let fx = m.update(0, Event::Key(Key::Enter));
        assert!(fx.contains(&Effect::ReleaseFocus));
        assert!(m.input.is_none());
        assert_eq!(m.state, IslandState::Slips, "Enter adds, it does not close the island");
        assert!(fx.contains(&Effect::SaveTodos));
        m.update(0, Event::TodoAdded("".into()));
        m.update(0, Event::TodoAdded("review PR".into()));
        assert_eq!(m.todos.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["review PR", "write release notes"]);
        let id = m.todos[1].id;
        m.update(0, Event::Click(Action::TodoToggle(id)));
        assert!(m.todos[1].done);
        m.update(0, Event::Click(Action::TodoClearDone));
        assert_eq!(m.todos.len(), 1);
        let id = m.todos[0].id;
        m.update(0, Event::Click(Action::TodoRemove(id)));
        assert!(m.todos.is_empty());
        assert!(m.available_slips().contains(&ModuleId::Todo), "always there to add tasks");
    }

    #[test]
    fn custom_timer_steps_and_saves() {
        let mut m = model();
        assert_eq!(m.config.timer.custom_min, 15);
        let fx = m.update(0, Event::Click(Action::TimerCustom(1)));
        assert_eq!(m.config.timer.custom_min, 20);
        assert!(fx.contains(&Effect::SaveValue("timer", "custom_min", "20".into())));
        for _ in 0..3 {
            m.update(0, Event::Click(Action::TimerCustom(-1)));
        }
        assert_eq!(m.config.timer.custom_min, 9, "20 → 15 → 10 → 9");
        for _ in 0..20 {
            m.update(0, Event::Click(Action::TimerCustom(-1)));
        }
        assert_eq!(m.config.timer.custom_min, 1);
        let fx = m.update(0, Event::Click(Action::TimerStart(m.config.timer.custom_min)));
        assert!(has_timer(&fx, TimerId::FocusDone));
    }

    #[test]
    fn one_media_tag_at_a_time() {
        let mut m = model();
        m.update(0, Event::Media(vec![session("a", "One", true)]));
        m.update(100, Event::Media(vec![session("a", "Two", true)]));
        assert_eq!(m.tags.iter().filter(|t| t.media).count(), 1);
        assert!(m.current_tag().unwrap().text.starts_with("Two"));
    }

    #[test]
    fn weather_change_pops_up_and_stale_weather_refreshes_on_open() {
        let mut m = model();
        m.update(0, Event::Clock { year: 2026, month: 10, day: 25, minutes: 600.0, utc_offset_min: 420, unix: 1_800_000_000 });
        let mut w = crate::weather::parse_open_meteo(include_str!("../testdata/openmeteo.json"), "Jakarta", 1_800_000_000).unwrap();
        m.update(0, Event::Weather(w.clone()));
        assert!(m.tags.is_empty(), "the first reading is quiet");
        w.condition =
            if w.condition == crate::weather::Condition::Rain { crate::weather::Condition::Clear } else { crate::weather::Condition::Rain };
        w.description = "Hujan".into();
        m.update(1000, Event::Weather(w));
        assert!(m.current_tag().unwrap().text.starts_with("Hujan ·"));
        m.tags.clear();
        m.update(2000, Event::Timer(TimerId::TagHide));
        // Forty minutes later the slip asks for fresh data when it opens.
        m.update(40 * 60_000, Event::Click(Action::ToggleSlips));
        let fx = m.update(40 * 60_000, Event::Click(Action::SelectSlip(ModuleId::Weather)));
        assert!(fx.contains(&Effect::RefreshWeather));
    }

    #[test]
    fn hover_without_content_stays_orb() {
        let mut cfg = Config::default();
        for id in ModuleId::ALL {
            cfg.modules.set(id, false);
        }
        let mut m = Model::new(cfg, 1, 0);
        m.update(0, Event::PointerEnter);
        m.update(200, Event::Timer(TimerId::HoverDelay));
        assert_eq!(m.state, IslandState::Orb);
        m.update(300, Event::Click(Action::ToggleSlips));
        assert_eq!(m.state, IslandState::Orb, "nothing to show");
    }

    #[test]
    fn reentering_cancels_collapse() {
        let mut m = model();
        m.update(0, Event::Click(Action::ToggleSlips));
        assert_eq!(m.state, IslandState::Slips);
        m.update(10, Event::PointerLeave);
        m.update(20, Event::PointerEnter);
        m.update(700, Event::Timer(TimerId::Collapse));
        assert_eq!(m.state, IslandState::Slips);
    }

    #[test]
    fn game_sinks_to_seam_and_only_urgent_tags_show() {
        let mut m = model();
        let fx = m.update(
            0,
            Event::Fullscreen { active: true, exe: "D:\\steamapps\\common\\Doom\\Doom.exe".into(), signals: Signals::default() },
        );
        assert_eq!(m.state, IslandState::Seam);
        assert!(fx.iter().any(|e| matches!(e, Effect::GameStarted(x) if x == "doom.exe")));
        assert!(has_timer(&fx, TimerId::Break));
        m.update(10, Event::Pipe(Request::Done { title: None, id: "build".into(), ok: true, code: Some(0), duration_s: None, hwnd: None }));
        assert_eq!(m.state, IslandState::Seam, "success tags wait during games");
        m.update(20, Event::Privacy(vec![(Device::Microphone, "Discord".into())]));
        assert_eq!(m.state, IslandState::Tag, "privacy always shows");
        m.update(7000, Event::Timer(TimerId::TagHide));
        assert_eq!(m.state, IslandState::Seam);
    }

    #[test]
    fn alt_tab_within_grace_keeps_one_session() {
        let mut m = model();
        m.update(0, Event::Fullscreen { active: true, exe: "game.exe".into(), signals: GPU });
        let fx = m.update(60_000, Event::Fullscreen { active: false, exe: String::new(), signals: Signals::default() });
        assert!(has_timer(&fx, TimerId::GameGrace));
        assert_eq!(m.state, IslandState::Orb);
        let fx = m.update(90_000, Event::Fullscreen { active: true, exe: "game.exe".into(), signals: GPU });
        assert!(!fx.iter().any(|e| matches!(e, Effect::GameStarted(_))));
        assert_eq!(m.game.as_ref().unwrap().started_ms, 0);
        m.update(200_000, Event::Fullscreen { active: false, exe: String::new(), signals: Signals::default() });
        let fx = m.update(260_000, Event::Timer(TimerId::GameGrace));
        assert!(fx.contains(&Effect::GameEnded));
        assert_eq!(m.playlog.minutes["game.exe"], 4);
    }

    #[test]
    fn closing_the_game_ends_the_session_at_once() {
        let mut m = model();
        m.update(0, Event::Fullscreen { active: true, exe: "game.exe".into(), signals: GPU });
        m.update(120_000, Event::Fullscreen { active: false, exe: String::new(), signals: Signals::default() });
        assert!(m.game.is_some(), "alt-tab grace");
        let fx = m.update(121_000, Event::GameExited);
        assert!(m.game.is_none());
        assert!(fx.contains(&Effect::GameEnded));
        assert_eq!(m.playlog.minutes["game.exe"], 2);
    }

    #[test]
    fn browser_fullscreen_is_not_a_game() {
        let mut m = model();
        let fx = m.update(0, Event::Fullscreen { active: true, exe: "chrome.exe".into(), signals: GPU });
        assert_eq!(m.state, IslandState::Seam);
        assert!(m.game.is_none());
        assert!(!fx.iter().any(|e| matches!(e, Effect::GameStarted(_))));
    }

    #[test]
    fn game_module_off_still_seams_but_no_session() {
        let mut m = model();
        m.update(0, Event::SetModule(ModuleId::Game, false));
        m.update(0, Event::Fullscreen { active: true, exe: "game.exe".into(), signals: GPU });
        assert_eq!(m.state, IslandState::Seam);
        assert!(m.game.is_none());
    }

    #[test]
    fn command_done_and_failed_tags() {
        let mut m = model();
        m.update(
            0,
            Event::Pipe(Request::Notify {
                id: None,
                title: "cargo test".into(),
                body: None,
                level: Level::Info,
                progress: None,
                sticky: true,
                ttl_ms: None,
                hwnd: None,
            }),
        );
        assert!(matches!(m.jobs[0].status, JobStatus::Running { .. }));
        m.update(
            134_000,
            Event::Pipe(Request::Done { title: None, id: "cargo test".into(), ok: true, code: Some(0), duration_s: None, hwnd: None }),
        );
        let tag = m.current_tag().unwrap();
        assert_eq!(tag.detail.as_deref(), Some("2m14s"));
        m.update(
            140_000,
            Event::Pipe(Request::Done {
                title: None,
                id: "npm run build".into(),
                ok: false,
                code: Some(1),
                duration_s: Some(3.0),
                hwnd: Some("0x10".into()),
            }),
        );
        let tag = m.current_tag().unwrap();
        assert_eq!(tag.priority, 3);
        assert_eq!(tag.detail.as_deref(), Some("exit 1"));
        let (_, action) = tag.action.clone().unwrap();
        let fx = m.update(141_000, Event::Click(action));
        assert!(fx.contains(&Effect::FocusWindow(0x10)));
        assert_eq!(m.most_urgent_slip(), Some(ModuleId::Commands));
    }

    #[test]
    fn done_without_notify_uses_title() {
        let mut m = model();
        m.update(
            0,
            Event::Pipe(Request::Done {
                title: Some("npm run build".into()),
                id: "npm run build".into(),
                ok: true,
                code: None,
                duration_s: Some(31.0),
                hwnd: None,
            }),
        );
        assert_eq!(m.jobs[0].title, "npm run build");
        assert_eq!(m.jobs[0].id, "npm-run-build");
    }

    #[test]
    fn jobs_are_capped() {
        let mut m = model();
        for i in 0..20 {
            m.update(
                i,
                Event::Pipe(Request::Done { title: None, id: format!("job {i}"), ok: true, code: None, duration_s: None, hwnd: None }),
            );
        }
        assert_eq!(m.jobs.len(), MAX_JOBS);
        assert_eq!(m.jobs[0].id, "job-19");
    }

    #[test]
    fn port_stop_needs_confirmation() {
        let mut m = model();
        m.update(
            0,
            Event::Ports(vec![
                PortEntry { port: 3000, pid: 100, process: "node.exe".into(), can_stop: true },
                PortEntry { port: 135, pid: 900, process: "svchost.exe".into(), can_stop: false },
                PortEntry { port: 5040, pid: 901, process: "svchost.exe".into(), can_stop: false },
            ]),
        );
        assert_eq!(m.ports.len(), 1, "low and hidden ports filtered");
        let fx = m.update(10, Event::Click(Action::PortStop(3000)));
        assert!(!fx.iter().any(|e| matches!(e, Effect::Kill(_))));
        let fx = m.update(20, Event::Click(Action::PortStop(3000)));
        assert!(fx.contains(&Effect::Kill(100)));
        // Confirmation expires.
        m.update(30, Event::Click(Action::PortStop(3000)));
        m.update(4000, Event::Timer(TimerId::ConfirmExpire));
        let fx = m.update(4010, Event::Click(Action::PortStop(3000)));
        assert!(!fx.iter().any(|e| matches!(e, Effect::Kill(_))));
    }

    #[test]
    fn clipboard_detection_and_actions() {
        let mut m = model();
        m.update(0, Event::Clipboard { text: "#ff8800".into(), unix_now: 0 });
        assert!(m.clip.is_some());
        let fx = m.update(10, Event::Click(Action::Clip(ClipAction::Rgb)));
        assert!(fx.contains(&Effect::CopyText("rgb(255, 136, 0)".into())));
        let fx = m.update(20, Event::Click(Action::Clip(ClipAction::NewUuid)));
        let uuids: Vec<_> = fx.iter().filter_map(|e| if let Effect::CopyText(t) = e { Some(t.clone()) } else { None }).collect();
        assert_eq!(uuids[0].len(), 36);
        m.update(30, Event::Clipboard { text: "plain words".into(), unix_now: 0 });
        assert!(m.clip.is_none());
    }

    #[test]
    fn focus_timer_lifecycle_and_quiet() {
        let mut m = model();
        let fx = m.update(0, Event::Click(Action::TimerStart(25)));
        assert!(fx.contains(&Effect::SetTimer(TimerId::FocusDone, 25 * 60_000)));
        m.update(60_000, Event::Click(Action::TimerPause));
        assert_eq!(m.timer.remaining(999_999), Some(24 * 60_000));
        m.update(120_000, Event::Click(Action::TimerResume));
        assert_eq!(m.timer.remaining(120_000), Some(24 * 60_000));
        // Low-priority tags are held back while a round runs.
        m.update(130_000, Event::Capture(Capture { path: "a.png".into(), name: "a.png".into(), kind: CaptureKind::Screenshot, at_ms: 0 }));
        assert!(m.current_tag().is_none());
        m.update(25 * 60_000 + 60_000, Event::Timer(TimerId::FocusDone));
        assert!(m.on_break, "a break follows");
        assert_eq!(m.current_tag().unwrap().text, "Round 1 done · 5 min break");
    }

    #[test]
    fn wheel_changes_volume_and_flips_slips() {
        let mut m = model();
        m.mixer.master.level = 0.5;
        let fx = m.update(0, Event::Wheel { notches: 2, shift: false });
        assert!(fx.contains(&Effect::SetMasterVolume(0.54)));
        let fx = m.update(0, Event::Wheel { notches: -1, shift: true });
        assert!(fx.is_empty() || !fx.iter().any(|e| matches!(e, Effect::Media { .. })), "no session, no command");
        m.update(0, Event::Click(Action::ToggleSlips));
        let first = m.front;
        m.update(0, Event::Wheel { notches: -1, shift: false });
        assert_ne!(m.front, first);
    }

    #[test]
    fn media_controls_target_selected_source() {
        let mut m = model();
        m.update(0, Event::Media(vec![session("Spotify.exe", "A", true), session("Chrome", "B", false)]));
        m.update(0, Event::Click(Action::MediaSource(1)));
        let fx = m.update(0, Event::Click(Action::MediaPlayPause));
        assert!(fx.contains(&Effect::Media { session: "Chrome".into(), cmd: MediaCmd::PlayPause }));
        assert!(m.media[1].playing, "optimistic toggle");
        let fx = m.update(0, Event::MiddleClick);
        assert!(fx.iter().any(|e| matches!(e, Effect::Media { session, .. } if session == "Chrome")));
    }

    #[test]
    fn no_media_session_falls_back_to_media_key() {
        let mut m = model();
        let fx = m.update(0, Event::MiddleClick);
        assert!(fx.contains(&Effect::MediaKey));
    }

    #[test]
    fn mixer_rows_know_which_apps_have_controls() {
        let mut m = model();
        m.update(0, Event::Media(vec![session("Spotify.exe", "A", true)]));
        m.update(
            0,
            Event::Mixer(MixerSnapshot {
                device: "Headphones".into(),
                master: Volume { level: 0.6, muted: false },
                apps: vec![
                    AudioApp { pid: 1, name: "Spotify".into(), exe: "spotify.exe".into(), ..Default::default() },
                    AudioApp { pid: 2, name: "Game".into(), exe: "game.exe".into(), ..Default::default() },
                ],
            }),
        );
        assert!(m.mixer.apps[0].has_controls);
        assert!(!m.mixer.apps[1].has_controls);
        let fx = m.update(0, Event::Slider(SliderTarget::App(2), 0.25));
        assert!(fx.contains(&Effect::SetAppVolume(2, 0.25)));
        let fx = m.update(0, Event::Click(Action::AppMute(2)));
        assert!(fx.contains(&Effect::SetAppMute(2, true)));
    }

    #[test]
    fn samplers_run_only_while_visible() {
        let mut m = model();
        assert!(m.samplers().is_empty());
        m.update(0, Event::Click(Action::ToggleSlips));
        m.update(0, Event::Click(Action::SelectSlip(ModuleId::Mixer)));
        assert!(m.samplers().contains(&Sampler::Peaks));
        assert!(m.samplers().contains(&Sampler::Second));
        m.update(0, Event::Click(Action::SelectSlip(ModuleId::Ports)));
        assert!(!m.samplers().contains(&Sampler::Peaks));
        assert!(m.samplers().contains(&Sampler::Ports));
        let fx = m.update(0, Event::Click(Action::Collapse));
        assert!(fx.contains(&Effect::StopSampler(Sampler::Ports)));
        assert!(m.samplers().is_empty());
    }

    #[test]
    fn hotkey_takes_and_releases_focus() {
        let mut m = model();
        let fx = m.update(0, Event::Hotkey);
        assert!(fx.contains(&Effect::TakeFocus));
        assert!(m.keyboard_open);
        let fx = m.update(10, Event::Key(Key::Escape));
        assert!(fx.contains(&Effect::ReleaseFocus));
        assert_eq!(m.state, IslandState::Orb);
        m.update(20, Event::Hotkey);
        m.update(30, Event::FocusLost);
        assert_eq!(m.state, IslandState::Orb);
    }

    #[test]
    fn disabling_a_module_clears_it_and_its_slip() {
        let mut m = model();
        m.update(0, Event::Media(vec![session("a", "A", true)]));
        m.update(0, Event::Click(Action::ToggleSlips));
        assert_eq!(m.front, Some(ModuleId::Media));
        let fx = m.update(0, Event::SetModule(ModuleId::Media, false));
        assert!(fx.contains(&Effect::SaveModule(ModuleId::Media, false)));
        assert!(m.media.is_empty());
        assert_ne!(m.front, Some(ModuleId::Media));
        assert!(!m.available_slips().contains(&ModuleId::Media));
        // Events for disabled modules are ignored.
        m.update(0, Event::Media(vec![session("a", "A", true)]));
        assert!(m.media.is_empty());
    }

    #[test]
    fn privacy_comes_first_and_tags_once() {
        let mut m = model();
        m.update(0, Event::Privacy(vec![(Device::Camera, "Zoom".into())]));
        assert_eq!(m.most_urgent_slip(), Some(ModuleId::Privacy));
        let n = m.tags.len();
        m.update(10, Event::Privacy(vec![(Device::Camera, "Zoom".into())]));
        assert_eq!(m.tags.len(), n);
        assert_eq!(m.privacy[0].since_ms, 0);
        m.update(20, Event::Privacy(vec![]));
        assert!(m.privacy.is_empty());
    }

    #[test]
    fn pause_hides_everything() {
        let mut m = model();
        m.update(0, Event::Pause(true));
        assert_eq!(m.state, IslandState::Hidden);
        m.update(0, Event::Capture(Capture { path: "a.png".into(), name: "a".into(), kind: CaptureKind::Screenshot, at_ms: 0 }));
        assert_eq!(m.state, IslandState::Hidden);
        m.update(0, Event::Pause(false));
        assert_eq!(m.state, IslandState::Orb);
    }

    #[test]
    fn break_reminder_repeats_and_snoozes() {
        let mut m = model();
        m.update(0, Event::Fullscreen { active: true, exe: "g.exe".into(), signals: GPU });
        let fx = m.update(90 * 60_000, Event::Timer(TimerId::Break));
        assert!(m.current_tag().unwrap().text.starts_with("1h 30m played"));
        assert!(fx.contains(&Effect::SetTimer(TimerId::Break, BREAK_REPEAT_MS)));
        let fx = m.update(90 * 60_000 + 1, Event::Click(Action::BreakSnooze));
        assert!(fx.contains(&Effect::SetTimer(TimerId::Break, SNOOZE_MS)));
    }

    #[test]
    fn snipping_tool_changes_nothing() {
        let mut m = model();
        let fx = m.update(0, Event::Fullscreen { active: true, exe: "C:\\Windows\\System32\\SnippingTool.exe".into(), signals: GPU });
        assert_eq!(m.state, IslandState::Orb);
        assert!(m.game.is_none());
        assert!(!fx.iter().any(|e| matches!(e, Effect::GameStarted(_))));
    }

    #[test]
    fn plain_fullscreen_app_is_not_a_game() {
        let mut m = model();
        m.update(0, Event::Fullscreen { active: true, exe: "C:\\Tools\\slides.exe".into(), signals: Signals::default() });
        assert_eq!(m.state, IslandState::Seam);
        assert!(m.game.is_none());
    }

    #[test]
    fn snooze_gives_feedback_and_moves_the_reminder() {
        let mut m = model();
        m.update(0, Event::Fullscreen { active: true, exe: "g.exe".into(), signals: GPU });
        assert_eq!(m.break_due_ms, Some(90 * 60_000));
        m.update(90 * 60_000, Event::Timer(TimerId::Break));
        let (_, action) = m.current_tag().unwrap().action.clone().unwrap();
        assert_eq!(action, Action::BreakSnooze);
        let now = 90 * 60_000 + 5;
        m.update(now, Event::Click(action));
        assert_eq!(m.break_due_ms, Some(now + SNOOZE_MS));
        assert_eq!(m.current_tag().unwrap().text, "Break snoozed · 15 min");
        assert!(!m.tags.iter().any(|t| t.text.contains("time for a break")));
    }

    #[test]
    fn slips_opened_from_the_tray_close_by_themselves() {
        let mut m = model();
        let fx = m.update(0, Event::Click(Action::ToggleSlips));
        assert_eq!(m.state, IslandState::Slips);
        assert!(fx.contains(&Effect::SetTimer(TimerId::Collapse, UNATTENDED_MS)));
        m.update(UNATTENDED_MS, Event::Timer(TimerId::Collapse));
        assert_eq!(m.state, IslandState::Orb);
    }

    #[test]
    fn clicking_elsewhere_closes_the_slips() {
        let mut m = model();
        m.update(0, Event::PointerEnter);
        m.update(10, Event::Click(Action::ToggleSlips));
        m.update(20, Event::PointerLeave);
        m.update(30, Event::OutsideFocus);
        assert_eq!(m.state, IslandState::Orb);
        // Not while the pointer is still on the island.
        m.update(40, Event::PointerEnter);
        m.update(50, Event::Click(Action::ToggleSlips));
        m.update(60, Event::OutsideFocus);
        assert_eq!(m.state, IslandState::Slips);
    }

    #[test]
    fn prayer_reminders_fire_on_time() {
        let mut m = model();
        m.update(0, Event::SetModule(ModuleId::Prayer, true));
        // 11:00 local in Jakarta.
        let fx = m.update(0, Event::Clock { year: 2025, month: 1, day: 1, minutes: 11.0 * 60.0, utc_offset_min: 420, unix: 1_735_704_000 });
        let dzuhur = m.prayer_day.unwrap().get(crate::prayer::Prayer::Dzuhur).unwrap().ceil();
        let wait = fx.iter().find_map(|e| if let Effect::SetTimer(TimerId::Prayer, ms) = e { Some(*ms) } else { None }).unwrap();
        assert_eq!(wait, ((dzuhur - 10.0 - 660.0) * 60_000.0) as u64, "reminder 10 min before Dzuhur");
        m.update(wait, Event::Timer(TimerId::Prayer));
        assert_eq!(m.current_tag().unwrap().text, "Dzuhur in 10 min");
        let at = wait + 10 * 60_000;
        m.update(at, Event::Timer(TimerId::Prayer));
        assert!(m.tags.iter().any(|t| t.text.starts_with("Waktu Dzuhur")));
        assert!(m.available_slips().contains(&ModuleId::Prayer));
        // Off by default.
        assert!(!Config::default().modules.prayer);
    }

    #[test]
    fn prayer_reminders_show_over_fullscreen_and_after_a_late_timer() {
        let mut m = model();
        m.update(0, Event::SetModule(ModuleId::Prayer, true));
        m.update(0, Event::Clock { year: 2025, month: 1, day: 1, minutes: 11.0 * 60.0, utc_offset_min: 420, unix: 1_735_704_000 });
        let dzuhur = m.prayer_day.unwrap().get(crate::prayer::Prayer::Dzuhur).unwrap().ceil();
        m.update(10, Event::Fullscreen { active: true, exe: "C:\\Tools\\video.exe".into(), signals: Signals::default() });
        assert_eq!(m.state, IslandState::Seam);
        // The computer slept: the timer fires five minutes late.
        let late = ((dzuhur - 5.0 - 660.0) * 60_000.0) as u64;
        m.update(late, Event::Timer(TimerId::Prayer));
        let t = m.current_tag().expect("reminder despite the fullscreen app");
        assert_eq!(t.text, "Dzuhur in 5 min");
        assert_eq!(m.state, IslandState::Tag);
        // A clock sync right after does not repeat it.
        let n = m.tags.len();
        m.update(late + 10, Event::Clock { year: 2025, month: 1, day: 1, minutes: dzuhur - 5.0, utc_offset_min: 420, unix: 1_735_704_000 });
        assert_eq!(m.tags.len(), n);
        // No timer at all (modern standby): the next clock sync catches the prayer time.
        let after = late + 7 * 60_000;
        m.update(after, Event::Clock { year: 2025, month: 1, day: 1, minutes: dzuhur + 2.0, utc_offset_min: 420, unix: 1_735_704_000 });
        assert!(m.tags.iter().any(|t| t.text.starts_with("Waktu Dzuhur")), "{:?}", m.tags);
    }

    fn event(title: &str, start: i64, alarms: Vec<i64>) -> crate::calendar::CalEvent {
        crate::calendar::CalEvent {
            title: title.into(),
            location: String::new(),
            start,
            end: start + 3600,
            all_day: false,
            feed: 0,
            alarms,
            link: String::new(),
        }
    }

    #[test]
    fn calendar_reminders_use_the_events_own_alarms() {
        let mut m = model();
        let unix = 1_792_911_900;
        m.update(0, Event::Clock { year: 2026, month: 10, day: 25, minutes: 14.0 * 60.0, utc_offset_min: 420, unix });
        let fx = m.update(0, Event::Calendar(vec![event("Dentist", unix + 3600, vec![1800])]));
        let wait = fx.iter().find_map(|e| if let Effect::SetTimer(TimerId::Calendar, ms) = e { Some(*ms) } else { None }).unwrap();
        assert_eq!(wait, 1800 * 1000, "the event's own 30-minute reminder comes first");
        m.update(5, Event::Fullscreen { active: true, exe: "C:\\Tools\\video.exe".into(), signals: Signals::default() });
        m.update(wait, Event::Timer(TimerId::Calendar));
        assert_eq!(m.current_tag().unwrap().text, "Dentist · in 30 min");
        // Then the configured 10 minutes before.
        m.update(wait + 20 * 60_000, Event::Timer(TimerId::Calendar));
        assert!(m.tags.iter().any(|t| t.text == "Dentist · in 10 min"), "{:?}", m.tags);
        // A refresh of the feed repeats nothing.
        let n = m.tags.len();
        m.update(wait + 20 * 60_000 + 10, Event::Calendar(vec![event("Dentist", unix + 3600, vec![1800])]));
        assert_eq!(m.tags.len(), n);
    }

    #[test]
    fn calendar_event_added_close_to_its_start_is_reminded_at_once() {
        let mut m = model();
        let unix = 1_792_911_900;
        m.update(0, Event::Clock { year: 2026, month: 10, day: 25, minutes: 14.0 * 60.0, utc_offset_min: 420, unix });
        m.update(0, Event::Calendar(vec![event("Standup", unix + 4 * 60, vec![])]));
        assert_eq!(m.current_tag().unwrap().text, "Standup · in 4 min");
        // Long past events stay quiet.
        let mut m = model();
        m.update(0, Event::Clock { year: 2026, month: 10, day: 25, minutes: 14.0 * 60.0, utc_offset_min: 420, unix });
        m.update(0, Event::Calendar(vec![event("Old", unix - 3600, vec![])]));
        assert!(m.current_tag().is_none());
    }

    #[test]
    fn moved_to_another_display_the_island_stays_an_orb() {
        let mut m = model();
        m.update(0, Event::Displaced(true));
        m.update(0, Event::Fullscreen { active: true, exe: "C:\\Tools\\video.exe".into(), signals: Signals::default() });
        assert_eq!(m.state, IslandState::Orb, "no need to sink on a display without the fullscreen app");
        m.update(10, Event::Notice("Build finished".into()));
        assert_eq!(m.state, IslandState::Tag, "tags still show");
        m.update(20_000, Event::Timer(TimerId::TagHide));
        m.update(20_000, Event::Displaced(false));
        assert_eq!(m.state, IslandState::Seam, "back on the fullscreen display it sinks again");
        m.update(30_000, Event::Fullscreen { active: false, exe: String::new(), signals: Signals::default() });
        assert_eq!(m.state, IslandState::Orb);
    }

    fn quote(symbol: &str, change_pct: f64) -> crate::finance::Quote {
        crate::finance::Quote {
            symbol: symbol.into(),
            name: String::new(),
            price: 80_785.0,
            change_pct,
            currency: "USD".into(),
            kind: "CRYPTOCURRENCY".into(),
            points: vec![],
        }
    }

    #[test]
    fn big_price_moves_tag_once_per_step() {
        let mut m = model();
        m.update(0, Event::Quotes(vec![quote("BTC-USD", 1.0)]));
        assert!(m.current_tag().is_none());
        m.update(10, Event::Quotes(vec![quote("BTC-USD", 3.4)]));
        let t = m.current_tag().expect("a 3 % move");
        assert_eq!(t.text, "Bitcoin ▲ 3.4% · $80,785");
        let action = t.action.clone().unwrap().1;
        m.tags.clear();
        m.update(20, Event::Quotes(vec![quote("BTC-USD", 4.1)]));
        assert!(m.current_tag().is_none(), "same step: quiet");
        m.update(30, Event::Quotes(vec![quote("BTC-USD", 6.2)]));
        assert!(m.current_tag().unwrap().text.contains("▲ 6.2%"));
        m.tags.clear();
        m.update(40, Event::Quotes(vec![quote("BTC-USD", -3.1)]));
        assert!(m.current_tag().unwrap().text.contains("▼ 3.1%"), "a turn the other way");
        // "Open" opens the Markets slip.
        m.update(50, Event::Click(action));
        assert_eq!((m.state, m.front), (IslandState::Slips, Some(ModuleId::Finance)));
        // Turned off: quiet.
        let mut m = model();
        m.config.finance.price_alerts = false;
        m.update(0, Event::Quotes(vec![quote("BTC-USD", 9.0)]));
        assert!(m.current_tag().is_none());
    }

    #[test]
    fn fresh_headlines_pop_up_once() {
        let mut m = model();
        let unix = 1_792_975_000;
        m.update(0, Event::Clock { year: 2026, month: 10, day: 26, minutes: 600.0, utc_offset_min: 420, unix });
        let news = crate::finance::parse_news(include_str!("../testdata/yahoo_news.json"), "AAPL");
        m.update(10, Event::News(news.clone()));
        let t = m.current_tag().expect("a fresh headline");
        assert!(t.text.starts_with("AAPL · Apple seeks more iPhone Duo"), "{}", t.text);
        assert_eq!(t.action.as_ref().unwrap().1, Action::OpenUrl(news[0].link.clone()));
        assert_eq!(m.news.len(), 3);
        m.tags.clear();
        m.update(20, Event::News(news));
        assert!(m.current_tag().is_none(), "seen already");
    }

    #[test]
    fn the_notch_turns_through_glances_while_nothing_plays() {
        let mut m = model();
        m.config.dock.style = crate::config::Style::Notch;
        m.update(0, Event::SetModule(ModuleId::Prayer, true));
        m.update(0, Event::Clock { year: 2026, month: 10, day: 25, minutes: 600.0, utc_offset_min: 420, unix: 1_792_911_900 });
        m.update(0, Event::TodoAdded("Buy rice".into()));
        m.update(10, Event::Quotes(vec![quote("BTC-USD", 1.0)]));
        assert_eq!(m.glances(), vec![Glance::Quote(1), Glance::Todo, Glance::Prayer]);
        assert!(m.carousel_armed);
        let first = m.glance();
        m.update(CAROUSEL_MS, Event::Timer(TimerId::Carousel));
        assert_ne!(m.glance(), first);
        // Music playing: the notch shows it and stops turning.
        let fx = m.update(
            CAROUSEL_MS + 10,
            Event::Media(vec![MediaSession { id: "a".into(), title: "Song".into(), playing: true, ..Default::default() }]),
        );
        assert!(fx.contains(&Effect::KillTimer(TimerId::Carousel)));
        // The orb style never turns.
        let mut m = model();
        m.update(0, Event::TodoAdded("x".into()));
        m.update(0, Event::SetModule(ModuleId::Prayer, true));
        let fx = m.update(0, Event::Clock { year: 2026, month: 10, day: 25, minutes: 600.0, utc_offset_min: 420, unix: 1_792_911_900 });
        assert!(!has_timer(&fx, TimerId::Carousel));
    }

    #[test]
    fn the_notch_skips_prayer_after_isya() {
        let mut m = model();
        m.update(0, Event::SetModule(ModuleId::Prayer, true));
        m.update(0, Event::TodoAdded("Buy rice".into()));
        // 23:30 local: every prayer today is over.
        m.update(0, Event::Clock { year: 2026, month: 10, day: 25, minutes: 1410.0, utc_offset_min: 420, unix: 1_792_911_900 });
        assert_eq!(m.glances(), vec![Glance::Todo]);
    }

    #[test]
    fn ramadan_counts_down_to_imsak_and_buka() {
        let mut m = model();
        m.update(0, Event::SetModule(ModuleId::Prayer, true));
        // 1 March 2026 is in Ramadan 1447; 10:00 in Jakarta.
        m.update(0, Event::Clock { year: 2026, month: 3, day: 1, minutes: 600.0, utc_offset_min: 420, unix: 1_772_334_000 });
        assert!(m.ramadan_day().is_some_and(|d| (10..=13).contains(&d)), "{:?}", m.hijri());
        let (what, at) = m.fasting().unwrap();
        assert_eq!(what, "Buka");
        assert_eq!(Some(at), m.prayer_day.unwrap().get(crate::prayer::Prayer::Maghrib));
        // Imsak gets a reminder too, in Ramadan only.
        assert!(m.prayer_moments().iter().any(|(_, p, _)| *p == crate::prayer::Prayer::Imsak));
        let mut m = model();
        m.update(0, Event::SetModule(ModuleId::Prayer, true));
        m.update(0, Event::Clock { year: 2026, month: 10, day: 26, minutes: 600.0, utc_offset_min: 420, unix: 1_792_983_600 });
        assert!(m.fasting().is_none());
        assert!(!m.prayer_moments().iter().any(|(_, p, _)| *p == crate::prayer::Prayer::Imsak));
    }

    #[test]
    fn focus_rounds_are_followed_by_breaks() {
        let mut m = model();
        let fx = m.update(0, Event::Click(Action::TimerStart(25)));
        assert!(fx.contains(&Effect::FocusSession(true)));
        let fx = m.update(25 * 60_000, Event::Timer(TimerId::FocusDone));
        assert!(fx.contains(&Effect::FocusSession(false)));
        assert!(m.on_break && m.timer.is_running());
        assert_eq!(m.focus_rounds, 1);
        assert_eq!(m.timer.remaining(25 * 60_000), Some(5 * 60_000));
        assert!(m.tags.iter().any(|t| t.text == "Round 1 done · 5 min break"));
        // The break ends with an offer to start again.
        m.update(30 * 60_000, Event::Timer(TimerId::FocusDone));
        assert!(!m.on_break && m.timer == FocusTimer::Idle);
        let t = m.tags.iter().find(|t| t.text.starts_with("Break over")).unwrap();
        assert_eq!(t.action.as_ref().unwrap().1, Action::TimerStart(25));
        // Every fourth round, the long break.
        m.focus_rounds = 3;
        m.update(31 * 60_000, Event::Click(Action::TimerStart(25)));
        m.update(56 * 60_000, Event::Timer(TimerId::FocusDone));
        assert_eq!(m.timer.remaining(56 * 60_000), Some(15 * 60_000));
        m.update(57 * 60_000, Event::Click(Action::TimerSkipBreak));
        assert_eq!(m.timer, FocusTimer::Idle);
    }

    #[test]
    fn clipboard_history_keeps_recent_and_pinned() {
        let mut m = model();
        for i in 0..25 {
            m.update(i, Event::Clipboard { text: format!("text {i}"), unix_now: 1000 + i as i64 });
        }
        assert_eq!(m.clip_history.len(), CLIP_HISTORY);
        assert_eq!(m.clip_history[0].text, "text 24");
        // Copying again moves it to the top instead of repeating it.
        m.update(30, Event::Clipboard { text: "text 10".into(), unix_now: 2000 });
        assert_eq!(m.clip_history.iter().filter(|e| e.text == "text 10").count(), 1);
        let id = m.clip_history[0].id;
        let fx = m.update(31, Event::Click(Action::ClipPin(id)));
        assert!(fx.contains(&Effect::SaveClipPins));
        for i in 0..30 {
            m.update(40 + i, Event::Clipboard { text: format!("more {i}"), unix_now: 3000 + i as i64 });
        }
        assert!(m.clip_history.iter().any(|e| e.text == "text 10" && e.pinned), "pinned entries stay");
        assert_eq!(m.clip_matches("")[0].text, "text 10", "pinned first");
        // "more 20" … "more 29" are still kept ("more 2" has dropped off).
        assert_eq!(m.clip_matches("MORE 2").len(), 10, "search ignores case");
        // Clicking pastes into the app that has the keyboard.
        let fx = m.update(80, Event::Click(Action::ClipPaste(id)));
        assert!(fx.contains(&Effect::PasteText("text 10".into(), true)));
        // Searching: typing filters, Enter copies the first match.
        m.update(81, Event::Click(Action::ClipSearch));
        m.update(82, Event::Text("more 29".into()));
        let fx = m.update(83, Event::Key(Key::Enter));
        assert!(fx.contains(&Effect::PasteText("more 29".into(), false)));
        m.update(84, Event::Click(Action::ClipClear));
        assert!(m.clip_history.iter().all(|e| e.pinned));
    }

    #[test]
    fn notes_from_the_slip_and_the_command_line() {
        let mut m = model();
        m.update(0, Event::Click(Action::NoteAdd));
        assert_eq!(m.typing(InputFor::Note), Some(""));
        m.update(1, Event::Text("Call Budi".into()));
        let fx = m.update(2, Event::Key(Key::Enter));
        assert!(fx.contains(&Effect::SaveNotes));
        m.update(3, Event::Pipe(Request::Note { text: "deploy key rotated".into() }));
        assert_eq!(m.notes.iter().map(|n| n.text.as_str()).collect::<Vec<_>>(), vec!["deploy key rotated", "Call Budi"]);
        assert!(m.tags.iter().any(|t| t.text.starts_with("Note · deploy")));
        let id = m.notes[0].id;
        assert!(m.update(4, Event::Click(Action::NoteCopy(id))).contains(&Effect::CopyText("deploy key rotated".into())));
        m.update(5, Event::Click(Action::NoteRemove(id)));
        assert_eq!(m.notes.len(), 1);
    }

    #[test]
    fn battery_network_and_power_plans() {
        let mut m = model();
        m.config.power.battery_plan = "{bat}".into();
        m.config.power.plugged_plan = "{ac}".into();
        let p = |pct: u8, on_ac: bool| PowerState { percent: Some(pct), on_ac, saver: false, secs_left: Some(3600) };
        m.update(0, Event::Power(p(80, true)));
        let fx = m.update(1, Event::Power(p(79, false)));
        assert!(fx.contains(&Effect::SetPowerPlan("{bat}".into())), "unplugged: the battery plan");
        m.update(2, Event::Power(p(20, false)));
        assert!(m.tags.iter().any(|t| t.text.starts_with("Battery 20%")));
        let n = m.tags.len();
        m.update(3, Event::Power(p(19, false)));
        assert_eq!(m.tags.len(), n, "warned once per level");
        m.update(4, Event::Power(p(9, false)));
        assert!(m.current_tag().unwrap().text.starts_with("Battery 9%"));
        let fx = m.update(5, Event::Power(p(9, true)));
        assert!(fx.contains(&Effect::SetPowerPlan("{ac}".into())));
        // The connection.
        let net = |online: bool, vpn: Option<&str>| NetState { online, vpn: vpn.map(String::from), wifi: None };
        m.tags.clear();
        m.update(10, Event::Network(net(true, None)));
        assert!(m.tags.is_empty(), "the first reading is quiet");
        m.update(11, Event::Network(net(true, Some("WireGuard"))));
        assert!(m.tags.iter().any(|t| t.text == "VPN connected · WireGuard"));
        m.update(12, Event::Network(net(false, None)));
        assert!(m.tags.iter().any(|t| t.text.starts_with("Offline")));
        assert!(m.tags.iter().any(|t| t.text == "VPN disconnected · WireGuard"));
        // Picking a plan.
        m.update(
            13,
            Event::PowerPlans(vec![("{a}".into(), "Balanced".into()), ("{b}".into(), "Best performance".into())], Some("{a}".into())),
        );
        let fx = m.update(14, Event::Click(Action::SetPowerPlan(1)));
        assert!(fx.contains(&Effect::SetPowerPlan("{b}".into())));
    }

    #[test]
    fn dropped_files_pin_to_quick_launch() {
        let mut m = model();
        let fx = m.update(0, Event::Dropped(vec!["C:\\Tools\\Code.exe".into(), "D:\\Projects".into(), "C:\\Tools\\Code.exe".into()]));
        let names: Vec<&str> = m.config.launch.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Code", "Projects"]);
        assert!(fx.iter().any(|e| matches!(e, Effect::SaveValue("launch", "items", _))));
        let fx = m.update(1, Event::Click(Action::Launch(0)));
        assert!(fx.contains(&Effect::Open("C:\\Tools\\Code.exe".into())));
        m.update(2, Event::Click(Action::LaunchEdit));
        m.update(3, Event::Click(Action::Launch(0)));
        assert_eq!(m.config.launch.items.len(), 1, "editing: a tap unpins");
    }

    #[test]
    fn downloads_show_while_they_grow_and_tag_when_done() {
        let mut m = model();
        m.update(0, Event::Download { name: "big.iso".into(), bytes: 1_000_000, done: None });
        m.update(1000, Event::Download { name: "big.iso".into(), bytes: 5_000_000, done: None });
        assert_eq!(m.downloads[0].bytes, 5_000_000);
        assert!(m.available_slips().contains(&ModuleId::Captures));
        m.update(2000, Event::Download { name: "big.iso".into(), bytes: 0, done: Some("C:\\Users\\x\\Downloads\\big.iso".into()) });
        assert!(m.downloads.is_empty());
        assert_eq!(m.captures[0].kind, CaptureKind::Download);
        assert!(m.tags.iter().any(|t| t.text == "Downloaded · big.iso"));
        // Cancelled: gone without a word.
        m.update(3000, Event::Download { name: "x.zip".into(), bytes: 10, done: None });
        let n = m.tags.len();
        m.update(4000, Event::Download { name: "x.zip".into(), bytes: 0, done: Some(String::new()) });
        assert!(m.downloads.is_empty());
        assert_eq!(m.tags.len(), n);
    }

    #[test]
    fn price_levels_tag_once_per_crossing() {
        let mut m = model();
        m.config.finance.watchlist = vec![crate::config::WatchItem {
            symbol: "BTC-USD".into(),
            name: "Bitcoin".into(),
            above: Some(81_000.0),
            below: Some(79_000.0),
        }];
        m.config.finance.price_alerts = false;
        let at = |p: f64| crate::finance::Quote { price: p, ..quote("BTC-USD", 0.0) };
        m.update(0, Event::Quotes(vec![at(80_000.0)]));
        assert!(m.tags.is_empty());
        m.update(1, Event::Quotes(vec![at(81_200.0)]));
        assert_eq!(m.current_tag().unwrap().text, "Bitcoin above 81000 · $81,200");
        m.tags.clear();
        m.update(2, Event::Quotes(vec![at(81_500.0)]));
        assert!(m.tags.is_empty(), "still above: quiet");
        m.update(3, Event::Quotes(vec![at(78_900.0)]));
        assert!(m.current_tag().unwrap().text.starts_with("Bitcoin below 79000"));
    }

    #[test]
    fn meeting_now_offers_join() {
        let mut m = model();
        let unix = 1_792_911_900;
        m.update(0, Event::Clock { year: 2026, month: 10, day: 25, minutes: 600.0, utc_offset_min: 420, unix });
        let mut e = event("Standup", unix - 60, vec![]);
        e.link = "https://meet.google.com/abc-defg-hij".into();
        m.update(0, Event::Calendar(vec![e]));
        assert_eq!(m.meeting().map(|e| e.title.as_str()), Some("Standup"));
        let fx = m.update(1, Event::Click(Action::MicToggle));
        assert!(fx.contains(&Effect::MicToggle));
    }

    #[test]
    fn update_notice_offers_install() {
        let mut m = model();
        m.update(0, Event::UpdateAvailable { version: "0.2.0".into() });
        let (label, action) = m.current_tag().unwrap().action.clone().unwrap();
        assert_eq!(label, "Install");
        let fx = m.update(5, Event::Click(action));
        assert!(fx.contains(&Effect::InstallUpdate));
        let before = m.tags.len();
        m.update(10, Event::UpdateAvailable { version: "0.2.0".into() });
        assert_eq!(m.tags.len(), before, "announced once");
    }

    fn quake_near_jakarta(id: &str, mag: f32, time: i64) -> crate::weather::Alert {
        crate::weather::Alert {
            id: id.into(),
            hazard: crate::weather::Hazard::Earthquake,
            level: crate::weather::Level::Green,
            title: format!("M{mag} · near Jakarta"),
            lat: -6.6,
            lon: 106.9,
            time,
            magnitude: Some(mag),
            source: "USGS".into(),
            url: "https://earthquake.usgs.gov/x".into(),
            tsunami: false,
        }
    }

    #[test]
    fn nearby_alerts_tag_once_and_old_ones_stay_quiet() {
        let mut m = model();
        let unix = 1_792_825_200;
        m.update(0, Event::Clock { year: 2026, month: 10, day: 24, minutes: 600.0, utc_offset_min: 420, unix });
        m.update(0, Event::Alerts(vec![quake_near_jakarta("old", 5.0, unix - 2 * 86_400)]));
        assert!(m.current_tag().is_none(), "two days old: no tag");
        m.update(10, Event::Alerts(vec![quake_near_jakarta("old", 5.0, unix - 2 * 86_400), quake_near_jakarta("new", 5.2, unix - 60)]));
        let t = m.current_tag().unwrap();
        assert!(t.text.starts_with("Earthquake"), "{}", t.text);
        assert_eq!(t.action.as_ref().unwrap().1, Action::OpenUrl("https://earthquake.usgs.gov/x".into()));
        let n = m.tags.len();
        m.update(20, Event::Alerts(vec![quake_near_jakarta("new", 5.2, unix - 60)]));
        assert_eq!(m.tags.len(), n, "same alert is not repeated");
        assert!(m.available_slips().contains(&ModuleId::Weather));
    }

    #[test]
    fn offline_shows_the_weather_slip_without_errors() {
        let mut m = model();
        m.update(0, Event::Online(false));
        assert!(m.current_tag().is_none(), "no error tag for being offline");
        assert!(m.available_slips().contains(&ModuleId::Weather));
        let fx = m.update(5, Event::Click(Action::RefreshWeather));
        assert!(fx.contains(&Effect::RefreshWeather));
    }

    #[test]
    fn playlog_rolls_over() {
        let mut p = PlayLog { day: "2026-10-23".into(), minutes: BTreeMap::from([("a.exe".into(), 30)]) };
        p.roll("2026-10-23");
        assert_eq!(p.total(), 30);
        p.roll("2026-10-24");
        assert_eq!(p.total(), 0);
    }

    #[test]
    fn capture_kinds() {
        assert_eq!(CaptureKind::from_path("C:\\x\\Clip 2026.MP4"), Some(CaptureKind::Clip));
        assert_eq!(CaptureKind::from_path("shot.png"), Some(CaptureKind::Screenshot));
        assert_eq!(CaptureKind::from_path("notes.txt"), None);
    }
}
