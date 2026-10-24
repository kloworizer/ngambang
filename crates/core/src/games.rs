//! Classify the fullscreen foreground app (SPEC §7.1).

use crate::config::GameConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullscreenKind {
    /// A game: starts a session, stats and break reminder.
    Game,
    /// Fullscreen video, slides, a browser: Seam only.
    Media,
}

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
];

pub fn classify(exe: &str, cfg: &GameConfig) -> FullscreenKind {
    let exe = crate::format::exe_name(exe);
    let eq = |list: &[String]| list.iter().any(|g| crate::format::exe_name(g) == exe);
    if eq(&cfg.games) {
        FullscreenKind::Game
    } else if eq(&cfg.not_games) || KNOWN_NOT_GAMES.contains(&exe.as_str()) {
        FullscreenKind::Media
    } else {
        FullscreenKind::Game
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        let mut cfg = GameConfig::default();
        assert_eq!(classify("C:\\Games\\Witcher3.exe", &cfg), FullscreenKind::Game);
        assert_eq!(classify("C:\\Program Files\\Google\\Chrome\\chrome.exe", &cfg), FullscreenKind::Media);
        cfg.not_games.push("Witcher3.EXE".into());
        assert_eq!(classify("witcher3.exe", &cfg), FullscreenKind::Media);
        cfg.games.push("chrome.exe".into());
        assert_eq!(classify("chrome.exe", &cfg), FullscreenKind::Game);
    }
}
