# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.9] - 2026-10-26

### Added
- **Ramadan mode.** In Ramadan the Sholat slip, the notch and the glance
  count down to imsak before dawn and to buka (Maghrib) during the day, and
  imsak gets a reminder too. The Sholat slip shows the Hijri date and the
  qibla direction with a small needle. `prayer.hijri_adjust` (Settings ▸
  Location & prayer) moves the Hijri date by a day or two to match the
  announced one.
- **Focus rounds with breaks.** A break follows each round (5 min, 15 after
  every fourth), then the island offers the next round; skip a break from
  the slip. During rounds Windows 11's Do not disturb is turned on. Lengths
  in Settings ▸ Focus & commands.
- **Clipboard history.** The last 20 copied texts (pinned ones are kept and
  survive restarts): click one to paste it into the app you are in, search
  with Find, pin or remove. Text that password managers mark private is
  never recorded.
- **Quick notes**, a new module: write a note in the slip or from a terminal
  with `ngambang note "…"`; copy or delete it later.
- **Battery, network & power**, a new module: battery level and time left,
  warnings at 20 % (your level) and 10 %, online/offline and VPN connected /
  disconnected tags, and the power plans to switch with one click; pick
  plans for battery and for plugged in (Settings ▸ Games & power) and they
  switch by themselves.
- **Download progress.** A browser download in your Downloads folder shows
  in Captures (and the glance) while it grows, then a "Downloaded" tag with
  Open.
- **Price levels.** Set "above" and "below" for any watchlist symbol
  (Settings ▸ Markets); a tag pops up once each time the price crosses one.
- **Meeting mode.** Meeting links are found in Google Calendar's conference
  field and in descriptions (Meet, Zoom, Teams, Webex). During a meeting the
  glance has Join and a microphone mute/unmute button; reminders and the
  calendar rows join with one click.
- **Quick launch**, a new module: drop apps, folders or files on the island
  to pin them; open them from the slip with their own icons; Edit removes.

### Fixed
- Docked left or right, the Markets slip shows three headlines (it had room
  for only one).

