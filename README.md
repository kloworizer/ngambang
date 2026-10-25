# Ngambang

[![CI](https://github.com/kloworizer/ngambang/actions/workflows/ci.yml/badge.svg)](https://github.com/kloworizer/ngambang/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/kloworizer/ngambang?include_prereleases&label=release)](https://github.com/kloworizer/ngambang/releases)
[![License: MIT + Commons Clause](https://img.shields.io/badge/license-MIT%20%2B%20Commons%20Clause-blue.svg)](LICENSE)

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

Specification: [docs/SPEC.md](docs/SPEC.md)

## The island

| State | What you see | When |
|-------|--------------|------|
| **Seam** | a 3 px line on the edge | a fullscreen app has focus |
| **Orb** | a dot with the progress ring ("the tide") | resting |
| **Tag** | a label that pops out of the orb | hover, or a new event (new song, weather change, a warning, prayer time …) |
| **Slips** | a card with index tabs for every active module (tabs along the bottom when docked to the top or bottom) | click, or the hotkey |

| Input | Action |
|-------|--------|
| Hover the orb | Show what is running; while music plays, the tag has the cover, artist and ⏮ ⏯ ⏭ |
| Click | Open the slips; click again or move away to close |
| Mouse wheel on the orb or a tag | Volume of the player that is playing (the system volume when nothing plays); the level shows as you scroll |
| Shift + wheel on the orb | Next / previous track |
| Middle-click the orb | Play / pause |
| Wheel on the slips, or ↑ / ↓ | Flip between slips |
| Drag the orb, a tag or a slip's background | Move it to another edge or monitor; it follows the pointer as you drag |
| `Win+Alt+Space` | Open with the keyboard (`Tab`, `Enter`, `←`/`→` on sliders, `Esc`) |
| Right-click the orb or tray icon | Settings, modules, dock edge, pause, start with Windows |

## Modules

Every module except Jadwal sholat starts enabled. Switch any of them on or
off in **Settings…**, the tray menu (**Modules ▸**) or `[modules]` of the
config file; a module that is off costs nothing.

| Key | Module | What it does |
|-----|--------|--------------|
| `media` | Media player | Now playing from every app with Windows media controls (music apps, browsers, video players), with album art and artist. Switch between sources; play, pause, skip, seek. A new track pops up with its cover and controls. |
| `mixer` | Volume mixer | Volume and mute for **every** app making sound, including games and voice chat. Rows say whether the app also has media controls. |
| `commands` | Command tags | `ngambang notify / done / run` and shell hooks: ✓ or ✕ tags when long commands finish, with a button to jump back to the terminal. |
| `ports` | Local ports | Servers listening on localhost; open in the browser or stop (with confirmation). |
| `clipboard` | Dev clipboard | Recognises JWT, JSON, base64, Unix time, UUID, hex colours and tracking links, and offers one-tap transforms. Skips anything a password manager marks private. |
| `timer` | Focus timer | 25 / 50-minute rounds, or a custom length set with − and +, on the ring; holds back minor tags while it runs. |
| `game` | Game stats & sessions | Session time, GPU load, VRAM, NVIDIA temperature, ping, a break reminder, today's play time and an optional power-plan switch. |
| `captures` | Screenshots & clips | Tags new files in `Videos\Captures`, `Pictures\Screenshots` and folders you add; copy or reveal them. |
| `privacy` | Mic & camera beacon | Alerts when an app uses the microphone or camera — even during games — with a button to mute the mic. |
| `weather` | Weather & early warnings | Current weather and the next hours from [Open-Meteo](https://open-meteo.com), plus early warnings from [GDACS](https://www.gdacs.org) (the UN/EU Global Disaster Alert and Coordination System: earthquakes, tsunamis, cyclones, floods, volcanoes, droughts, wildfires) and [USGS](https://earthquake.usgs.gov) earthquake reports. Disasters near your location raise a tag; tsunami and Red alerts come through even during games. |
| `prayer` | Jadwal sholat | Prayer times for your location, calculated offline (Kemenag RI by default; MWL, ISNA, Egypt, Umm al-Qura, JAKIM, Karachi), with a reminder before each prayer and a tag at the time. Off by default. |
| `todo` | To-do list | A short list of tasks: type one straight into the slip, tick it off, clear finished ones. Saved between restarts. |
| `calendar` | Calendar | This week and your upcoming events from Google Calendar or any iCal calendar (link as many as you like in Settings ▸ Calendar), with a reminder before each event. |
| `finance` | Markets watchlist | Prices, today's change and a sparkline for shares, indices, crypto and currencies (Yahoo Finance). Add symbols in Settings ▸ Markets by typing a name. Loads only while the slip is open. |

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

More (git hooks, coding-agent hooks, the pipe protocol): [docs/integrations.md](docs/integrations.md).

## Settings

Right-click the orb or the tray icon and choose **Settings…**. Pages are
listed in the sidebar (`Ctrl+Tab` moves to the next one):

| Page | Options |
|------|---------|
| General | Start with Windows · reduce motion · hide from screenshots and screen sharing · sink to a thin line in fullscreen apps · hotkey · screen edge · monitor · position along the edge |
| Modules | Turn each of the fourteen modules on or off and set the order of their tabs |
| Media & clipboard | Pop-up for new tracks · volume step · clipboard tags |
| Focus & commands | Timer presets · custom round length · long-command threshold |
| Games | Break reminder · ping host · power plan to use while gaming (picked from your plans) |
| Location & prayer | A city from the list (about 240 places: Indonesia in depth, Malaysia, Singapore, Brunei, the Middle East and large cities worldwide; Jakarta by default), your own coordinates, or **Use my location** · calculation method · Asr (standard or Hanafi) · reminder minutes |
| Weather & updates | Disaster pop-ups on/off · earthquake radius · automatic update checks (check now from the right-click menu) |
| Calendar | Link calendars by their secret iCal address (Google Calendar ▸ Settings ▸ your calendar ▸ Integrate calendar), name and colour; pick one to edit or remove it. Indonesian public holidays are linked by default · reminder minutes · days ahead |
| Markets | Search box with live matches (shares, indices, crypto, currency pairs) · your watchlist with Move up / Move down / Remove |

Everything applies as soon as you press **Save**; nothing needs a restart.
The tray menu also has quick toggles for modules, dock edge, pause and
Start with Windows.

## Configuration file

Settings are stored in `%APPDATA%\Ngambang\config.toml`, created on first run
with every option commented. You can edit it directly (**Open config file**
in Settings); changes apply as soon as you save. Saving from the Settings
window rewrites only the lines it changes, so your comments stay. The most
useful keys:

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

- No telemetry and no accounts. Ngambang only makes these requests:
  - weather from Open-Meteo for your location's coordinates (every 30 min),
  - early warnings from GDACS and USGS (every 5 min),
  - one update check per day to GitHub Releases,
  - your linked calendars' iCal addresses (every 15 min, Calendar module),
  - prices for your watchlist from Yahoo Finance (every minute, only while
    the Markets slip is open), and symbol search while you type in Settings,
  - ICMP ping to your gateway while game stats are on screen.

  Turn off the Weather module and automatic update checks and none of these
  run except the ping.
- **Works offline.** Before each request Ngambang asks Windows whether the
  computer is online. Without a connection nothing is requested and no
  errors pop up: the weather slip keeps the last data marked
  "No connection · showing data from …", prayer times keep working (they are
  calculated on the device), and everything refreshes when the connection
  returns.
- Clipboard text is kept in memory only while it is shown, and never logged.
- The CLI pipe accepts only your user account and only local connections.

## Updates

Ngambang checks GitHub Releases once a day. When a new version is out, a tag
offers **Install**; it downloads the zip, verifies its SHA-256 checksum,
replaces the files and restarts. You can also use **Check for updates** in
the tray menu or Settings. Updating needs the install folder to be writable
(for example `%LOCALAPPDATA%\Programs\Ngambang`).

## Troubleshooting

`ngambang doctor` prints what the island can see: whether it is running, the
internet status, and every microphone/camera use Windows has recorded. Run it
with the app in question open and include the output in a bug report.

## Known limitations

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

Ngambang is free to use, change and share under the [MIT License with the Commons Clause](LICENSE).

| | |
|---|---|
| **Allowed** | Using it for anything: personal use, work and client projects, streaming and monetized channels, schools, companies. Changing it, and sharing it or your changed version for free (with the license). |
| **Not allowed** | Selling Ngambang: selling copies or a changed version of it, charging for it as part of a product, or offering a paid service (hosting, support, consulting) whose value comes mainly from Ngambang. |

To sell Ngambang or build a paid product or service on it, ask for a separate license: esha@angkasa.dev.

The Commons Clause makes this "source available" rather than open source in the OSI sense. Third-party software used by Ngambang keeps its own license: see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
