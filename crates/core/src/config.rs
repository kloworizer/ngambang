//! User configuration (`config.toml`). Every module can be switched on or off
//! independently; there are no fixed profiles.

use serde::{Deserialize, Serialize};

use crate::geometry::Edge;

/// Every optional module. The shell (orb, docking, Seam in fullscreen) is not a
/// module and is always present.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ModuleId {
    Media,
    Mixer,
    Commands,
    Ports,
    Clipboard,
    Timer,
    Game,
    Captures,
    Privacy,
    Prayer,
    Weather,
    Todo,
    Calendar,
    Finance,
    Notes,
    Power,
    Launch,
}

impl ModuleId {
    pub const ALL: [ModuleId; 17] = [
        ModuleId::Media,
        ModuleId::Mixer,
        ModuleId::Commands,
        ModuleId::Ports,
        ModuleId::Clipboard,
        ModuleId::Timer,
        ModuleId::Game,
        ModuleId::Captures,
        ModuleId::Privacy,
        ModuleId::Prayer,
        ModuleId::Weather,
        ModuleId::Todo,
        ModuleId::Calendar,
        ModuleId::Finance,
        ModuleId::Notes,
        ModuleId::Power,
        ModuleId::Launch,
    ];

    /// Key used in the `[modules]` table.
    pub fn key(self) -> &'static str {
        match self {
            ModuleId::Media => "media",
            ModuleId::Mixer => "mixer",
            ModuleId::Commands => "commands",
            ModuleId::Ports => "ports",
            ModuleId::Clipboard => "clipboard",
            ModuleId::Timer => "timer",
            ModuleId::Game => "game",
            ModuleId::Captures => "captures",
            ModuleId::Privacy => "privacy",
            ModuleId::Prayer => "prayer",
            ModuleId::Weather => "weather",
            ModuleId::Todo => "todo",
            ModuleId::Calendar => "calendar",
            ModuleId::Finance => "finance",
            ModuleId::Notes => "notes",
            ModuleId::Power => "power",
            ModuleId::Launch => "launch",
        }
    }

    /// Human label for menus.
    pub fn label(self) -> &'static str {
        match self {
            ModuleId::Media => "Media player",
            ModuleId::Mixer => "Volume mixer",
            ModuleId::Commands => "Command tags (CLI)",
            ModuleId::Ports => "Local ports",
            ModuleId::Clipboard => "Dev clipboard",
            ModuleId::Timer => "Focus timer",
            ModuleId::Game => "Game stats & sessions",
            ModuleId::Captures => "Screenshots & clips",
            ModuleId::Privacy => "Mic & camera beacon",
            ModuleId::Prayer => "Jadwal sholat (prayer times)",
            ModuleId::Weather => "Weather & early warnings",
            ModuleId::Todo => "To-do list",
            ModuleId::Calendar => "Calendar",
            ModuleId::Finance => "Markets watchlist",
            ModuleId::Notes => "Quick notes",
            ModuleId::Power => "Battery, network & power",
            ModuleId::Launch => "Quick launch",
        }
    }

    pub fn from_key(key: &str) -> Option<ModuleId> {
        ModuleId::ALL.into_iter().find(|m| m.key() == key)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub dock: DockConfig,
    pub modules: Modules,
    pub media: MediaConfig,
    pub commands: CommandsConfig,
    pub ports: PortsConfig,
    pub clipboard: ClipboardConfig,
    pub timer: TimerConfig,
    pub game: GameConfig,
    pub captures: CapturesConfig,
    pub privacy: PrivacyConfig,
    pub location: LocationConfig,
    pub prayer: PrayerConfig,
    pub weather: WeatherConfig,
    pub calendar: CalendarConfig,
    pub finance: FinanceConfig,
    pub power: PowerConfig,
    pub launch: LaunchConfig,
    pub update: UpdateConfig,
    pub motion: MotionConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DockConfig {
    pub edge: Edge,
    /// Position along the edge, 0.0 = start (top/left) … 1.0 = end.
    pub offset: f32,
    /// "primary" or a monitor device name such as `\\.\DISPLAY2`.
    pub monitor: String,
    pub hotkey: String,
    /// Sink to the 3 px Seam while a fullscreen app has focus.
    pub hide_in_fullscreen: bool,
    /// With more than one display, move to another one while a fullscreen
    /// app (a video, a game) covers this one, and come back afterwards.
    pub move_in_fullscreen: bool,
    pub style: Style,
    /// What the notch turns through when nothing plays, in order, by key
    /// (see `NOTCH_GLANCES`).
    pub notch_glances: Vec<String>,
    /// Seconds each notch glance stays.
    pub notch_glance_s: u32,
    /// Executables (file names) that never move or sink the island when
    /// they go fullscreen, such as a streaming tool.
    pub fullscreen_ignore: Vec<String>,
    /// Executables that always sink the island to the Seam on their own
    /// display instead of moving it to another one.
    pub fullscreen_sink: Vec<String>,
    /// Hide the island while you share your screen in a meeting app.
    pub hide_when_sharing: bool,
    /// While dragging, snap to the middle and the ends of an edge.
    pub snap: bool,
}

/// Keys for `dock.notch_glances`, in the default order.
pub const NOTCH_GLANCES: [&str; 5] = ["weather", "markets", "todo", "calendar", "prayer"];

impl Default for DockConfig {
    fn default() -> Self {
        DockConfig {
            edge: Edge::Left,
            offset: 0.45,
            monitor: "primary".into(),
            hotkey: "Win+Alt+Space".into(),
            hide_in_fullscreen: true,
            move_in_fullscreen: true,
            style: Style::Orb,
            notch_glances: NOTCH_GLANCES.iter().map(|k| k.to_string()).collect(),
            notch_glance_s: 6,
            fullscreen_ignore: Vec::new(),
            fullscreen_sink: Vec::new(),
            hide_when_sharing: true,
            snap: true,
        }
    }
}

impl DockConfig {
    /// The edge the island really uses: a notch always hangs from the top.
    pub fn edge(&self) -> Edge {
        match self.style {
            Style::Notch => Edge::Top,
            Style::Orb => self.edge,
        }
    }
}

/// How the island looks at rest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    /// A small orb on any edge that grows into tags and slips.
    #[default]
    Orb,
    /// A notch hanging from the top of the screen, dragged left or right,
    /// that widens for tags and drops slips down from it.
    Notch,
}

