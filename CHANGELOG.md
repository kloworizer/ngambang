# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.1] - 2026-10-24

### Added
- **Weather & early warnings** module: current weather and the next hours
  from Open-Meteo, early warnings from GDACS (earthquakes, tsunamis,
  cyclones, floods, volcanoes, droughts, wildfires) and USGS earthquake
  reports. Nearby disasters raise a tag; tsunami and Red alerts come through
  even during games.
- **Jadwal sholat** module: prayer times calculated offline (Kemenag RI by
  default, plus MWL, ISNA, Egypt, Umm al-Qura, JAKIM, Karachi), reminders
  before each prayer and a tag at the time.
- Shared **location** setting (Jakarta by default) with a list of cities,
  custom coordinates and **Use my location**.
- **Automatic updates**: a daily check against GitHub Releases, then a
  one-click install that verifies the checksum and restarts.
- Works **offline**: no requests or errors without a connection; cached
  weather is shown and refreshed when the connection returns.
- `ngambang doctor` for troubleshooting.
- Favicon for the project page.

### Fixed
- Clicking elsewhere, or opening from the tray, now always returns the
  island to the orb.
- Dragging the orb to another edge works (it used to need the orb to be
  pressed before the hover label appeared).
- Only real games start a game session: Snipping Tool and other screen
  overlays are ignored, and a fullscreen app needs to be in a game library
  folder, in exclusive fullscreen, or using the GPU heavily.
- The break reminder can be snoozed from its tag, and snoozing now shows a
  confirmation and the time until the next reminder.
- Mic/camera use recorded machine-wide is detected too.
- Saving settings no longer duplicated a config table whose header had a
  comment.

## [0.1.0] - 2026-10-24

First release.

### Added
- The island: Seam, Orb, Tag and Slips states, docked to any screen edge
  (drag the orb to move it), with compositor-driven transitions and the
  "tide" progress ring.
- Modules you can switch on or off one by one (tray menu or `config.toml`):
  media player, volume mixer, command tags, local ports, dev clipboard,
  focus timer, game stats & sessions, screenshots & clips, mic/camera beacon.
- Media control for every app that publishes Windows media controls, and
  per-app volume/mute for every app that plays sound.
- `ngambang` CLI: `notify`, `done`, `clear`, `run`, `hook`, `self-test`.
- Shell hooks for PowerShell, bash and zsh that report long commands.
- Game detection: the island sinks to a 3 px line while a fullscreen app has
  focus; session timer, break reminder, GPU load, VRAM, NVIDIA temperature,
  ping and an optional power-plan switch.
- Settings window (tray or orb right-click → Settings…): start with Windows,
  fullscreen behaviour, motion, capture hiding, hotkey, edge, monitor,
  position, modules, media, clipboard, command, timer and game options,
  including a power-plan picker.
- Config hot-reload, tray menu, keyboard navigation.
- CI on Linux and Windows (x64, ARM64), tagged releases, GitHub Pages site.
- Licensed under the MIT License with the Commons Clause; third-party
  licenses listed in THIRD_PARTY_NOTICES.md.
