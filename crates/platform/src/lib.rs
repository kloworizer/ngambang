//! Windows platform layer: window, compositor, Direct2D drawing and every OS
//! source the island listens to. On other targets only the pieces needed to
//! build the workspace exist.

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::{cli, run, self_test, RunOptions};

/// Paths used by the app (same on every OS so the CLI can print them).
pub mod paths {
    use std::path::PathBuf;

    fn base(var: &str) -> PathBuf {
        std::env::var_os(var).map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Ngambang")
    }

    /// `%APPDATA%\Ngambang\config.toml`
    pub fn config_file() -> PathBuf {
        base("APPDATA").join("config.toml")
    }

    /// `%LOCALAPPDATA%\Ngambang`
    pub fn local_dir() -> PathBuf {
        base("LOCALAPPDATA")
    }

    pub fn state_file() -> PathBuf {
        local_dir().join("state.json")
    }

    pub fn log_file() -> PathBuf {
        local_dir().join("ngambang.log")
    }
}
