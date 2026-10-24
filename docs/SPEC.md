# Ngambang — v0.1 Product & Technical Specification

> *ngambang* (Javanese/Indonesian): floating, drifting on the surface.

Ngambang is an edge-docked "island" for Windows: a small orb that lives on any
screen edge, shows ambient progress, and unfolds into slips for media, coding
and gaming. It is built natively (Rust + Windows composition) to idle at
**0 % CPU and under 12 MB private working set**.

---

## 1. Final feature recommendation

### 1.1 What ships in v0.1

There are no fixed personas or profiles. Everything beyond the shell is a
**module** the user turns on or off on its own — from the tray menu
(Modules ▸) or `[modules]` in `config.toml`. A module that is off starts no
thread, registers no callback and allocates nothing.

| # | Feature | Module key | Why it is in v0.1 |
|---|---------|------------|-------------------|
| 1 | Island shell: Seam / Orb / Tag / Slips, any-edge docking, eased morph transitions | *(always on)* | The product's identity; everything else hangs off it |
| 2 | Game/fullscreen detection → Seam | *(always on)* | The island must never get in the way of a fullscreen app |
| 3 | **Media: every source** (all SMTC sessions, switchable) | `media` | Most-used feature of any island |
| 4 | **Per-app mixer** (volume/mute for every process making sound) | `mixer` | Makes "control all apps" true even for apps without media controls |
| 5 | Orb gestures: wheel = volume, Shift+wheel = skip, middle-click = play/pause | `media` / `mixer` | Zero-UI control, unique to an edge orb |
| 6 | **`ngambang` CLI + named pipe** (notify / progress / done / run) | `commands` | One integration point for every tool, script, hook and agent |
| 7 | Long-command tags via PowerShell / bash / zsh hooks | `commands` | "Tell me when the build finishes" without opening anything |
| 8 | **Ports slip** (listening localhost ports → open / stop) | `ports` | Answers "what's on :3000?" in one glance |
| 9 | **Dev clipboard** (JWT, JSON, base64, Unix time, UUID, hex colour, tracking URLs) | `clipboard` | High daily value, pure local logic |
| 10 | **Game peek + Game slip** (session time, GPU %, VRAM, NVIDIA temp, ping) | `game` | Glanceable stats without an overlay injector |
| 11 | Break reminder, per-game daily time, optional power-plan switch | `game` | Healthy, cheap, distinctive |
| 12 | Captures watcher (new screenshot / clip → tag, copy) | `captures` | Closes the "where did my clip go?" loop |
| 13 | Privacy beacon (mic / camera in use → alert, mute mic) | `privacy` | Must interrupt even during games; cheap to build |
| 14 | Focus timer | `timer` | Reuses the tide ring |
| 15 | Weather + early warnings (Open-Meteo; GDACS and USGS), offline-safe | `weather` | Added in 0.1.1 |
| 16 | Jadwal sholat: offline prayer times and reminders | `prayer` | Added in 0.1.1, off by default |
| 17 | Automatic updates from GitHub Releases | *(always on, can be disabled)* | Added in 0.1.1 |

