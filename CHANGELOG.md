# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

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
- Config hot-reload, tray menu, start with Windows, keyboard navigation.
- CI on Linux and Windows (x64, ARM64), tagged releases, GitHub Pages site.
