//! Classify the fullscreen foreground app (SPEC §7.1). Only real games start
//! a session: a fullscreen window alone is not enough.

use crate::config::GameConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullscreenKind {
    /// A game: starts a session, stats and break reminder.
    Game,
    /// Fullscreen video, slides, a browser: Seam only.
    Media,
    /// Screen overlays (snipping, magnifier, Start, Game Bar): not fullscreen at all.
    Ignore,
}

/// What the platform measured about the fullscreen process.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Signals {
    /// Windows reports exclusive Direct3D fullscreen (`QUNS_RUNNING_D3D_FULL_SCREEN`).
    pub exclusive_d3d: bool,
    /// The process's own 3D engine load, in percent, if it could be measured.
    pub gpu_3d_pct: Option<f32>,
}

/// A fullscreen app using at least this much 3D GPU counts as a game.
pub const GPU_GAME_PCT: f32 = 20.0;

/// Transparent fullscreen overlays that must never move the island.
const OVERLAYS: &[&str] = &[
    "snippingtool.exe",
    "screenclippinghost.exe",
    "screensketch.exe",
    "sharex.exe",
    "greenshot.exe",
    "lightshot.exe",
    "flameshot.exe",
    "magnify.exe",
    "osk.exe",
    "searchhost.exe",
    "searchapp.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "textinputhost.exe",
    "gamebar.exe",
    "gamebarftserver.exe",
    "xboxgamebarwidgets.exe",
    "lockapp.exe",
    "logonui.exe",
    "consent.exe",
    "powertoys.screenruler.exe",
    "powertoys.colorpickerui.exe",
    "powertoys.poweraccent.exe",
    "powertoys.peek.ui.exe",
];

/// Apps that go fullscreen but are not games.
const KNOWN_NOT_GAMES: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "arc.exe",
    "vlc.exe",
    "mpv.exe",
    "mpc-hc64.exe",
    "mpc-be64.exe",
    "potplayermini64.exe",
    "wmplayer.exe",
    "video.ui.exe",
    "microsoft.media.player.exe",
    "applicationframehost.exe",
    "powerpnt.exe",
    "netflix.exe",
    "spotify.exe",
    "zoom.exe",
    "teams.exe",
    "ms-teams.exe",
    "obs64.exe",
    "explorer.exe",
    "windowsterminal.exe",
    "code.exe",
    "devenv.exe",
    "mstsc.exe",
    "photos.exe",
    "acrobat.exe",
];

/// Folders games are installed into by the common launchers.
const GAME_DIRS: &[&str] = &[
    "\\steamapps\\common\\",
    "\\epic games\\",
    "\\xboxgames\\",
    "\\riot games\\",
    "\\gog galaxy\\games\\",
    "\\gog games\\",
    "\\ea games\\",
    "\\electronic arts\\",
    "\\ubisoft game launcher\\games\\",
    "\\battle.net\\",
    "\\blizzard entertainment\\",
    "\\rockstar games\\",
    "\\itch\\apps\\",
    "\\hoyoplay\\games\\",
];

/// `path` is the full executable path (a bare name also works).
pub fn classify(path: &str, signals: Signals, cfg: &GameConfig) -> FullscreenKind {
    let exe = crate::format::exe_name(path);
    let lower = path.to_ascii_lowercase().replace('/', "\\");
    let listed = |list: &[String]| list.iter().any(|g| crate::format::exe_name(g) == exe);
    if listed(&cfg.games) {
        return FullscreenKind::Game;
    }
    if OVERLAYS.contains(&exe.as_str()) {
        return FullscreenKind::Ignore;
    }
    if listed(&cfg.not_games) || KNOWN_NOT_GAMES.contains(&exe.as_str()) {
        return FullscreenKind::Media;
    }
    let in_game_dir = GAME_DIRS.iter().any(|d| lower.contains(d));
    let busy_gpu = signals.gpu_3d_pct.is_some_and(|p| p >= GPU_GAME_PCT);
    if in_game_dir || signals.exclusive_d3d || busy_gpu {
        FullscreenKind::Game
    } else {
        FullscreenKind::Media
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Signals = Signals { exclusive_d3d: false, gpu_3d_pct: None };

    #[test]
    fn overlays_are_ignored() {
        let cfg = GameConfig::default();
        assert_eq!(classify("C:\\Windows\\System32\\SnippingTool.exe", NONE, &cfg), FullscreenKind::Ignore);
        assert_eq!(
            classify("ScreenClippingHost.exe", Signals { exclusive_d3d: false, gpu_3d_pct: Some(60.0) }, &cfg),
            FullscreenKind::Ignore
        );
    }

    #[test]
    fn a_fullscreen_window_alone_is_not_a_game() {
        let cfg = GameConfig::default();
        assert_eq!(classify("C:\\Tools\\viewer.exe", NONE, &cfg), FullscreenKind::Media);
        assert_eq!(classify("C:\\Tools\\viewer.exe", Signals { exclusive_d3d: false, gpu_3d_pct: Some(3.0) }, &cfg), FullscreenKind::Media);
    }

    #[test]
    fn real_games_are_recognised() {
        let cfg = GameConfig::default();
        assert_eq!(classify("D:\\SteamLibrary\\steamapps\\common\\Hades\\Hades.exe", NONE, &cfg), FullscreenKind::Game);
        assert_eq!(classify("C:\\XboxGames\\Forza\\Content\\forza.exe", NONE, &cfg), FullscreenKind::Game);
        assert_eq!(classify("C:\\Indie\\game.exe", Signals { exclusive_d3d: true, gpu_3d_pct: None }, &cfg), FullscreenKind::Game);
        assert_eq!(classify("C:\\Indie\\game.exe", Signals { exclusive_d3d: false, gpu_3d_pct: Some(45.0) }, &cfg), FullscreenKind::Game);
    }

    #[test]
    fn user_lists_win() {
        let mut cfg = GameConfig::default();
        assert_eq!(classify("chrome.exe", NONE, &cfg), FullscreenKind::Media);
        cfg.games.push("chrome.exe".into());
        assert_eq!(classify("C:\\x\\Chrome.exe", NONE, &cfg), FullscreenKind::Game);
        cfg.not_games.push("Hades.exe".into());
        assert_eq!(classify("D:\\steamapps\\common\\Hades\\Hades.exe", NONE, &cfg), FullscreenKind::Media);
    }
}