**0.1.1 notes.** Network work runs on one worker thread (`net.rs`): WinHTTP
GETs with 10–15 s timeouts, preceded by Windows' `INetworkListManager`
connectivity check. Offline, nothing is requested; due work waits and the
connection is re-checked every minute. Weather refreshes every 30 min,
warnings every 5 min, update checks daily; failures retry after 10 min and
never raise tags. The last weather and warnings are cached in `state.json`.
Game detection now requires a game-library folder, exclusive fullscreen, or
≥ 20 % 3D GPU load by the process (Task Manager's counter), and ignores
screen overlays such as Snipping Tool.

### 1.2 Deliberately *not* in v0.1

| Feature | Reason | Target |
|---------|--------|--------|
| Album art in the media slip | WIC decode on demand; v0.1 shows a tinted placeholder tile | v0.2 |
| Embedded Instrument Sans / IBM Plex Mono | In-memory DirectWrite font loader; v0.1 uses Segoe UI Variable and Cascadia Mono/Consolas | v0.2 |
| Physical spring animations | `Spring*NaturalMotionAnimation.FinalValue` needs an `IReference<T>` windows-rs cannot build without a custom COM object; v0.1 uses overshooting cubic-bezier key frames | v0.2 |
| Chat / notification stack | `UserNotificationListener` needs package identity (MSIX or sparse package) and user consent | v0.3 |
| FPS / frame-time | PresentMon ETW needs admin or *Performance Log Users* membership | v0.2 (opt-in) |
| AMD / Intel GPU temperature | Needs ADLX / vendor SDKs; CPU temp needs a kernel driver (never ship WinRing0) | v0.2 (AMD) |
| Harbor (file shelf), colour picker, Docker slip, CI status | Valuable but not core to coder/gamer launch | v0.2–v0.3 |
| Output-device switching | Only possible via undocumented `IPolicyConfig` | v0.2 behind a flag |
| Toggling Windows Do Not Disturb | No public API exists; Ngambang suppresses only its own tags | — |
| Waktu Sholat, social screen-time, light theme, plugin API | Later modules | v0.3+ |

### 1.3 "Control all apps" — the exact promise

| Capability | Which apps | Mechanism |
|------------|-----------|-----------|
| See what's playing, play/pause, next/prev, seek | Every app that publishes Windows media controls (SMTC): most music/video apps and all major browsers (incl. web players) | `GlobalSystemMediaTransportControlsSessionManager` |
| Volume and mute | **Every process that outputs audio**, including games, voice chat, and apps without SMTC | WASAPI `IAudioSessionManager2` / `ISimpleAudioVolume` |
| Play/pause when no SMTC session exists | Whatever app the OS routes media keys to | `SendInput(VK_MEDIA_PLAY_PAUSE)` fallback |

The UI marks each mixer row as **controls** (SMTC matched) or **volume only**,
so users always know what a row can do.

---

## 2. Platform & targets

- **OS:** Windows 11 22H2+ (primary). Windows 10 22H2 best-effort.
- **Arch:** x86_64 and aarch64 (`*-pc-windows-msvc`).
- **Language:** Rust (stable, edition 2021), `windows` crate (windows-rs), no async runtime.
- **Rendering:** `Windows.UI.Composition` via desktop interop for shapes and
  animation; Direct2D + DirectWrite into composition drawing surfaces for text,
  icons and the wave. One D3D11 device (BGRA, no depth) shared by everything.
- **Distribution v0.1:** unpackaged single `ngambang.exe` (zip + winget
  manifest). Code-signed. MSIX/sparse package arrives with notifications in v0.3.

### 2.1 Performance budgets (release gate)

| Metric | Budget | How measured |
|--------|--------|--------------|
| Private working set, idle (orb) | **≤ 12 MB** | Task Manager "Memory (active private working set)" after 60 s idle |
| Private working set, slips open | ≤ 22 MB | same, with media + mixer open |
| Back to idle after collapse | ≤ 14 MB within 5 s | same |
| CPU, idle 10 min | **0.00 %**, < 1 wake-up/s | Windows Performance Recorder (CPU usage precise) |
| CPU, media playing, orb only | 0.00 % | ring is a composition animation, no ticks |
| Cold start → orb visible | ≤ 150 ms | `--self-test` timestamp log |
| Hover → expand animation starts | ≤ 16 ms | ETW frame timing |
| Binary size | ≤ 4 MB | release build |

"Working set" in Task Manager's default column includes mapped GPU-driver
images (often 20–35 MB with D3D). The budget is on *private* working set.

---

## 3. Visual specification

All sizes in DIPs (scale by monitor DPI). Mirror horizontally/vertically
according to the docked edge — "inner" = the side touching the edge.

### 3.1 Colour tokens

| Token | Hex | Use |
|-------|-----|-----|
| `surface` | `#15171A` | orb, tag, slip body |
| `surface-2` | `#1E2125` | inner tiles, inactive tabs |
| `surface-3` | `#23272C` | secondary buttons |
| `hairline` | `#FFFFFF` @ 8 % | 1 px border on all surfaces |
| `track` | `#3A3F45` | ring/progress track |
| `text` | `#ECEDEE` | primary text |
| `text-2` | `#9BA1A8` | secondary text (≥ 4.5:1 on surface) |
| `label` | `#8E959C` | mono uppercase labels |
| `media` | `#F2A93B` | media accent |
| `privacy` | `#FF6B4A` | mic/cam, failures, destructive |
| `game` | `#B7E04A` | gamer, focus timer |
| `ok` | `#7FD1AE` | success |
| `coder` | `#A99BFF` | CLI progress, dev clipboard |
| `ports` | `#4CC3D9` | ports, captures |

High contrast mode (`SPI_GETHIGHCONTRAST`): switch to system colours
(`COLOR_WINDOW`, `COLOR_WINDOWTEXT`, `COLOR_HIGHLIGHT`), drop shadows and accents.

### 3.2 Typography

- Sans: **Segoe UI Variable Text** (Windows 11), falling back to Segoe UI.
- Mono: **Cascadia Mono**, falling back to Consolas.
- v0.2: embed Instrument Sans and IBM Plex Mono (both OFL) with
  `IDWriteInMemoryFontFileLoader` to match the mockup exactly.
- Sizes: label 11 (mono, +8 % tracking, uppercase), body 13–14, title 17 semibold,
  big numerals 40–44 mono.
- Ship OFL licence texts in `assets/fonts/`.

### 3.3 Geometry

| Element | Spec |
|---------|------|
| **Seam** | 3 × 92, inner corners 0, outer 2; flush with the edge; may be split into stacked colour segments (one per live activity) |
| **Orb** | 30 Ø body, 22 Ø ring (2 px), 7 Ø core dot; hit target 44 × 44; 8 from edge |
| **Tag** | height 40; inner radius 12, outer radius 20 ("tied end tight, free end round"); padding 6 inner / 16 outer; auto width ≤ 360 |
| **Slip** | width 372; padding 18; radius 14, except the outer-leading corner 6 (where tabs attach); 12 from edge |
| **Index tabs** | 44 × 48, stacked with 6 gap on the slip's outer side, 10 from its top; active tab = `surface` + hairline, joined to the slip; inactive = `surface-2` |
| Buttons | ≥ 44 × 44 hit area; radius 10–12 |
| Shadow | composition `DropShadow`: blur 44, offset 18 away from the edge, black 45 % (orb/tag 30 / 10 / 35 %) |

Asymmetric radii: the body is a `CompositionRoundedRectangleGeometry` with the
large radius; a second small rectangle with the small radius overlaps the inner
side. Both are `CompositionSpriteShape`s in one `ShapeVisual`, so size
animations stay on the compositor.

### 3.4 The tide (signature element)

- **Ring:** `CompositionEllipseGeometry` + `TrimEnd`. For media and timers,
  `TrimEnd` runs a *linear* `ScalarKeyFrameAnimation` from the current fraction
  to 1.0 over the remaining duration — DWM animates it, the app does no work.
  Restart only on seek/pause/track change.
- **Wave:** a pre-built Direct2D sine path (period 11 DIP, amplitude 3.5) for
  the played part; flat 2 px line for the remainder. On pause, animate the wave
  visual's `Scale.Y` → 0.15 over 250 ms ("the tide goes flat").

### 3.5 Motion

Every state change morphs: the new scene's rounded clip starts as the old
shape and grows (or shrinks) to the new one while the two scenes crossfade.
All of it runs as compositor key-frame animations inside DWM.

| Transition | Animation (v0.1) | Params |
|------------|------------------|--------|
| Growing (Orb → Tag → Slips) | clip Size + Offset key frames | cubic-bezier (0.34, 1.56, 0.64, 1) — slight overshoot, 340 ms |
| Shrinking (Slips → Orb) | clip Size + Offset key frames | cubic-bezier (0.22, 1, 0.36, 1), 240 ms |
| Crossfade | Opacity | in 110 ms, out 130 ms |
| Tab flip | content crossfade 120 ms + 8 DIP slide | cubic ease-out |
| Seam ↔ Orb | opacity 150 ms | linear |
| Pause wave | `Scale.Y` 250 ms | ease-out |

If "Animation effects" is off (`SPI_GETCLIENTAREAANIMATION` = FALSE), all
transitions become instant with a 80 ms opacity fade. **No animation ever runs
while a game has focus.**

---

## 4. Shell behaviour

### 4.1 States

| State | Visible | Enters when |
|-------|---------|-------------|
| `Hidden` | nothing | user pause from tray |
| `Seam` | 3 px line | fullscreen app focused (§7.1), or `dock.style = "seam"` |
| `Orb` | orb + ring | default resting state |
| `Tag` | orb + tag | hover ≥ 120 ms with content, or a new event (auto-hide 4 s; failures 8 s) |
| `Slips` | slip + tabs | click on orb/tag, or hotkey |

Rules:
- Leaving `Slips`: collapse 600 ms after the pointer leaves (re-entry cancels);
  `Esc`; click elsewhere (`WM_ACTIVATE` deactivation when opened by hotkey).
- Event tags queue; one shown at a time, highest priority first (§4.4).
- In `Seam`, events change Seam segment colour only; hovering the edge 300 ms
  opens a one-line **peek** tag (game stats or the event).

### 4.2 Window

- `CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP, …, WS_POPUP)`.
- Per-monitor DPI awareness v2 (manifest). Handle `WM_DPICHANGED`, `WM_DISPLAYCHANGE`, `WM_SETTINGCHANGE`.
- Window bounds = bounding box of the *target* state, set at the start of an
  expand and at the end of a collapse. `SetWindowRgn` to the shape's rounded
  rect so clicks outside reach the desktop.
- Re-assert `HWND_TOPMOST` on `EVENT_SYSTEM_FOREGROUND` (taskbar and some apps steal it).
- Hover via `TrackMouseEvent(TME_HOVER | TME_LEAVE)`. Wheel arrives without focus
  ("scroll inactive windows", default on).
- Excluded from Alt-Tab (tool window). Visible in screenshots and screen
  shares by default; `privacy.hide_from_capture = true` applies
  `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`.

### 4.3 Docking

- Position = `{ monitor, edge: left|right|top|bottom, offset: 0.0–1.0 }`, relative
  to the monitor's **work area** (`MONITORINFO.rcWork`), so it never sits under the taskbar.
- Default: primary monitor, `left`, `0.45`.
- Re-dock: press-and-hold the orb 200 ms, drag; on release snap to the nearest
  edge of the monitor under the pointer; persist to `state.json`.
- Monitor identity: device interface path; fallback to primary when missing.

### 4.4 Priority (what goes in front)

Highest wins the tag queue, the ring and the front slip:

1. Privacy beacon (mic/cam active)
2. Break reminder / timer finished
3. Command failed
4. CLI progress (`--sticky`)
5. Command succeeded, capture saved
6. Media (playing)
7. Game stats
8. Media (paused), idle

During game mode, only 1–2 may raise a tag; everything else waits and is shown
as Seam segments.

### 4.5 Input

| Input | Action |
|-------|--------|
| Hover orb | Tag (if content) |
| Click orb / tag | Slips |
| Wheel over orb | System volume ±2 % per notch (ring shows level in `text` colour for 1.5 s) |
| Shift + wheel over orb | Next / previous track (front source) |
| Middle-click orb | Play/pause front source (media-key fallback if none) |
| Wheel over slips (not on a slider) | Flip to next/previous slip |
| `Win+Alt+Space` (configurable) | Toggle Slips with keyboard focus |
| Keyboard in Slips | `Tab`/`Shift+Tab` controls, `↑`/`↓` flip slips, `Space`/`Enter` activate, `Esc` close |
| Right-click orb | Same menu as tray |

Hotkey via `RegisterHotKey`; it grants foreground rights, so the window may
`SetForegroundWindow` itself for keyboard use, then return to no-activate on close.

### 4.6 Tray, startup, single instance

- Settings window (Settings… in the tray / orb menu): a native window with
  standard controls (keyboard and screen-reader friendly) for every common
  option, including Start with Windows and a power-plan picker. Saving
  validates the form (`core::settings`), rewrites only changed lines of
  `config.toml`, and the island reloads immediately.
- Tray icon (`Shell_NotifyIconW`): Settings…, Modules ▸ (one check item per module),
  Dock edge ▸, Pause island, Start with Windows ✓, Open config file, Open log
  folder, Quit. Left-click the tray icon opens the slips.
- Autostart: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- Single instance: mutex `Local\Ngambang-<user SID>`. A second launch with no
  arguments flashes the orb and exits; with arguments it runs as the CLI (§6.1).

---

## 5. Media module

### 5.1 Sources (SMTC)

- `GlobalSystemMediaTransportControlsSessionManager::RequestAsync()` once on the media worker.
- Subscribe: `SessionsChanged`, `CurrentSessionChanged`; per session
  `MediaPropertiesChanged`, `PlaybackInfoChanged`, `TimelinePropertiesChanged`.
- Ordering: playing first, then most recently changed. The front source drives
  the ring and tag. Source chips in the slip switch the shown session.
- Controls: `TryTogglePlayPauseAsync`, `TrySkipNextAsync`, `TrySkipPreviousAsync`,
  `TryChangePlaybackPositionAsync` — each enabled only if
  `PlaybackInfo.Controls.Is…Enabled`.
- App name: packaged AUMID → `AppInfo.DisplayInfo.DisplayName`; unpackaged
  (e.g. `Foo.exe`) → the exe's `FileDescription`, else the base name.
- Album art (v0.2): `MediaProperties.Thumbnail` → `OpenReadAsync` → WIC decode to
  120 × 120 premultiplied BGRA → drawing surface, only while the slip is
  open. v0.1 draws a tinted placeholder tile.
- Time label redraws once per second **only while the slip or tag is visible**.

### 5.2 Mixer (WASAPI)

- Default render endpoint (`eRender`, `eMultimedia`); rebuild on
  `IMMNotificationClient::OnDefaultDeviceChanged`.
- `IAudioSessionManager2::GetSessionEnumerator` (call once *before*
  `RegisterSessionNotification`, or new-session callbacks never fire) from an MTA thread.
- Per session: `IAudioSessionControl2::GetProcessId`, skip system sounds
  (`IsSystemSoundsSession` = S_OK) unless `media.show_system_sounds`.
  Group sessions by process. Subscribe `IAudioSessionEvents`
  (`OnSimpleVolumeChanged`, `OnStateChanged`, `OnSessionDisconnected`).
- Volume/mute: `ISimpleAudioVolume::SetMasterVolume/SetMute` with our own event
  context GUID so our echo is ignored.
- System row: `IAudioEndpointVolume` + `IAudioEndpointVolumeCallback`.
- Peak meters: `IAudioMeterInformation::GetPeakValue` at 30 Hz **only while the
  mixer is visible** (a `SetTimer` that is killed on collapse).
- Icons: `SHGetFileInfoW(SHGFI_ICON | SHGFI_SMALLICON)` of the process image, loaded on open, freed on collapse.
- **SMTC ↔ mixer link:** match a session's AUMID to a process (unpackaged: exe
  base name; packaged: `GetApplicationUserModelId` on the process handle). Matched
  rows show **controls**, others **volume only**.

