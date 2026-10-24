//! The island itself, built for the Windows GUI subsystem (no console window).
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    let verbose = std::env::args().any(|a| a == "--verbose");
    std::process::exit(ngambang_platform::run(ngambang_platform::RunOptions { verbose }));
}

#[cfg(not(windows))]
fn main() {
    eprintln!("ngambangw runs on Windows only.");
    std::process::exit(1);
}
