//! `ngambang` — the command-line tool. It starts the island, talks to it over
//! the local pipe, and prints shell hooks.

mod args;

use args::Command;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let code = match args::parse(argv) {
        Ok(cmd) => execute(cmd),
        Err(e) => {
            eprintln!("ngambang: {e}");
            64
        }
    };
    std::process::exit(code);
}

fn load_config() -> ngambang_core::config::Config {
    let path = ngambang_platform::paths::config_file();
    std::fs::read_to_string(path).map(|t| ngambang_core::config::Config::parse(&t).0).unwrap_or_default()
}

fn execute(cmd: Command) -> i32 {
    match cmd {
        Command::Help => {
            println!("{}", args::USAGE);
            0
        }
        Command::Version => {
            println!("ngambang {}", ngambang_core::VERSION);
            0
        }
        Command::ConfigPath => {
            println!("{}", ngambang_platform::paths::config_file().display());
            0
        }
        Command::Hook(shell) => {
            print!("{}", args::hook(shell, &load_config()));
            0
        }
        other => platform(other),
    }
}

#[cfg(windows)]
fn platform(cmd: Command) -> i32 {
    use ngambang_core::protocol::Request;
    use ngambang_platform::cli::{self, SendError};

    let report = |r: Result<(), SendError>, quiet: bool| -> i32 {
        match r {
            Ok(()) => 0,
            Err(_) if quiet => 0,
            Err(SendError::NotRunning) => {
                eprintln!("ngambang: the island is not running (start it with `ngambang`)");
                2
            }
            Err(SendError::Rejected(e)) => {
                eprintln!("ngambang: {e}");
                1
            }
            Err(SendError::Io(e)) => {
                eprintln!("ngambang: {e}");
                1
            }
        }
    };

    match cmd {
        Command::Start => match cli::start_island() {
            Ok(()) => {
                println!("Ngambang started. Right-click the orb or its tray icon for options.");
                0
            }
            Err(e) => {
                eprintln!("ngambang: cannot start ngambangw.exe: {e}");
                1
            }
        },
        Command::SelfTest { strict } => ngambang_platform::self_test(strict),
        Command::Send { mut req, quiet } => {
            let hwnd = cli::terminal_window();
            match &mut req {
                Request::Notify { hwnd: h, .. } | Request::Done { hwnd: h, .. } => *h = hwnd,
                _ => {}
            }
            report(cli::send(&req), quiet)
        }
        Command::Run { id, argv } => {
            let title = args::command_line(&argv);
            let id = id.unwrap_or_else(|| title.clone());
            let hwnd = cli::terminal_window();
            let _ = cli::send(&Request::Notify {
                id: Some(id.clone()),
                title: title.clone(),
                body: None,
                level: Default::default(),
                progress: None,
                sticky: true,
                ttl_ms: None,
                hwnd: hwnd.clone(),
            });
            let start = std::time::Instant::now();
            let status = std::process::Command::new(&argv[0]).args(&argv[1..]).status().or_else(|e| {
                // npm, yarn and friends are .cmd scripts: run them through cmd.
                if e.kind() == std::io::ErrorKind::NotFound {
                    std::process::Command::new("cmd").arg("/C").args(&argv).status()
                } else {
                    Err(e)
                }
            });
            let (ok, code) = match &status {
                Ok(s) => (s.success(), s.code()),
                Err(e) => {
                    eprintln!("ngambang: cannot run {}: {e}", argv[0]);
                    (false, Some(127))
                }
            };
            let _ = cli::send(&Request::Done { title: Some(title), id, ok, code, duration_s: Some(start.elapsed().as_secs_f64()), hwnd });
            code.unwrap_or(if ok { 0 } else { 1 })
        }
        _ => 0,
    }
}

#[cfg(not(windows))]
fn platform(_: Command) -> i32 {
    eprintln!("ngambang: this command needs Windows");
    1
}