### 5.3 Acceptance

- [ ] Two SMTC apps playing: both appear as chips; pausing one from the slip does not affect the other.
- [ ] A game with no SMTC appears in the mixer within 1 s of making sound and can be muted.
- [ ] Changing an app's volume in the Windows mixer updates our slider within 100 ms, and vice versa.
- [ ] Media playing, orb only, 10 minutes: 0.00 % CPU.

---

## 6. Coder module

### 6.1 CLI

Two executables ship side by side:

- `ngambangw.exe` — the island (GUI subsystem, no console window). Autostart points here.
- `ngambang.exe` — the console CLI. With no arguments it starts `ngambangw.exe`;
  otherwise it connects to the pipe, sends one line, prints nothing on success, and exits.

```
ngambang notify <title> [--id <id>] [--progress <0..1>] [--level info|ok|warn|fail]
                        [--body <text>] [--sticky] [--ttl <ms>]
ngambang done   <id|title> (--ok | --fail) [--code <n>] [--duration <secs>]
ngambang clear  <id>
ngambang run -- <command> [args…]        # runs it, reports start, duration, exit code
ngambang hook (powershell | bash | zsh)  # prints a profile snippet
```

Exit codes: `0` delivered, `2` island not running (silent with `--quiet`),
`64` usage error.

