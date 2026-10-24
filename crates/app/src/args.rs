//! Command-line parsing for `ngambang`. Pure, so it is tested on every OS.

use ngambang_core::config::Config;
use ngambang_core::protocol::{Level, Request};

pub const USAGE: &str = "\
Ngambang — an edge-docked island for Windows

USAGE:
    ngambang [start]                      Start the island (ngambangw.exe)
    ngambang notify <title> [options]     Show a tag, or track a running job
        --id <id>          Update this job instead of creating one
        --progress <0..1>  Show progress on the ring
        --level <info|ok|warn|fail>
        --body <text>
        --sticky           Keep it as a running job until `done`
        --ttl <ms>         How long the tag stays (1000–60000)
    ngambang done <id> (--ok | --fail) [--code <n>] [--duration <secs>]
    ngambang clear <id>
    ngambang run [--id <id>] -- <command> [args…]
                                          Run a command and report how it ended
    ngambang hook <powershell|bash|zsh>   Print a shell snippet for long commands
    ngambang self-test [--strict]         Check drawing, pipe and config
    ngambang config-path                  Print the config file location
    ngambang --version | --help

Add --quiet to notify/done/clear to stay silent when the island is not running.";

#[derive(Debug, PartialEq)]
pub enum Command {
    Start,
    Send { req: Request, quiet: bool },
    Run { id: Option<String>, argv: Vec<String> },
    Hook(Shell),
    SelfTest { strict: bool },
    ConfigPath,
    Version,
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum Shell {
    PowerShell,
    Bash,
    Zsh,
}

fn value(p: &mut lexopt::Parser, flag: &str) -> Result<String, String> {
    p.value().map_err(|_| format!("{flag} needs a value"))?.into_string().map_err(|_| format!("{flag}: not valid text"))
}

pub fn parse(args: Vec<String>) -> Result<Command, String> {
    use lexopt::prelude::*;
    let mut p = lexopt::Parser::from_args(args);
    let first = match p.next().map_err(|e| e.to_string())? {
        None => return Ok(Command::Start),
        Some(Long("version")) | Some(Short('V')) => return Ok(Command::Version),
        Some(Long("help")) | Some(Short('h')) => return Ok(Command::Help),
        Some(Long("self-test")) => "self-test".to_string(),
        Some(Value(v)) => v.into_string().map_err(|_| "invalid command")?,
        Some(other) => return Err(other.unexpected().to_string()),
    };
    match first.as_str() {
        "start" => Ok(Command::Start),
        "help" => Ok(Command::Help),
        "version" => Ok(Command::Version),
        "config-path" => Ok(Command::ConfigPath),
        "self-test" => {
            let mut strict = false;
            while let Some(a) = p.next().map_err(|e| e.to_string())? {
                match a {
                    Long("strict") => strict = true,
                    other => return Err(other.unexpected().to_string()),
                }
            }
            Ok(Command::SelfTest { strict })
        }
        "hook" => {
            let shell = match p.next().map_err(|e| e.to_string())? {
                Some(Value(v)) => v.into_string().unwrap_or_default(),
                _ => return Err("hook needs a shell: powershell, bash or zsh".into()),
            };
            match shell.to_ascii_lowercase().as_str() {
                "powershell" | "pwsh" | "ps" => Ok(Command::Hook(Shell::PowerShell)),
                "bash" => Ok(Command::Hook(Shell::Bash)),
                "zsh" => Ok(Command::Hook(Shell::Zsh)),
                other => Err(format!("unknown shell \"{other}\"")),
            }
        }
        "notify" => {
            let (mut id, mut title, mut body, mut level, mut progress, mut sticky, mut ttl, mut quiet) =
                (None, None, None, Level::Info, None, false, None, false);
            while let Some(a) = p.next().map_err(|e| e.to_string())? {
                match a {
                    Long("id") => id = Some(value(&mut p, "--id")?),
                    Long("body") => body = Some(value(&mut p, "--body")?),
                    Long("level") => {
                        let v = value(&mut p, "--level")?;
                        level = Level::parse(&v).ok_or(format!("unknown level \"{v}\""))?;
                    }
                    Long("progress") => {
                        let v = value(&mut p, "--progress")?;
                        let f: f32 = v.parse().map_err(|_| format!("--progress: \"{v}\" is not a number"))?;
                        // Accept 0..1 or a percentage.
                        progress = Some(if f > 1.0 { f / 100.0 } else { f });
                    }
                    Long("sticky") => sticky = true,
                    Long("ttl") => {
                        let v = value(&mut p, "--ttl")?;
                        ttl = Some(v.parse().map_err(|_| format!("--ttl: \"{v}\" is not a number"))?);
                    }
                    Long("quiet") | Short('q') => quiet = true,
                    Value(v) if title.is_none() => title = Some(v.into_string().map_err(|_| "title: not valid text")?),
                    other => return Err(other.unexpected().to_string()),
                }
            }
            let title = title.ok_or("notify needs a title")?;
            Ok(Command::Send { req: Request::Notify { id, title, body, level, progress, sticky, ttl_ms: ttl, hwnd: None }, quiet })
        }
        "done" => {
            let (mut id, mut ok, mut code, mut duration, mut quiet) = (None, None, None, None, false);
            while let Some(a) = p.next().map_err(|e| e.to_string())? {
                match a {
                    Long("ok") => ok = Some(true),
                    Long("fail") => ok = Some(false),
                    Long("code") => {
                        let v = value(&mut p, "--code")?;
                        code = Some(v.parse().map_err(|_| format!("--code: \"{v}\" is not a number"))?);
                    }
                    Long("duration") => {
                        let v = value(&mut p, "--duration")?;
                        duration = Some(v.parse().map_err(|_| format!("--duration: \"{v}\" is not a number"))?);
                    }
                    Long("quiet") | Short('q') => quiet = true,
                    Value(v) if id.is_none() => id = Some(v.into_string().map_err(|_| "id: not valid text")?),
                    other => return Err(other.unexpected().to_string()),
                }
            }
            let id: String = id.ok_or("done needs an id")?;
            let ok = ok.or(code.map(|c| c == 0)).ok_or("done needs --ok or --fail")?;
            Ok(Command::Send { req: Request::Done { title: Some(id.clone()), id, ok, code, duration_s: duration, hwnd: None }, quiet })
        }
        "clear" => {
            let (mut id, mut quiet) = (None, false);
            while let Some(a) = p.next().map_err(|e| e.to_string())? {
                match a {
                    Long("quiet") | Short('q') => quiet = true,
                    Value(v) if id.is_none() => id = Some(v.into_string().map_err(|_| "id: not valid text")?),
                    other => return Err(other.unexpected().to_string()),
                }
            }
            Ok(Command::Send { req: Request::Clear { id: id.ok_or("clear needs an id")? }, quiet })
        }
        "run" => {
            let mut id = None;
            let mut argv = Vec::new();
            while let Some(a) = p.next().map_err(|e| e.to_string())? {
                match a {
                    Long("id") if argv.is_empty() => id = Some(value(&mut p, "--id")?),
                    Value(v) => {
                        argv.push(v.into_string().map_err(|_| "argument: not valid text")?);
                        argv.extend(p.raw_args().map_err(|e| e.to_string())?.map(|a| a.to_string_lossy().into_owned()));
                    }
                    other => return Err(other.unexpected().to_string()),
                }
            }
            if argv.is_empty() {
                return Err("run needs a command: ngambang run -- <command>".into());
            }
            Ok(Command::Run { id, argv })
        }
        other => Err(format!("unknown command \"{other}\" (see ngambang --help)")),
    }
}

/// The shell snippet with this config's threshold and ignore list filled in.
pub fn hook(shell: Shell, cfg: &Config) -> String {
    let template = match shell {
        Shell::PowerShell => include_str!("../assets/hooks/powershell.ps1"),
        Shell::Bash => include_str!("../assets/hooks/bash.sh"),
        Shell::Zsh => include_str!("../assets/hooks/zsh.zsh"),
    };
    let words: Vec<String> =
        cfg.commands.ignore.iter().map(|w| w.split_whitespace().next().unwrap_or("").to_string()).filter(|w| !w.is_empty()).collect();
    let ignore = match shell {
        Shell::PowerShell => format!("@({})", words.iter().map(|w| format!("'{}'", w.replace('\'', "''"))).collect::<Vec<_>>().join(", ")),
        _ => words
            .iter()
            .filter(|w| w.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)))
            .cloned()
            .collect::<Vec<_>>()
            .join(" "),
    };
    template.replace("{{THRESHOLD}}", &cfg.commands.long_command_s.to_string()).replace("{{IGNORE}}", &ignore)
}