impl Style {
    pub const ALL: [Style; 2] = [Style::Orb, Style::Notch];

    pub fn name(self) -> &'static str {
        match self {
            Style::Orb => "orb",
            Style::Notch => "notch",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Modules {
    pub media: bool,
    pub mixer: bool,
    pub commands: bool,
    pub ports: bool,
    pub clipboard: bool,
    pub timer: bool,
    pub game: bool,
    pub captures: bool,
    pub privacy: bool,
    pub prayer: bool,
    pub weather: bool,
    pub todo: bool,
    pub calendar: bool,
    pub finance: bool,
    pub notes: bool,
    pub power: bool,
    pub launch: bool,
    /// Tab order, by module key; modules not listed follow in the default order.
    pub order: Vec<String>,
}

impl Default for Modules {
    fn default() -> Self {
        Modules {
            media: true,
            mixer: true,
            commands: true,
            ports: true,
            clipboard: true,
            timer: true,
            game: true,
            captures: true,
            privacy: true,
            prayer: false,
            weather: true,
            todo: true,
            calendar: true,
            finance: true,
            notes: true,
            power: true,
            launch: true,
            order: Vec::new(),
        }
    }
}

impl Modules {
    pub fn get(&self, m: ModuleId) -> bool {
        match m {
            ModuleId::Media => self.media,
            ModuleId::Mixer => self.mixer,
            ModuleId::Commands => self.commands,
            ModuleId::Ports => self.ports,
            ModuleId::Clipboard => self.clipboard,
            ModuleId::Timer => self.timer,
            ModuleId::Game => self.game,
            ModuleId::Captures => self.captures,
            ModuleId::Privacy => self.privacy,
            ModuleId::Prayer => self.prayer,
            ModuleId::Weather => self.weather,
            ModuleId::Todo => self.todo,
            ModuleId::Calendar => self.calendar,
            ModuleId::Finance => self.finance,
            ModuleId::Notes => self.notes,
            ModuleId::Power => self.power,
            ModuleId::Launch => self.launch,
        }
    }

    pub fn set(&mut self, m: ModuleId, on: bool) {
        let slot = match m {
            ModuleId::Media => &mut self.media,
            ModuleId::Mixer => &mut self.mixer,
            ModuleId::Commands => &mut self.commands,
            ModuleId::Ports => &mut self.ports,
            ModuleId::Clipboard => &mut self.clipboard,
            ModuleId::Timer => &mut self.timer,
            ModuleId::Game => &mut self.game,
            ModuleId::Captures => &mut self.captures,
            ModuleId::Privacy => &mut self.privacy,
            ModuleId::Prayer => &mut self.prayer,
            ModuleId::Weather => &mut self.weather,
            ModuleId::Todo => &mut self.todo,
            ModuleId::Calendar => &mut self.calendar,
            ModuleId::Finance => &mut self.finance,
            ModuleId::Notes => &mut self.notes,
            ModuleId::Power => &mut self.power,
            ModuleId::Launch => &mut self.launch,
        };
        *slot = on;
    }

    pub fn enabled(&self) -> impl Iterator<Item = ModuleId> + '_ {
        ModuleId::ALL.into_iter().filter(|m| self.get(*m))
    }

    /// Every module in the user's order: the listed ones first, then the rest.
    pub fn ordered(&self) -> Vec<ModuleId> {
        let mut out: Vec<ModuleId> = Vec::with_capacity(ModuleId::ALL.len());
        for key in &self.order {
            if let Some(m) = ModuleId::from_key(key) {
                if !out.contains(&m) {
                    out.push(m);
                }
            }
        }
        let rest: Vec<ModuleId> = ModuleId::ALL.into_iter().filter(|m| !out.contains(m)).collect();
        out.extend(rest);
        out
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaConfig {
    /// Show a short tag when a new track starts.
    pub track_tags: bool,
    pub show_system_sounds: bool,
    /// Volume change per wheel notch, in percent.
    pub volume_step: u32,
}

impl Default for MediaConfig {
    fn default() -> Self {
        MediaConfig { track_tags: true, show_system_sounds: false, volume_step: 2 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandsConfig {
    /// Shell hooks report commands that ran at least this long.
    pub long_command_s: u64,
    pub ignore: Vec<String>,
}

impl Default for CommandsConfig {
    fn default() -> Self {
        CommandsConfig {
            long_command_s: 30,
            ignore: ["vim", "nvim", "less", "more", "ssh", "top", "htop", "man", "watch"].map(String::from).to_vec(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortsConfig {
    pub min_port: u16,
    pub hide_processes: Vec<String>,
}

impl Default for PortsConfig {
    fn default() -> Self {
        PortsConfig {
            min_port: 1024,
            hide_processes: ["svchost.exe", "lsass.exe", "services.exe", "wininit.exe", "spoolsv.exe"].map(String::from).to_vec(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardConfig {
    /// Show a short tag when a developer format is recognised.
    pub tags: bool,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        ClipboardConfig { tags: true }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimerConfig {
    pub presets_min: Vec<u32>,
    /// The length of the "Custom" round, changed with − and + in the slip.
    pub custom_min: u32,
    /// Hold back lower-priority tags while a round runs.
    pub quiet_while_running: bool,
    /// A break follows each round (0 = no breaks) ...
    pub break_min: u32,
    /// ... a longer one after every `long_every` rounds.
    pub long_break_min: u32,
    pub long_every: u32,
    /// Turn on Windows' Do not disturb (focus session) during rounds.
    pub focus_assist: bool,
}

impl Default for TimerConfig {
    fn default() -> Self {
        TimerConfig {
            presets_min: vec![25, 50],
            custom_min: 15,
            quiet_while_running: true,
            break_min: 5,
            long_break_min: 15,
            long_every: 4,
            focus_assist: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameConfig {
    /// Executables that always count as games.
    pub games: Vec<String>,
    /// Executables that never count as games (fullscreen video, presentations…).
    pub not_games: Vec<String>,
    pub break_after_min: u32,
    /// "gateway", an IPv4 address or a host name.
    pub ping_host: String,
    /// Power plan GUID to switch to during games; empty = don't switch.
    pub power_plan: String,
}

impl Default for GameConfig {
    fn default() -> Self {
        GameConfig { games: Vec::new(), not_games: Vec::new(), break_after_min: 90, ping_host: "gateway".into(), power_plan: String::new() }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CapturesConfig {
    /// Folders watched in addition to Videos\Captures and Pictures\Screenshots.
    pub extra_dirs: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrivacyConfig {
    /// Exclude the island from screenshots and screen sharing.
    pub hide_from_capture: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocationConfig {
    /// Display name of the place.
    pub city: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl Default for LocationConfig {
    fn default() -> Self {
        LocationConfig { city: "Jakarta".into(), latitude: -6.2088, longitude: 106.8456 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrayerConfig {
    pub method: crate::prayer::Method,
    pub asr: crate::prayer::Asr,
    /// Minutes before each prayer for a reminder tag; 0 = only at the time.
    pub reminder_min: u32,
    /// Days added to the calculated Hijri date (−2…2), to match the date
    /// announced after sighting the moon.
    pub hijri_adjust: i32,
}

impl Default for PrayerConfig {
    fn default() -> Self {
        PrayerConfig { method: crate::prayer::Method::Kemenag, asr: crate::prayer::Asr::Standard, reminder_min: 10, hijri_adjust: 0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherConfig {
    /// Raise tags for early warnings near the location.
    pub alerts: bool,
    /// Earthquakes within this distance raise a tag.
    pub quake_radius_km: u32,
    /// Smallest earthquake magnitude that raises a tag.
    pub quake_min_magnitude: f32,
    /// A tag shortly before the forecast says rain starts.
    pub rain_tags: bool,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        WeatherConfig { alerts: true, quake_radius_km: 300, quake_min_magnitude: 4.5, rain_tags: true }
    }
}

/// One linked calendar: a secret iCal address (Google, Outlook, iCloud…).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalendarFeed {
    pub name: String,
    pub url: String,
    /// "#RRGGBB".
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalendarConfig {
    pub feeds: Vec<CalendarFeed>,
    /// Minutes before an event for a reminder; 0 = none.
    pub reminder_min: u32,
    /// Days ahead shown in the slip.
    pub days: u32,
}

/// Public holidays in Indonesia (Google's holiday calendar), linked by default.
pub const HOLIDAYS_URL: &str =
    "https://calendar.google.com/calendar/ical/en-gb.indonesian%23holiday%40group.v.calendar.google.com/public/basic.ics";

impl CalendarFeed {
    pub fn holidays() -> CalendarFeed {
        CalendarFeed { name: "Libur Indonesia".into(), url: HOLIDAYS_URL.into(), color: "#FF8A70".into() }
    }
}

impl Default for CalendarConfig {
    fn default() -> Self {
        CalendarConfig { feeds: vec![CalendarFeed::holidays()], reminder_min: 10, days: 7 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchItem {
    /// Yahoo Finance symbol: BBCA.JK, ^JKSE, BTC-USD, USDIDR=X, AAPL…
    pub symbol: String,
    pub name: String,
    /// Alert when the price rises above / falls below these levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub above: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub below: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FinanceConfig {
    pub watchlist: Vec<WatchItem>,
    /// A tag when a symbol moves `move_pct` since the previous close, and
    /// again at every further step.
    pub price_alerts: bool,
    pub move_pct: f32,
    /// A tag for fresh headlines about the watchlist.
    pub news: bool,
}

impl Default for FinanceConfig {
    fn default() -> Self {
        let item = |symbol: &str, name: &str| WatchItem { symbol: symbol.into(), name: name.into(), ..Default::default() };
        FinanceConfig {
            watchlist: vec![item("^JKSE", "IDX Composite"), item("BTC-USD", "Bitcoin"), item("USDIDR=X", "USD/IDR")],
            price_alerts: true,
            move_pct: 3.0,
            news: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PowerConfig {
    /// Power plan GUIDs to switch to on battery and when plugged in; empty = leave as is.
    pub battery_plan: String,
    pub plugged_plan: String,
    /// Battery percentage that raises a warning (and again at 10 %).
    pub low_battery: u32,
    /// Tags when the connection or a VPN drops or comes back.
    pub network_tags: bool,
}

impl Default for PowerConfig {
    fn default() -> Self {
        PowerConfig { battery_plan: String::new(), plugged_plan: String::new(), low_battery: 20, network_tags: true }
    }
}

/// An app, folder, file or link pinned to the Quick launch slip.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchItem {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchConfig {
    pub items: Vec<LaunchItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// Look for a newer release on GitHub once a day.
    pub auto_check: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        UpdateConfig { auto_check: true }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MotionConfig {
    /// Force instant transitions (Windows' "Animation effects" setting is also honoured).
    pub reduce: bool,
}

/// The file written on first run. Kept in sync with `Config::default()` by a test.
pub const DEFAULT_TOML: &str = r##"# Ngambang configuration — changes apply as soon as you save.
# Every module can be turned on or off; the tray menu "Modules" does the same.

[dock]
edge = "left"              # left | right | top | bottom
offset = 0.45              # 0.0 = start of the edge, 1.0 = end
monitor = "primary"        # or a device name like \\.\DISPLAY2
hotkey = "Win+Alt+Space"
hide_in_fullscreen = true  # sink to a 3 px line while a fullscreen app has focus
move_in_fullscreen = true  # with two or more displays, move to another one meanwhile
style = "orb"              # orb (any edge) | notch (top of the screen, drag left or right)
notch_glances = ["weather", "markets", "todo", "calendar", "prayer"]  # what the notch turns through, in order
notch_glance_s = 6         # seconds each one stays (3–60)
fullscreen_ignore = []     # exe names that never move or sink the island, e.g. ["obs64.exe"]
fullscreen_sink = []       # exe names that sink the island to a line instead of moving it
hide_when_sharing = true   # hide while you share your screen in Zoom, Teams, Meet …
snap = true                # snap to the middle and the ends of an edge while dragging

[modules]
media = true       # now playing from every app with media controls
mixer = true       # per-app volume for every app making sound
commands = true    # `ngambang notify/run` and shell-hook tags
ports = true       # local listening ports, open / stop
clipboard = true   # JWT, JSON, base64, UUID, timestamps, colours, tracking links
timer = true       # focus timer
game = true        # game sessions, GPU, ping, break reminder
captures = true    # new screenshots and clips
privacy = true     # mic / camera in use
prayer = false     # jadwal sholat: prayer times and reminders
weather = true     # weather and early warnings (GDACS, USGS)
todo = true        # a small to-do list
calendar = true    # upcoming events from linked calendars
finance = true     # prices for your watchlist
notes = true       # quick notes (also `ngambang note "..."`)
power = true       # battery, network and VPN, power plans
launch = true      # pinned apps, folders and files (drop them on the island)
order = []         # tab order by key, e.g. ["media", "todo"]; the rest follow

[media]
track_tags = true
show_system_sounds = false
volume_step = 2

[commands]
long_command_s = 30
ignore = ["vim", "nvim", "less", "more", "ssh", "top", "htop", "man", "watch"]

[ports]
min_port = 1024
hide_processes = ["svchost.exe", "lsass.exe", "services.exe", "wininit.exe", "spoolsv.exe"]

[clipboard]
tags = true

[timer]
presets_min = [25, 50]
custom_min = 15            # the "Custom" round; − and + in the slip change it
quiet_while_running = true
break_min = 5              # a break after each round; 0 = none
long_break_min = 15        # a longer break ...
long_every = 4             # ... after this many rounds
focus_assist = true        # Windows' Do not disturb during rounds

[game]
games = []                 # exe names that always count as games
not_games = []             # exe names that never count
break_after_min = 90
ping_host = "gateway"      # "gateway", an IPv4 address or a host name
power_plan = ""            # power plan GUID to use while gaming; empty = don't switch

[captures]
extra_dirs = []

[privacy]
hide_from_capture = false

[location]                 # used by prayer times and weather
city = "Jakarta"
latitude = -6.2088
longitude = 106.8456

[prayer]
method = "kemenag"         # kemenag | jakim | mwl | isna | egypt | makkah | karachi
asr = "standard"           # standard | hanafi
reminder_min = 10          # reminder before each prayer; 0 = only at the time
hijri_adjust = 0           # days added to the Hijri date (-2…2) to match the announced one

[weather]
alerts = true              # tags for disasters near the location
quake_radius_km = 300
quake_min_magnitude = 4.5
rain_tags = true           # a tag shortly before rain starts

[calendar]                 # link calendars in Settings ▸ Calendar
feeds = [{ name = "Libur Indonesia", url = "https://calendar.google.com/calendar/ical/en-gb.indonesian%23holiday%40group.v.calendar.google.com/public/basic.ics", color = "#FF8A70" }]
reminder_min = 10          # reminder before each event; 0 = none
days = 7                   # days ahead shown

[finance]                  # add symbols in Settings ▸ Markets (search by name)
watchlist = [{ symbol = "^JKSE", name = "IDX Composite" }, { symbol = "BTC-USD", name = "Bitcoin" }, { symbol = "USDIDR=X", name = "USD/IDR" }]
price_alerts = true        # a tag when a symbol moves move_pct, and at every further step
move_pct = 3.0             # percent since the previous close
news = true                # a tag for fresh headlines about your watchlist

[power]
battery_plan = ""          # power plan GUID to use on battery; empty = leave as is
plugged_plan = ""          # power plan GUID to use when plugged in
low_battery = 20           # warn at this battery level (and at 10%)
network_tags = true        # tags when the connection or a VPN drops or comes back

[launch]                   # drop apps, folders or files on the island to pin them
items = []

[update]
auto_check = true          # look for a new release on GitHub once a day

[motion]
reduce = false
"##;

impl Config {
    /// Parse a config file. Never fails: problems are returned as warnings and
    /// the affected values fall back to defaults.
    pub fn parse(text: &str) -> (Config, Vec<String>) {
        let mut warnings = Vec::new();
        let mut cfg = match toml::from_str::<Config>(text) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(format!("config.toml: {}", first_line(&e.to_string())));
                // Fall back table by table so one bad value doesn't reset everything.
                Self::parse_lenient(text, &mut warnings)
            }
        };
        cfg.validate(&mut warnings);
        (cfg, warnings)
    }

    fn parse_lenient(text: &str, warnings: &mut Vec<String>) -> Config {
        let mut cfg = Config::default();
        let Ok(table) = text.parse::<toml::Table>() else {
            return cfg;
        };
        macro_rules! section {
            ($key:literal, $field:ident) => {
                if let Some(v) = table.get($key) {
                    match v.clone().try_into() {
                        Ok(parsed) => cfg.$field = parsed,
                        Err(e) => warnings.push(format!("[{}] ignored: {}", $key, first_line(&e.to_string()))),
                    }
                }
            };
        }
        section!("dock", dock);
        section!("modules", modules);
        section!("media", media);
        section!("commands", commands);
        section!("ports", ports);
        section!("clipboard", clipboard);
        section!("timer", timer);
        section!("game", game);
        section!("captures", captures);
        section!("privacy", privacy);
        section!("location", location);
        section!("prayer", prayer);
        section!("weather", weather);
        section!("calendar", calendar);
        section!("finance", finance);
        section!("power", power);
        section!("launch", launch);
        section!("update", update);
        section!("motion", motion);
        cfg
    }

    fn validate(&mut self, warnings: &mut Vec<String>) {
        let before = self.dock.notch_glances.len();
        self.dock.notch_glances.retain(|k| NOTCH_GLANCES.contains(&k.as_str()));
        if self.dock.notch_glances.len() != before {
            warnings.push(format!("dock.notch_glances: use any of {}", NOTCH_GLANCES.join(", ")));
        }
        if !(3..=60).contains(&self.dock.notch_glance_s) {
            warnings.push("dock.notch_glance_s must be 3–60".into());
            self.dock.notch_glance_s = self.dock.notch_glance_s.clamp(3, 60);
        }
        if !(0.0..=1.0).contains(&self.dock.offset) || self.dock.offset.is_nan() {
            warnings.push("dock.offset must be between 0.0 and 1.0".into());
            self.dock.offset = self.dock.offset.clamp(0.0, 1.0);
            if self.dock.offset.is_nan() {
                self.dock.offset = DockConfig::default().offset;
            }
        }
        if crate::hotkey::Hotkey::parse(&self.dock.hotkey).is_none() {
            warnings.push(format!("dock.hotkey \"{}\" not understood", self.dock.hotkey));
            self.dock.hotkey = DockConfig::default().hotkey;
        }
        if !(1..=20).contains(&self.media.volume_step) {
            warnings.push("media.volume_step must be 1–20".into());
            self.media.volume_step = self.media.volume_step.clamp(1, 20);
        }
        if self.game.break_after_min != 0 && self.game.break_after_min < 10 {
            warnings.push("game.break_after_min must be 0 (off) or at least 10".into());
            self.game.break_after_min = 10;
        }
        self.timer.presets_min.retain(|m| (1..=240).contains(m));
        if self.timer.presets_min.is_empty() {
            self.timer.presets_min = TimerConfig::default().presets_min;
        }
        self.timer.presets_min.truncate(3);
        if self.timer.break_min > 60 || self.timer.long_break_min > 120 || self.timer.long_every == 0 {
            warnings.push("timer: break_min 0–60, long_break_min 0–120, long_every 1 or more".into());
            self.timer.break_min = self.timer.break_min.min(60);
            self.timer.long_break_min = self.timer.long_break_min.min(120);
            self.timer.long_every = self.timer.long_every.max(1);
        }
        if !(-2..=2).contains(&self.prayer.hijri_adjust) {
            warnings.push("prayer.hijri_adjust must be -2…2".into());
            self.prayer.hijri_adjust = self.prayer.hijri_adjust.clamp(-2, 2);
        }
        if !(0.5..=50.0).contains(&self.finance.move_pct) {
            warnings.push("finance.move_pct must be 0.5–50".into());
            self.finance.move_pct = if self.finance.move_pct.is_nan() { 3.0 } else { self.finance.move_pct.clamp(0.5, 50.0) };
        }
        if !(1..=240).contains(&self.timer.custom_min) {
            warnings.push("timer.custom_min must be 1–240".into());
            self.timer.custom_min = self.timer.custom_min.clamp(1, 240);
        }
        if !(-90.0..=90.0).contains(&self.location.latitude) || !(-180.0..=180.0).contains(&self.location.longitude) {
            warnings.push("location.latitude/longitude are out of range".into());
            self.location = LocationConfig::default();
        }
        if !(10..=5000).contains(&self.weather.quake_radius_km) {
            warnings.push("weather.quake_radius_km must be 10–5000".into());
            self.weather.quake_radius_km = self.weather.quake_radius_km.clamp(10, 5000);
        }
        if self.calendar.reminder_min > 120 {
            warnings.push("calendar.reminder_min must be 0–120".into());
            self.calendar.reminder_min = 120;
        }
        self.calendar.days = self.calendar.days.clamp(1, 30);
        self.calendar.feeds.retain(|f| !f.url.trim().is_empty());
        self.finance.watchlist.retain(|w| !w.symbol.trim().is_empty());
        self.finance.watchlist.truncate(20);
        if self.prayer.reminder_min > 60 {
            warnings.push("prayer.reminder_min must be 0–60".into());
            self.prayer.reminder_min = 60;
        }
        if !self.game.power_plan.is_empty() && crate::format::parse_guid(&self.game.power_plan).is_none() {
            warnings.push("game.power_plan is not a GUID".into());
            self.game.power_plan.clear();
        }
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

fn first_line(s: &str) -> String {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
}

/// Set `key = value` inside `[table]` of a TOML document, keeping every other
/// line (and comments) intact. `value` must already be a TOML literal.
pub fn set_toml_value(text: &str, table: &str, key: &str, value: &str) -> String {
    let header = format!("[{table}]");
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let mut in_table = false;
    let mut table_start = None;
    let mut table_end = lines.len();
    for (i, line) in lines.iter().enumerate() {
        let t = line[..comment_start(line).unwrap_or(line.len())].trim();
        if t.starts_with('[') {
            if in_table {
                table_end = i;
                break;
            }
            if t == header {
                in_table = true;
                table_start = Some(i);
            }
            continue;
        }
        if in_table {
            if let Some((k, rest)) = line.trim().split_once('=') {
                if k.trim() == key {
                    // Preserve a trailing comment if there is one.
                    let comment = comment_start(rest).map(|h| rest[rest[..h].trim_end().len()..].to_string()).unwrap_or_default();
                    let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                    lines[i] = format!("{indent}{key} = {value}{comment}");
                    return join_lines(&lines, text);
                }
            }
        }
    }
    match table_start {
        Some(_) => {
            // Insert before trailing blank lines of the table.
            let mut at = table_end;
            while at > 0 && lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            lines.insert(at, format!("{key} = {value}"));
        }
        None => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(header);
            lines.push(format!("{key} = {value}"));
        }
    }
    join_lines(&lines, text)
}

/// Where a `#` comment starts on a TOML line, skipping `#` inside strings
/// (colours like "#A99BFF", URLs with fragments).
fn comment_start(line: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        match quote {
            Some('"') if escaped => escaped = false,
            Some('"') if c == '\\' => escaped = true,
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' => return Some(i),
            None => {}
        }
    }
    None
}

fn join_lines(lines: &[String], original: &str) -> String {
    let mut out = lines.join("\n");
    if original.ends_with('\n') || original.is_empty() {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_matches_default_struct() {
        let (cfg, warnings) = Config::parse(DEFAULT_TOML);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn empty_file_is_defaults() {
        let (cfg, warnings) = Config::parse("");
        assert!(warnings.is_empty());
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn bad_value_only_resets_its_table() {
        let text = "[dock]\nedge = \"diagonal\"\n[modules]\nports = false\n";
        let (cfg, warnings) = Config::parse(text);
        assert!(!warnings.is_empty());
        assert_eq!(cfg.dock.edge, Edge::Left);
        assert!(!cfg.modules.ports);
    }

    #[test]
    fn validation_clamps() {
        let (cfg, w) =
            Config::parse("[dock]\noffset = 3.0\n[media]\nvolume_step = 0\n[game]\nbreak_after_min = 2\npower_plan = \"nope\"\n");
        assert_eq!(cfg.dock.offset, 1.0);
        assert_eq!(cfg.media.volume_step, 1);
        assert_eq!(cfg.game.break_after_min, 10);
        assert!(cfg.game.power_plan.is_empty());
        assert_eq!(w.len(), 4);
    }

    #[test]
    fn modules_toggle_roundtrip() {
        let mut m = Modules::default();
        for id in ModuleId::ALL {
            m.set(id, false);
            assert!(!m.get(id));
            assert_eq!(ModuleId::from_key(id.key()), Some(id));
        }
        assert_eq!(m.enabled().count(), 0);
    }

    #[test]
    fn set_value_keeps_comments() {
        let out = set_toml_value(DEFAULT_TOML, "modules", "ports", "false");
        assert!(out.lines().any(|l| l.starts_with("ports = false ") && l.ends_with("# local listening ports, open / stop")));
        assert!(out.contains("# Ngambang configuration"));
        let (cfg, w) = Config::parse(&out);
        assert!(w.is_empty());
        assert!(!cfg.modules.ports);
        assert!(cfg.modules.media);
    }

    #[test]
    fn hash_inside_strings_is_not_a_comment() {
        let base = "[calendar]\nfeeds = []   # a note\n";
        let first = set_toml_value(base, "calendar", "feeds", r##"[{ name = "Work", url = "https://x/a.ics#y", color = "#A99BFF" }]"##);
        let again = set_toml_value(&first, "calendar", "feeds", r##"[{ name = "Home", url = "https://x/b.ics", color = "#7FD1AE" }]"##);
        let (cfg, w) = Config::parse(&again);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(cfg.calendar.feeds.len(), 1);
        assert_eq!(cfg.calendar.feeds[0].color, "#7FD1AE");
        assert_eq!(
            again.lines().nth(1).unwrap(),
            r##"feeds = [{ name = "Home", url = "https://x/b.ics", color = "#7FD1AE" }]   # a note"##
        );
        assert_eq!(comment_start(r#"a = "x#y" # note"#), Some(10));
        assert_eq!(comment_start(r#"a = 'x#y'"#), None);
        assert_eq!(comment_start(r##"a = "q\"#" # c"##), Some(11));
    }

    #[test]
    fn module_order_puts_listed_first() {
        let (cfg, _) = Config::parse("[modules]\norder = [\"todo\", \"nope\", \"media\", \"todo\"]\n");
        let o = cfg.modules.ordered();
        assert_eq!(&o[..3], &[ModuleId::Todo, ModuleId::Media, ModuleId::Mixer]);
        assert_eq!(o.len(), ModuleId::ALL.len());
    }

    #[test]
    fn set_value_finds_headers_with_comments() {
        let text = "[location]   # shared\ncity = \"Jakarta\"\n";
        let out = set_toml_value(text, "location", "city", "\"Bandung\"");
        assert_eq!(out.matches("[location]").count(), 1);
        assert!(out.contains("city = \"Bandung\""));
    }

    #[test]
    fn set_value_inserts_missing_key_and_table() {
        let out = set_toml_value("[dock]\nedge = \"left\"\n\n[media]\n", "dock", "offset", "0.2");
        let (cfg, _) = Config::parse(&out);
        assert_eq!(cfg.dock.offset, 0.2);
        let out = set_toml_value("", "modules", "game", "false");
        let (cfg, _) = Config::parse(&out);
        assert!(!cfg.modules.game);
    }

    #[test]
    fn serialises_back() {
        let (cfg, _) = Config::parse(&Config::default().to_toml());
        assert_eq!(cfg, Config::default());
    }
}