### 6.2 Pipe protocol v1

- Name: `\\.\pipe\ngambang-<user SID>`; DACL grants only the current user;
  `PIPE_REJECT_REMOTE_CLIENTS`; message mode; max 16 concurrent clients.
- One JSON object per line, ≤ 4 KB, UTF-8. Server replies one line.

```json
{"v":1,"op":"notify","id":"deploy","title":"deploy staging","level":"info","progress":0.4,"sticky":true}
{"v":1,"op":"done","id":"deploy","ok":true,"code":0,"duration_s":134,"hwnd":"0x1a2b3c"}
{"v":1,"op":"clear","id":"deploy"}
{"v":1,"op":"ping"}
```
Reply: `{"ok":true}` or `{"ok":false,"error":"…"}`.

- `id` defaults to a slug of `title`. Same `id` updates in place.
- `hwnd`: the CLI sends `GetAncestor(GetConsoleWindow(), GA_ROOTOWNER)` so
  **Open terminal** on a failed tag can bring that window forward (best-effort).
- Rate limit: 20 messages/s per client; extra lines are dropped.
- Unknown fields ignored; unknown `op` → error reply.

### 6.3 Shell hooks

`ngambang hook <shell>` prints a snippet; users add it to their profile.

- **PowerShell:** wrap `prompt`; read `Get-History -Count 1`
  (`StartExecutionTime`, `EndExecutionTime`) and `$?`/`$LASTEXITCODE`; if the
  duration ≥ `coder.long_command_s` (default 30), call `ngambang done`.