/// Display text for a command line.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn command_line(argv: &[String]) -> String {
    argv.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn no_args_starts() {
        assert_eq!(parse(vec![]).unwrap(), Command::Start);
        assert_eq!(parse(args(&["-V"])).unwrap(), Command::Version);
    }

    #[test]
    fn notify() {
        let c = parse(args(&["notify", "deploy staging", "--progress", "40", "--sticky", "--quiet"])).unwrap();
        let Command::Send { req: Request::Notify { title, progress, sticky, .. }, quiet } = c else { panic!() };
        assert_eq!(title, "deploy staging");
        assert_eq!(progress, Some(0.4));
        assert!(sticky && quiet);
        assert!(parse(args(&["notify"])).is_err());
        assert!(parse(args(&["notify", "x", "--level", "loud"])).is_err());
    }

    #[test]
    fn done_needs_outcome() {
        let c = parse(args(&["done", "cargo test", "--fail", "--code", "101", "--duration", "12.5"])).unwrap();
        let Command::Send { req: Request::Done { id, ok, code, duration_s, title, .. }, .. } = c else { panic!() };
        assert_eq!((id.as_str(), ok, code, duration_s), ("cargo test", false, Some(101), Some(12.5)));
        assert_eq!(title.as_deref(), Some("cargo test"));
        assert!(parse(args(&["done", "x"])).is_err());
        let Command::Send { req: Request::Done { ok, .. }, .. } = parse(args(&["done", "x", "--code", "0"])).unwrap() else { panic!() };
        assert!(ok);
    }

    #[test]
    fn run_keeps_child_flags() {
        let c = parse(args(&["run", "--id", "b", "--", "cargo", "build", "--release"])).unwrap();
        assert_eq!(c, Command::Run { id: Some("b".into()), argv: args(&["cargo", "build", "--release"]) });
        let c = parse(args(&["run", "npm", "run", "build", "--", "--watch"])).unwrap();
        assert_eq!(c, Command::Run { id: None, argv: args(&["npm", "run", "build", "--", "--watch"]) });
        assert!(parse(args(&["run"])).is_err());
    }

    #[test]
    fn hooks_are_filled_in() {
        let cfg = Config::default();
        for shell in [Shell::PowerShell, Shell::Bash, Shell::Zsh] {
            let h = hook(shell, &cfg);
            assert!(!h.contains("{{"), "{shell:?}");
            assert!(h.contains("30"));
            assert!(h.contains("vim"));
        }
        assert!(hook(Shell::PowerShell, &cfg).contains("@('vim', 'nvim'"));
        assert!(matches!(parse(args(&["hook", "pwsh"])).unwrap(), Command::Hook(Shell::PowerShell)));
        assert!(parse(args(&["hook", "fish"])).is_err());
    }

    #[test]
    fn unknown_command() {
        assert!(parse(args(&["explode"])).is_err());
        assert_eq!(command_line(&args(&["git", "commit", "-m", "a b"])), "git commit -m \"a b\"");
    }
}
