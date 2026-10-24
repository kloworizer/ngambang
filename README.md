# Ngambang

[![CI](https://github.com/kloworizer/ngambang/actions/workflows/ci.yml/badge.svg)](https://github.com/kloworizer/ngambang/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/kloworizer/ngambang?include_prereleases&label=release)](https://github.com/kloworizer/ngambang/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

An edge-docked floating island for Windows. A small orb rests on any screen
edge, shows what is running on a progress ring, and unfolds into slips:
now playing, a per-app volume mixer, command results, local ports, a
developer clipboard, a focus timer, game stats, new captures and a
mic/camera alert.

*Ngambang* means "floating, drifting on the surface" in Javanese/Indonesian.

- **Native and light.** Rust and the Windows compositor; no browser engine,
  no .NET runtime. Idle means no timers and no polling.
- **Yours to shape.** Every module can be turned on or off on its own, from
  the tray menu or `config.toml`. Nothing is bundled into fixed profiles.
- **Never in the way.** When a game or video goes fullscreen, the island
  sinks to a 3 px line on the edge.

Mockup: <https://claude.ai/artifact/S2ADpcUg2EJqHLpewaD7uZ> · Specification: [docs/SPEC.md](docs/SPEC.md)

> **Status: 0.1.0, early.** Everything below is implemented, unit-tested
> where it is platform-independent, and built and self-tested on Windows in
> CI. It has not yet been through the full manual test matrix on real
> hardware ([SPEC §12](docs/SPEC.md#12-testing)), so expect rough edges.

## The island

| State | What you see | When |
|-------|--------------|------|
| **Seam** | a 3 px line on the edge | a fullscreen app has focus |
| **Orb** | a dot with the progress ring ("the tide") | resting |
| **Tag** | a label sliding out of the orb | hover, or a new event |
| **Slips** | a card with index tabs for every active module | click, or the hotkey |

| Input | Action |
|-------|--------|
| Hover the orb | Show what is running |
| Click | Open the slips; click again or move away to close |
| Mouse wheel on the orb | System volume |
| Shift + wheel on the orb | Next / previous track |
| Middle-click the orb | Play / pause |
| Wheel on the slips, or ↑ / ↓ | Flip between slips |
| Drag the orb | Move it to another edge or monitor |
| `Win+Alt+Space` | Open with the keyboard (`Tab`, `Enter`, `←`/`→` on sliders, `Esc`) |
| Right-click the orb or tray icon | Modules, dock edge, pause, autostart, config |

## Modules

All modules start enabled. Switch any of them off in the tray menu
(**Modules ▸**) or in `[modules]` of the config file; a module that is off
costs nothing.

| Key | Module | What it does |
|-----|--------|--------------|
| `media` | Media player | Now playing from every app with Windows media controls (music apps, browsers, video players). Switch between sources; play, pause, skip, seek. |
| `mixer` | Volume mixer | Volume and mute for **every** app making sound, including games and voice chat. Rows say whether the app also has media controls. |
| `commands` | Command tags | `ngambang notify / done / run` and shell hooks: ✓ or ✕ tags when long commands finish, with a button to jump back to the terminal. |
| `ports` | Local ports | Servers listening on localhost; open in the browser or stop (with confirmation). |
| `clipboard` | Dev clipboard | Recognises JWT, JSON, base64, Unix time, UUID, hex colours and tracking links, and offers one-tap transforms. Skips anything a password manager marks private. |
| `timer` | Focus timer | 25 / 50-minute rounds on the ring; holds back minor tags while it runs. |
| `game` | Game stats & sessions | Session time, GPU load, VRAM, NVIDIA temperature, ping, a break reminder, today's play time and an optional power-plan switch. |
| `captures` | Screenshots & clips | Tags new files in `Videos\Captures`, `Pictures\Screenshots` and folders you add; copy or reveal them. |
| `privacy` | Mic & camera beacon | Alerts when an app uses the microphone or camera — even during games — with a button to mute the mic. |

### What "control all apps" means

Windows offers two different hooks, and Ngambang uses both:

- **Playback** (title, play/pause, skip, seek) works for every app that
  publishes Windows media controls.
- **Volume and mute** work for every process that outputs audio, whether or
  not it has media controls.

If nothing publishes media controls, middle-click sends the system
play/pause key.

## Install

1. Download `ngambang-<version>-windows-x64.zip` (or `-arm64`) from
   [Releases](https://github.com/kloworizer/ngambang/releases).
2. Unzip it somewhere permanent, e.g. `%LOCALAPPDATA%\Programs\Ngambang`, and
   add that folder to your `PATH` if you want the CLI everywhere.
3. Run `ngambangw.exe` (or `ngambang` in a terminal). Turn on
   **Start with Windows** from the tray menu.

Requires Windows 11, or Windows 10 1903 or newer. The builds are not
code-signed yet, so SmartScreen may warn on first launch.

## Command line

```powershell
ngambang                                   # start the island
ngambang notify "deploy staging" --sticky  # a running job on the ring
ngambang notify "deploy staging" --progress 40
ngambang done "deploy staging" --ok
ngambang run -- cargo build --release      # runs it, reports ✓/✕ and time, keeps the exit code
ngambang hook powershell                   # print the long-command hook for your shell
ngambang self-test                         # check drawing, the pipe and the config
ngambang config-path
```

Add the hook to your shell to get a tag whenever a command runs longer than
30 seconds:

| Shell | Add to | Line |
|-------|--------|------|
| PowerShell | `$PROFILE` | `ngambang hook powershell \| Out-String \| Invoke-Expression` |
| bash (Git Bash, WSL) | `~/.bashrc` | `eval "$(ngambang.exe hook bash)"` |
| zsh (WSL) | `~/.zshrc` | `eval "$(ngambang.exe hook zsh)"` |

More (git hooks, Claude Code hooks, the pipe protocol): [docs/integrations.md](docs/integrations.md).

## Configuration

`%APPDATA%\Ngambang\config.toml` is created on first run with every option
commented. Changes apply as soon as you save. The most useful keys:

```toml
[dock]
edge = "left"            # left | right | top | bottom
offset = 0.45            # position along the edge
hotkey = "Win+Alt+Space"

[modules]
ports = false            # e.g. turn off what you don't use

[game]
break_after_min = 90     # 0 = no reminder
power_plan = ""          # a power plan GUID to use while gaming

[privacy]
hide_from_capture = false  # hide the island from screenshots and screen sharing
```

State (dock position, today's play time) lives in
`%LOCALAPPDATA%\Ngambang\state.json`; warnings go to `ngambang.log` next to it.

## Privacy

- No telemetry. The only network traffic is ICMP ping to your gateway (or a
  host you choose), and only while game stats are on screen.
- Clipboard text is kept in memory only while it is shown, and never logged.
- The CLI pipe accepts only your user account and only local connections.

## Known limitations (0.1.0)

- Games in legacy *exclusive* fullscreen cover all top-most windows, so the
  Seam is not visible there; borderless and modern fullscreen work.
- GPU temperature is NVIDIA-only; FPS is not shown yet (needs elevation).
- Album art is a placeholder tile; fonts are Segoe UI Variable / Cascadia Mono.
- Chat and notification tags need an MSIX package and are planned for 0.3.
- Windows has no public API for Do Not Disturb, so the island only quiets
  its own tags.

See the roadmap in [SPEC §14](docs/SPEC.md#14-roadmap-after-v01).

## Building

```sh
cargo test --workspace                       # core logic, runs on any OS
cargo build --release                        # on Windows: target/release/ngambang(w).exe
cargo check --target x86_64-pc-windows-msvc  # type-check the Windows layer from Linux/macOS
```

| Crate | Contents |
|-------|----------|
| `crates/core` | State machine, layout, drawing scene, config, protocol, clipboard detectors — no OS calls, fully unit-tested |
| `crates/platform` | Windows: window, compositor, Direct2D, SMTC, WASAPI, pipe, watchers, tray |
| `crates/app` | `ngambang.exe` (CLI) and `ngambangw.exe` (island) |

## Releases and versioning

Versions follow [SemVer](https://semver.org/), starting at **0.1.0**. The
version lives once, in `[workspace.package]` of `Cargo.toml`, and
[CHANGELOG.md](CHANGELOG.md) records each release.

- **CI** (`.github/workflows/ci.yml`) runs on every push and pull request:
  format, clippy, tests on Linux and Windows, release builds for x64 and
  ARM64, and `ngambang self-test`.
- **Release** (`.github/workflows/release.yml`) runs on a `vX.Y.Z` tag, or by
  hand from the Actions tab with a choice of `none / patch / minor / major`.
  A bump updates `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md`, commits,
  tags and publishes zipped builds with SHA-256 checksums. Releasing 0.1.0:
  run the workflow with `none`.
- **Pages** (`.github/workflows/pages.yml`) deploys `site/` when it changes on
  `main`. Enable it once under *Settings → Pages → Source: GitHub Actions*.

## License

[MIT](LICENSE)