- **zsh:** `preexec` / `precmd`.
- **bash** (Git Bash, WSL): `trap … DEBUG` + `PROMPT_COMMAND`. In WSL the
  snippet calls `ngambang.exe` through interop.
- Ignored commands (configurable): `vim`, `nvim`, `less`, `ssh`, `top`, `htop`, `man`.
- Tag: `✓ <command> · 2m14s` (`ok`) or `✕ <command> · exit 1` (`privacy` colour, 8 s, **Open terminal** button).

Agent hooks (e.g. Claude Code `Stop`/`Notification` hooks) and git hooks use the plain CLI — documented in `docs/integrations.md` (v0.1 deliverable).

### 6.4 Ports slip

- `GetExtendedTcpTable(TCP_TABLE_OWNER_PID_LISTENER)` for `AF_INET` and `AF_INET6`.
- Show listeners on loopback or any-address, port ≥ 1024, excluding PID 0/4 and
  `svchost.exe`, `lsass.exe`, `services.exe`, `wininit.exe` (configurable).
- Process name: `QueryFullProcessImageNameW`.
- Refreshed **when the slip opens** and every 3 s while it stays open. No
  background polling.
- **Open** → `ShellExecuteW("http://localhost:<port>")`.
- **Stop** → first click turns the button into **Confirm** for 3 s; second click
  `TerminateProcess`. Processes we can't open with `PROCESS_TERMINATE`
  (elevated/other user) show a disabled button labelled *elevated*.

### 6.5 Dev clipboard

- `AddClipboardFormatListener` → `WM_CLIPBOARDUPDATE` (no polling).
- **Skip entirely** if the clipboard contains `ExcludeClipboardContentFromMonitorProcessing`,
  `Clipboard Viewer Ignore`, or `CanIncludeInClipboardHistory` = 0 (password managers set these).
- Only `CF_UNICODETEXT` ≤ 64 KB. Never written to disk or logs.
- Detectors (pure functions in `core`, evaluated in this order):

| Type | Rule | Actions |
|------|------|---------|
| JWT | 3 base64url parts; header decodes to JSON containing `alg` | show header/payload, `exp` as relative time, copy payload |
| JSON | parses as object/array | pretty, minify |
| URL with tracking | query has `utm_*`, `fbclid`, `gclid`, `si`, `igshid`, `mc_eid` | strip tracking |
| UUID | RFC 4122 shape | upper/lower, new UUID |
| Unix time | 10 or 13 digits, between 2000 and 2100 | local / UTC / ISO-8601 |
| Hex colour | `#rgb`, `#rrggbb`, `#rrggbbaa` | swatch, rgb(), hsl() |
| Base64 | valid alphabet, len % 4 = 0, decodes to UTF-8 ≥ 8 chars | decode, encode |