### Changed
- **The glance shows everything worth knowing.** Hovering the orb (or the
  Seam) no longer shows one thing: it lists what matters now (mic or camera
  in use, the game, the focus round, a running build, the song with a
  play/pause button, the next event, the next prayer, a fresh warning, the
  weather, the next task and the watchlist's biggest move). Docked top or
  bottom it widens to the left and right; docked left or right it grows up
  and down. Click an item to open its slip.
- **Focus, Weather and Sholat without boxes.** The focus clock stands on its
  own between round − and + buttons; the weather is a large icon and
  temperature with readings and the next hours as plain columns; prayer
  times sit along a line that fills up as the day goes, the next one lit.
- Slips docked left or right can be shorter (from 200 px) so short modules
  carry less empty space.

## [0.1.8] - 2026-10-26

### Added
- **The notch turns through glances.** In the notch style, when nothing is
  playing the notch shows the weather, your watchlist, the next to-do, the
  next event and the next prayer time in turn, a few seconds each. While
  music plays it shows the song instead.
- **Headlines from Google News** as well as Yahoo Finance, so shares Yahoo
  has no news for (BBCA, BBRI…) get some too.

### Changed
- **Compact strips docked top or bottom.** Slips docked to the top or bottom
  edge (and under the notch) are at most 180 px tall, tabs included: the
  tabs sit as small icons in the header row and each module lays out in
  columns side by side.
- **Smaller everywhere.** Labels, details and buttons are smaller and slips
  docked left or right are narrower (400 px) and shorter; the headline of
  each module (the time, the temperature, the next prayer, the price) stays
  large.
- **Fresher headlines.** Checked every 5 minutes (every 2 while the Markets
  slip is open, and right away when you open it), newest first.

### Performance
- Less memory at rest: after 10 s idle the island also clears Direct2D's
  caches, lets the graphics driver release scratch memory and compacts its
  heap before trimming; Windows is told it may page the island out first.

## [0.1.7] - 2026-10-26

### Added
- **Move to another display during fullscreen.** With two or more displays,
  a fullscreen video or game sends the island to another display, where it
  stays a full orb and keeps showing tags; it goes back when the app leaves
  fullscreen or is minimized. On by default (`dock.move_in_fullscreen`, or
  Settings ▸ General).
- **Notch style.** Choose *Notch at the top* in Settings ▸ General ▸ Look or
  the tray's Dock edge menu: the island hangs from the top of the screen
  like a notch, widens for tags, drops its slips down and slides left or
  right when you drag it (`dock.style = "notch"`).
- **Market alerts and headlines.** The watchlist is checked every few
  minutes in the background: a tag pops up when a symbol moves 3 % from the
  previous close (and at every further step; change the step or turn it off
  in Settings ▸ Markets), and when a fresh headline about it appears, with a
  Read button. The Markets slip lists the latest headlines.
- **Calendar reminders from your events.** The reminders you set on each
  event in Google Calendar, Outlook or iCloud (VALARM) now pop up too,
  including ones on all-day events, alongside the reminder from Settings.

### Changed
- **A fuller music slip.** A larger cover and bigger player buttons, with
  no empty space: docked left or right, the cover and titles sit on top
  and the progress and controls stretch across the slip; docked top or
  bottom, the cover fills the slip's height.

### Fixed
- **Calendar and prayer reminders did not show** while a fullscreen app
  (or a maximized window with an auto-hiding taskbar) had focus or a focus
  round ran; they now come through like the break reminder. A reminder for
  an event that starts within the reminder time, or one whose timer woke up
  late after sleep, is no longer skipped.
- The island re-checks Windows' clock every minute, so a time sync, a time
  zone change or modern standby no longer shifts reminders.
- An alarm's own title in a calendar feed could replace the event's title.
- A maximized window no longer counts as fullscreen when the taskbar
  hides itself.

## [0.1.6] - 2026-10-25

### Changed
- **A fresh look.** Every slip leads with its most important thing, large: the
  next prayer, the session clock, the focus time, today's date, the
  temperature. In two columns both sides now carry about the same weight.
- **Icons first, words short.** Buttons carry icons ("▶ 25 min", "+ Add task",
  "✓ Clear done"), weather readings are an icon and a value, and labels are
  trimmed.
- **New icons**, drawn duotone (a soft fill under a crisp line), plus drop,
  wind and thermometer for the weather.
- **Gradients**: the island's surface has a soft sheen, and buttons, the
  active tab, meters, sliders and hero cards glow in their accent.
- **Motion**: slips slide out of the docked edge like a drawer and back in;
  flipping with the wheel slides the next slip in from its side; buttons dim
  while pressed and ripple when clicked.

### Performance
- Text is laid out and measured once and reused, so redrawing an open slip
  every second costs far less.
- The mixer's level meters update only when a level visibly changes, at most
  20 times a second.
- Cached text and pictures are let go when the island rests.

### Fixed
- The site's footer showed version 0.1.0; releases now update it.

## [0.1.5] - 2026-10-25

### Changed
- **Slips fit their content.** Each slip is as tall as what it shows, inside
  a narrow band, so short modules no longer carry empty space and flipping
  between modules only changes the size a little.
- **Docked top or bottom, tall modules go wide.** Short modules keep one
  column; taller ones (weather, prayer times, calendar, games…) spread into
  two columns side by side instead of hanging far down the screen. Docked
  left or right, slips stay one column.

### Fixed
- Flipping to a smaller slip with the wheel no longer closes the island when
  the pointer ends up just outside it: the area of the slip you were on keeps
  counting as the island until you move back in or away.

## [0.1.4] - 2026-10-25

### Added
- **Indonesian public holidays** are linked by default in the Calendar
  module (Google's holiday calendar). Existing setups get it once; removing it
  in Settings keeps it removed.
- **Edit linked calendars**: pick one in Settings ▸ Calendar, change its name,
  address or colour and press Save changes.

### Changed
- **Every slip has the same size** (a little wider, one fixed height), so
  flipping from a tall module to a short one no longer shrinks the island out
  from under the pointer. Lists show as many rows as fit; the tabs sit in one
  row.
- Update checks live in one place: right-click the orb or the tray icon ▸
  Check for updates (it becomes Install update when one is found). Settings
  keeps only the daily automatic check.

### Fixed
- **Borderless "windowed fullscreen" games** now sink the island to the Seam:
  the foreground window is looked at again every 1.5 s (such games resize
  after taking focus), and a fullscreen app that did not look like a game is
  measured again every 10 s, as games often load with a quiet GPU.
- **Game sessions end when the game closes**: the island watches the game's
  process instead of always waiting out the one-minute alt-tab grace period.

## [0.1.3] - 2026-10-25

### Added
- **Calendar** module: link Google Calendar (its secret iCal address) or any
  iCal calendar, as many as you like, each with its own colour. The slip shows
  this week and what is coming up; a reminder pops up before each event (with
  a Join button for online meetings). Recurring events, exceptions and time
  zones are handled; the last events are kept for offline use.
- **Markets** module: a watchlist of shares, indices, crypto and currencies
  (Yahoo Finance), with price, change and a sparkline for today. In Settings,
  type a name or symbol and pick from the matches. Prices load only while the
  slip is open, then every minute.
- **Module order**: Settings ▸ Modules lists every module with a tick to show
  it and Move up / Move down to order the tabs.
- **Volume level** everywhere: scrolling shows the level in a glance on the
  orb or tag and as a pill on an open slip; every mixer row shows its percent.

### Changed
- **Neumorphic design**: soft raised and pressed surfaces across the island
  (buttons, tabs, tiles, sliders, the orb) and soft cards in Settings.
- The slip's tabs always run in a rail along its bottom, on every edge, and
  are part of the slip, so they never hang past a short slip.
- Scrolling on the orb or a tag changes the volume of the player that is
  playing (its own app volume); the system volume when there is no player.
- "Add task" is a field in the to-do slip: type, Enter adds, Esc cancels.
- Weather slip redesigned: an icon-only refresh button, the temperature with
  feels-like, humidity and wind, the next hours as cards, warnings as a list.

### Fixed
- **Camera and microphone use** is now read from the devices: Media
  Foundation's sensor monitor reports every app streaming from a camera, and
  active audio capture sessions show every app recording. The privacy
  registry is the fallback and is re-read every few seconds. `ngambang doctor`
  shows all three.

## [0.1.2] - 2026-10-25

### Added
- **To-do list** module (on by default): add tasks from the slip, tick them
  off, clear finished ones; kept between restarts.
- **Custom focus round**: − and + in the timer slip set your own length
  (also in Settings), next to the preset buttons.
- **Album art and artist** in the media slip and the now-playing tag, which
  also has previous / play-pause / next buttons.
- **Pop-ups** with a spring animation for a new song, a change in the
  weather, early warnings and prayer reminders; a crossfade when a tag's
  content changes; hover and press feedback on the orb.
- **Settings** redesigned: a sidebar of pages with icons, page headers and
  modern Windows controls.
- **App icon** (the orb from the website) on both executables, the tray, the
  taskbar and Explorer, plus version details shown in Task Manager.
- About 240 places in the location list: more Indonesian cities, Malaysia,
  Singapore, Brunei, the Middle East and large cities worldwide.

### Changed
- Docked to the top or bottom, the slip's tabs run along its bottom edge.
- The weather loads as soon as the island starts and refreshes every 30
  minutes; a failed fetch retries after 20 s, 1, 2, 5 and then 10 minutes, and
  opening the weather slip refreshes data older than half an hour.

### Fixed
- Dragging the island now follows the pointer in real time, and works from a
  tag or an open slip (drag its background), not only from the orb.
- No more busy loop while Windows reports no internet connection: the network
  worker waits a minute between checks.
- Lower memory: idle working set is returned to Windows 30 s after the island
  settles; track info and album art are read only when a player changes them.

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
