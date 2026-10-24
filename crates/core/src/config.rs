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
}

impl ModuleId {
    pub const ALL: [ModuleId; 9] = [
        ModuleId::Media,
        ModuleId::Mixer,
        ModuleId::Commands,
        ModuleId::Ports,
        ModuleId::Clipboard,
        ModuleId::Timer,
        ModuleId::Game,
        ModuleId::Captures,
        ModuleId::Privacy,
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
}

impl Default for DockConfig {
    fn default() -> Self {
        DockConfig { edge: Edge::Left, offset: 0.45, monitor: "primary".into(), hotkey: "Win+Alt+Space".into(), hide_in_fullscreen: true }
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
        };
        *slot = on;
    }

    pub fn enabled(&self) -> impl Iterator<Item = ModuleId> + '_ {
        ModuleId::ALL.into_iter().filter(|m| self.get(*m))
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
    /// Hold back lower-priority tags while a round runs.
    pub quiet_while_running: bool,
}

impl Default for TimerConfig {
    fn default() -> Self {
        TimerConfig { presets_min: vec![25, 50], quiet_while_running: true }
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MotionConfig {
    /// Force instant transitions (Windows' "Animation effects" setting is also honoured).
    pub reduce: bool,
}

/// The file written on first run. Kept in sync with `Config::default()` by a test.
pub const DEFAULT_TOML: &str = r#"# Ngambang configuration — changes apply as soon as you save.
# Every module can be turned on or off; the tray menu "Modules" does the same.

[dock]
edge = "left"              # left | right | top | bottom
offset = 0.45              # 0.0 = start of the edge, 1.0 = end
monitor = "primary"        # or a device name like \\.\DISPLAY2
hotkey = "Win+Alt+Space"
hide_in_fullscreen = true  # sink to a 3 px line while a fullscreen app has focus

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
quiet_while_running = true

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

[motion]
reduce = false
"#;

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
        section!("motion", motion);
        cfg
    }

    fn validate(&mut self, warnings: &mut Vec<String>) {
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
        let t = line.trim();
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
            if let Some((k, rest)) = t.split_once('=') {
                if k.trim() == key {
                    // Preserve a trailing comment if there is one.
                    let comment = rest.find('#').map(|h| rest[rest[..h].trim_end().len()..].to_string()).unwrap_or_default();
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