- On detection: 3 s tag `JWT · expired 3h ago` (toggle `coder.clipboard_tags`).
- Writes back to the clipboard set a private format `Ngambang.Self` so we ignore our own update.

### 6.6 Focus timer

- Presets 25 / 50 / custom; tray and slip start/stop/+5 min.
- Ring = timer progress (composition animation). On finish: tag 8 s, priority 2.
- While running, priorities 3–8 raise no tags (shown as Seam/ring only).

### 6.7 Acceptance

- [ ] `ngambang run -- cargo build` shows progress-less "running" tag, then ✓/✕ with duration.
- [ ] 100 `notify` calls in a loop: island stays responsive; at most 20/s accepted.
- [ ] A different Windows user cannot connect to the pipe.
- [ ] Copying from a password manager never produces a tag.
- [ ] `node` listening on :3000 appears within 3 s of opening the slip; Stop needs two clicks.

---

## 7. Gamer module

### 7.1 Game detection (event-driven)

Signals, no polling:
1. Register an app bar with `SHAppBarMessage(ABM_NEW)` (no `ABM_SETPOS`, so no
   space is reserved) → receive `ABN_FULLSCREENAPP` open/close.
2. `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` → on each change call
   `SHQueryUserNotificationState` (`QUNS_RUNNING_D3D_FULL_SCREEN`, `QUNS_BUSY`,
   `QUNS_PRESENTATION_MODE`) and compare the foreground window rect with its monitor rect.

Classification of a fullscreen foreground process:
- in `gamer.games` (user list) → **game**
- in `gamer.not_games` or a known browser/video player
  (`chrome`, `msedge`, `firefox`, `vlc`, `mpv`, `Video.UI`, …) → **fullscreen media** (Seam only, no timer)
- otherwise → **game**

