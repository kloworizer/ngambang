//! Helpers for the `ngambang` command-line tool.

use ngambang_core::protocol::Request;

pub use super::pipe::SendError;

/// Send a request to the running island.
pub fn send(req: &Request) -> Result<(), SendError> {
    let name = super::pipe::default_pipe_name().map_err(|e| SendError::Io(e.to_string()))?;
    super::pipe::send(&name, &req.to_line())
}

/// The top-level window of the terminal this command runs in, as "0x…".
pub fn terminal_window() -> Option<String> {
    use windows::Win32::System::Console::GetConsoleWindow;
    use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOTOWNER};
    unsafe {
        let console = GetConsoleWindow();
        if console.0.is_null() {
            return None;
        }
        let root = GetAncestor(console, GA_ROOTOWNER);
        let h = if root.0.is_null() { console } else { root };
        Some(format!("0x{:x}", h.0 as usize))
    }
}

/// Start the island (`ngambangw.exe` next to this executable) detached.
pub fn start_island() -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let exe = std::env::current_exe()?;
    let island = exe.with_file_name("ngambangw.exe");
    std::process::Command::new(island).creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP).spawn().map(|_| ())
}

/// `ngambang doctor`: what the island can see, for bug reports.
pub fn doctor() -> i32 {
    println!("Ngambang {}", ngambang_core::VERSION);
    println!("config: {}", crate::paths::config_file().display());
    println!("state:  {}", crate::paths::state_file().display());
    println!("log:    {}", crate::paths::log_file().display());
    match super::pipe::default_pipe_name().map(|n| super::pipe::send(&n, &ngambang_core::protocol::Request::Ping.to_line())) {
        Ok(Ok(())) => println!("island: running"),
        Ok(Err(SendError::NotRunning)) => println!("island: not running"),
        Ok(Err(e)) => println!("island: {e:?}"),
        Err(e) => println!("island: {e}"),
    }
    let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
    println!(
        "internet: {}",
        match super::net::online() {
            Some(true) => "connected",
            Some(false) => "offline",
            None => "unknown",
        }
    );
    println!("exclusive fullscreen app: {}", super::system::exclusive_d3d());
    println!();
    println!("Microphone and camera use recorded by Windows:");
    let report = super::watch::privacy_report();
    if report.is_empty() {
        println!("  (nothing recorded)");
    }
    for line in report {
        println!("  {line}");
    }
    0
}
