//! Platform-independent core of Ngambang: configuration, the island state
//! machine, layout, the drawing scene and the CLI protocol. Nothing here
//! touches Windows, so it builds and tests on any OS.

pub mod calendar;
pub mod clipboard;
pub mod config;
pub mod finance;
pub mod format;
pub mod games;
pub mod geometry;
pub mod hotkey;
pub mod icons;
pub mod layout;
pub mod model;
pub mod prayer;
pub mod protocol;
pub mod settings;
pub mod soft;
pub mod theme;
pub mod update;
pub mod view;
pub mod weather;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