A session ends 60 s after the game leaves the foreground fullscreen (so Alt-Tab
doesn't split sessions) or immediately when the process exits
(`WaitForSingleObject` on its handle in the watcher thread).

**Caveat (document in README):** true *exclusive* fullscreen (legacy DXGI/D3D9)
covers all top-most windows; the Seam and peek are visible in borderless and
"fullscreen optimizations" modes, which is the default for modern games.

### 7.2 Seam mode & peek

- Entering game mode → `Seam`. Segments: one per live activity, coloured by
  token; no motion, colour changes only.
- Peek (hover edge 300 ms, or hold the hotkey): one-line tag
  `1:32  GPU 87%  71°C  ping 18ms`. Fields hide when unavailable.

### 7.3 Game slip

Opens on hotkey, or when the user Alt-Tabs to the desktop.
- Session time, today's total for this game.
- **GPU %:** PDH `\GPU Engine(*engtype_3D)\Utilization Percentage`, summed per adapter.
- **VRAM:** PDH `\GPU Adapter Memory(*)\Dedicated Usage`.
- **Temperature:** NVIDIA only in v0.1 — `LoadLibrary("nvml.dll")` at runtime
  (ships with the driver), `nvmlDeviceGetTemperature`. Missing → field hidden.
- **Ping:** `IcmpSendEcho2` (async, event) to `gamer.ping_host`
  (default: the default gateway from `GetBestRoute2`); every 2 s.
- All sampling (PDH, NVML, ICMP) runs at 1 Hz / 0.5 Hz **only while the peek or
  slip is visible**.
- Latest capture card (§7.4), break button, performance-mode toggle.

### 7.4 Captures

- `ReadDirectoryChangesW` (`FILE_NOTIFY_CHANGE_FILE_NAME`) on
  `%USERPROFILE%\Videos\Captures`, `%USERPROFILE%\Pictures\Screenshots`, plus `gamer.capture_dirs`.
- New `.png .jpg .mp4 .mkv` → wait until the file can be opened exclusively
  (retry ≤ 10 s) → tag `Clip saved · 0:30` (priority 5; in game mode it waits as a Seam segment).
- Actions: Open (`ShellExecuteW`), Copy (`CF_HDROP`), Show in folder.
- Thumbnail via `IShellItemImageFactory::GetImage` only when the slip opens.

### 7.5 Break reminder & time tracking

- After `gamer.break_after_min` (default 90) of continuous session: Seam segment
  turns `media` colour and a peek tag shows for 6 s (priority 2). Snooze 15 min from the slip.
- `%LOCALAPPDATA%\Ngambang\state.json` keeps per-exe minutes for today and the
  last 7 days; resets at local midnight. Nothing else is recorded.

### 7.6 Performance mode (opt-in)

- `gamer.power_plan = "<GUID>"`: on game start `PowerGetActiveScheme` (save) →
  `PowerSetActiveScheme`; restore on session end and on app exit/crash
  (restore also attempted at next start if `state.json` says a switch is pending).
- Windows 11 "power mode" overlay uses undocumented `PowerSetActiveOverlayScheme`:
  behind `experimental.power_overlay = true`, off by default.

### 7.7 Acceptance

- [ ] Launching a borderless-fullscreen game switches to Seam within 250 ms; no Ngambang animation frames while it has focus (verify with WPR).
- [ ] Fullscreen YouTube in a browser → Seam, but no game session starts.
- [ ] Alt-Tab out and back within 60 s keeps one session.
- [ ] Game slip shows GPU % on NVIDIA, AMD and Intel; temperature only on NVIDIA; no errors when NVML is absent.
- [ ] Power plan is restored after the game exits and after killing Ngambang mid-game (on next start).

---

## 8. Privacy beacon

- Watch `HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\{microphone,webcam}`
  (subtree, including `NonPackaged`) with `RegNotifyChangeKeyValue(…, TRUE, REG_NOTIFY_CHANGE_LAST_SET, event, TRUE)`; re-arm after each signal.
- In use ⇔ `LastUsedTimeStart != 0 && LastUsedTimeStop == 0`. App name: packaged
  key → display name; `NonPackaged` key → path (`#` → `\`) → `FileDescription`.
- Shows a slip with app name and elapsed time; buttons **Mute mic** (default
  capture endpoint `IAudioEndpointVolume::SetMute`, both `eConsole` and
  `eCommunications`) and **Privacy settings** (`ms-settings:privacy-microphone` / `privacy-webcam`).
- Then retreats to a `privacy` Seam segment / ring until use stops.
- Priority 1: shown even in game mode and during focus.

---

## 9. Configuration

`%APPDATA%\Ngambang\config.toml`, hot-reloaded (`ReadDirectoryChangesW` on the folder).
Invalid values are ignored with a one-time tag `config: <key> invalid`.

The file written on first run is `DEFAULT_TOML` in `crates/core/src/config.rs`
(a test keeps it identical to `Config::default()`). Abridged:

```toml
[dock]
edge = "left"              # left | right | top | bottom
offset = 0.45
monitor = "primary"
hotkey = "Win+Alt+Space"
hide_in_fullscreen = true

[modules]                  # turn any of these off; the tray menu does the same
media = true
mixer = true
commands = true
ports = true
clipboard = true
timer = true
game = true
captures = true
privacy = true

[commands]
long_command_s = 30
ignore = ["vim", "nvim", "less", "more", "ssh", "top", "htop", "man", "watch"]

[game]
games = []
not_games = []
break_after_min = 90
ping_host = "gateway"
power_plan = ""
```

Tray toggles and orb drags rewrite only the affected `key = value` lines, so
comments survive. Unknown keys are ignored; a bad value resets only its table
and shows a one-time tag.

---

## 10. Architecture

### 10.1 Repository layout

```
ngambang/
├─ Cargo.toml                 # workspace, version, shared release profile
├─ crates/
│  ├─ core/                   # NO Windows deps — builds and tests on any OS
│  │  ├─ model.rs             # Model, Event, Effect, update() — the state machine
│  │  ├─ view.rs              # Model → Scene (primitives + hit regions)
│  │  ├─ icons.rs             # line icons from primitives
│  │  ├─ layout.rs            # window box and placement per edge
│  │  ├─ protocol.rs          # pipe messages, limits, rate limiter
│  │  ├─ clipboard.rs         # §6.5 detectors and transforms
│  │  ├─ config.rs            # TOML schema, defaults, validation, in-place edits
│  │  ├─ games.rs  hotkey.rs  format.rs  geometry.rs  theme.rs
│  ├─ platform/src/win/       # windows-rs; everything that touches the OS
│  │  ├─ app.rs               # window, message loop, effect execution
│  │  ├─ comp.rs  gfx.rs      # compositor + Direct2D/DirectWrite
│  │  ├─ media.rs  audio.rs   # SMTC, WASAPI
│  │  ├─ pipe.rs  cli.rs  ports.rs  clipboard.rs  system.rs  watch.rs  tray.rs
│  │  └─ selftest.rs  log.rs  util.rs
│  └─ app/                    # binaries: ngambang (CLI), ngambangw (island); hook templates
├─ scripts/version.py         # release versioning
├─ site/                      # GitHub Pages
└─ docs/                      # SPEC.md, integrations.md
```

### 10.2 Threads

| Thread | Apartment | Owns |
|--------|-----------|------|
| UI | STA | Win32 loop + `CreateDispatcherQueueController(DQTYPE_THREAD_CURRENT)`; window, compositor, D2D, tray, hotkey, clipboard listener, WinEvent hook, appbar |
| Media | MTA | SMTC manager and session events |
| Audio | MTA | WASAPI enumerator, session/endpoint callbacks |
| Pipe | — | overlapped `ConnectNamedPipe` server |
| Watcher | — | `WaitForMultipleObjects` over registry, directory, process and ICMP handles |

Workers never touch UI objects. They send `core::Event` over
`std::sync::mpsc` and `PostMessageW(hwnd, WM_APP_EVENT)` to wake the UI thread,
which runs `core::update(&mut model, event) -> Vec<Effect>`; the renderer applies
effects (start animation, redraw surface X, resize window, …). Each loop
iteration processes all queued events, then renders once.

Samplers that need timers (PDH, NVML, ICMP, peak meters, port refresh, the
1 s time label) are started by an `Effect::StartSampler(kind)` when the relevant
surface becomes visible and stopped by `Effect::StopSampler(kind)` when it
hides. **Nothing ticks while the island is idle.**

### 10.3 Rendering

- One `ID3D11Device` (`D3D11_CREATE_DEVICE_BGRA_SUPPORT`) → `ID2D1Device` →
  `CompositionGraphicsDevice` via `ICompositorInterop`.
- Visual tree: `ContainerVisual` root → `ShapeVisual` (body shapes, ring,
  shadow host) → `SpriteVisual`s with `CompositionSurfaceBrush` for text/icon surfaces.
- Each text block has its own small drawing surface; redraw only when its string changes.
- Slip content surfaces are created on expand and **released on collapse**;
  orb/tag surfaces persist.
- Device lost (`D2DERR_RECREATE_TARGET`, `DXGI_ERROR_DEVICE_REMOVED`, or
  `CompositionGraphicsDevice.RenderingDeviceReplaced`): recreate device, redraw all live surfaces.

### 10.4 Dependencies (keep it short)

`windows` (feature-gated to what's used), `serde`, `serde_json`, `toml`,
`lexopt`. No tokio, no GUI framework, no image crates (WIC does decoding).

Release profile:
```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```

### 10.5 Errors & logging

- Module failures are isolated: a failing source logs and disables itself
  (with a one-time tag), never takes down the island.
- Log: 256 KB ring file at `%LOCALAPPDATA%\Ngambang\ngambang.log`, warnings and
  errors only; `--verbose` for debug. Never logs clipboard, media titles or command lines.
- Panics (with `panic = "abort"`) are preceded by a panic hook that writes the
  message to the log and restores any pending power plan.

---

## 11. Privacy & security

- No network traffic except ICMP echo to the configured ping host (gamer, while visible). No telemetry.
- Clipboard content is held in memory only while displayed.
- Pipe restricted to the current user, local only.
- Ports **Stop** is limited to processes the user already has rights to terminate, with confirmation.
- `state.json` holds dock position, per-game minutes and pending power-plan restore — nothing else.

---

## 12. Testing

- **core** (any OS, CI on Linux + Windows): state transitions, priority queue,
  layout for 4 edges × DPI 100/150/200 %, clipboard detectors (table-driven,
  incl. adversarial input), protocol parsing and limits, game classification, config validation.
- **Windows smoke (CI, `windows-latest`):** build both arches; `ngambang --self-test`
  creates window + compositor + D2D, draws the orb, round-trips one pipe
  message, prints startup time, exits 0.
- **Manual matrix per release:** Win11 24H2 and 23H2, Win10 22H2; single and
  mixed-DPI multi-monitor; taskbar bottom/top (Win10 left/right); borderless vs
  exclusive fullscreen game; browser fullscreen video; NVIDIA / AMD / Intel GPU;
  Windows Terminal, PowerShell 7, Git Bash, WSL; high contrast; animations off.
- **Perf gate:** script that launches, idles 10 min, opens/closes slips 50×, then
  records private working set and CPU against §2.1. Fails the release if over budget.

---

## 13. Milestones

Status as of v0.1.0: code for M0–M4 is written and type-checked/linted for
Windows; logic is unit-tested on every OS; CI builds both architectures and
runs `--self-test`. Behaviour on real hardware still needs the manual matrix
in §12.

| M | Scope | Done when | v0.1.0 |
|---|-------|-----------|--------|
| M0 | Workspace, window, compositor, D2D, orb on the edge, tray, config | `--self-test` passes in CI | done |
| M1 | All states, morph transitions, hover/click/keyboard/hotkey, 4-edge docking + drag, multi-monitor, DPI, reduced motion | §4 behaviours verified manually | built, needs manual check |
| M2 | Media: SMTC sources, slip, mixer, orb gestures, tide ring | §5.3 passes | built, needs manual check |
| M3 | Commands: pipe, CLI, hooks; ports; dev clipboard; focus timer | §6.7 passes | built, needs manual check |
| M4 | Game: detection, Seam/peek, game slip, captures, break, power plan | §7.7 passes | built, needs manual check |
| M5 | Privacy beacon, error isolation, logging, perf gate, signing, zip + winget | §2.1 budgets met | beacon, logging, zip done; perf gate, signing, winget open |

## 14. Roadmap after v0.1

- v0.2: FPS via PresentMon (opt-in, elevated helper), AMD temps (ADLX), Harbor, colour picker, Docker slip, UI Automation provider, output-device switch (flagged), light theme.
- v0.3: MSIX/sparse package → chat & notification stack (VIP mode, hide message text while screen-sharing), meeting countdown, CI status.
- Later: Waktu Sholat, social screen-time limits, community slip API over the same pipe protocol.
