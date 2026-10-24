//! Windows implementation.

#[macro_use]
pub(crate) mod log;
pub mod app;
pub mod audio;
pub mod cli;
pub mod clipboard;
pub mod comp;
pub mod gfx;
pub mod media;
pub mod pipe;
pub mod ports;
pub mod selftest;
pub mod settings;
pub mod system;
pub mod tray;
pub mod util;
pub mod watch;

pub use app::{run, RunOptions};
pub use selftest::run as self_test;
