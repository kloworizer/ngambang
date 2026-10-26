//! Slips docked top or bottom: wide, low strips (at most [`STRIP_MAX_H`]
//! tall, tabs included). Each module lays out in columns side by side; the
//! one thing that matters most stays large, everything else is small.

use super::*;

/// The tallest a strip may be, its tabs included.
pub const STRIP_MAX_H: f32 = 180.0;
const PAD: f32 = 12.0;
/// The header row: the module's name on the left, the tabs on the right.
const HEAD_H: f32 = 22.0;
/// Where content starts and how tall it may be.
const TOP: f32 = PAD + HEAD_H + 8.0;
const ROOM: f32 = STRIP_MAX_H - TOP - PAD;
const COL_GAP: f32 = 16.0;

const S_BODY: TextStyle = TextStyle { size: 12.5, weight: Weight::Regular, mono: false };
const S_BODY_M: TextStyle = TextStyle { size: 12.5, weight: Weight::Medium, mono: false };
const S_SMALL: TextStyle = TextStyle { size: 11.0, weight: Weight::Regular, mono: false };
const S_MONO: TextStyle = TextStyle { size: 11.0, weight: Weight::Regular, mono: true };
const S_TINY: TextStyle = TextStyle { size: 9.5, weight: Weight::Medium, mono: true };
const S_TITLE: TextStyle = TextStyle { size: 15.0, weight: Weight::Semibold, mono: false };

/// Column positions: the content width split by `weights`, with gaps.
fn columns(x0: f32, w: f32, weights: &[f32]) -> Vec<(f32, f32)> {
    let total: f32 = weights.iter().sum();
    let room = w - COL_GAP * (weights.len() as f32 - 1.0);
    let mut x = x0;
    weights
        .iter()
        .map(|k| {
            let cw = (room * k / total).floor();
            let c = (x, cw);
            x += cw + COL_GAP;
            c
        })
        .collect()
}

impl B<'_> {
    fn at(&mut self, (x, w): (f32, f32)) {
        self.x = x;
        self.w = w;
    }

    /// A small pill button: an icon and a short label.
    fn pill(&mut self, r: Rect, icon: Option<Icon>, label: &str, kind: Btn, action: Action) {
        let (fg, accent) = match kind {
            Btn::Primary(c) => (SURFACE, Some(c)),
            Btn::Danger => (SURFACE, Some(PRIVACY)),
            _ => (TEXT, None),
        };
        let radius = (r.h / 2.0).min(10.0);
        match accent {
            Some(c) => self.prims.extend(soft::raised_grad(r, radius, accent_light(c), accent_deep(c), 2.0)),
            None => self.prims.extend(soft::raised_grad(r, radius, SURFACE_TOP, SURFACE, 2.0)),
        }
        let style = TextStyle { size: 12.0, weight: if accent.is_some() { Weight::Semibold } else { Weight::Medium }, mono: false };
        let tw = if label.is_empty() { 0.0 } else { self.tm.width(label, style).ceil().min(r.w - 12.0) };
        let iw = if icon.is_some() { 14.0 } else { 0.0 };
        let gap = if icon.is_some() && !label.is_empty() { 6.0 } else { 0.0 };
        let x = r.x + (r.w - iw - gap - tw) / 2.0;
        if let Some(i) = icon {
            self.icon(i, Rect::new(x, r.y + (r.h - 14.0) / 2.0, 14.0, 14.0), fg);
        }
        if !label.is_empty() {
            self.prims.push(Prim::Text {
                rect: Rect::new(x + iw + gap, r.y, tw + 4.0, r.h),
                text: label.into(),
                style,
                color: fg,
                align: Align::Start,
                wrap: false,
            });
        }
        self.hits.push(Hit { rect: r, kind: HitKind::Click(action), label: label.into() });
    }

    /// A small square icon button without a face.
    fn mini_icon(&mut self, r: Rect, icon: Icon, fg: Color, action: Option<Action>, label: &str) {
        self.prims.extend(soft::raised(r, 8.0, SURFACE, 1.5));
        let fg = if action.is_none() { fg.with_alpha(0x55) } else { fg };
        let s = (r.h * 0.5).round().clamp(12.0, 16.0);
        self.icon(icon, Rect::new(r.x + (r.w - s) / 2.0, r.y + (r.h - s) / 2.0, s, s), fg);
        if let Some(a) = action {
            self.hits.push(Hit { rect: r, kind: HitKind::Click(a), label: label.into() });
        }
    }

    /// Small chips that wrap within the current column.
    fn mini_chips(&mut self, y: f32, chips: Vec<(String, Color, Action)>) -> f32 {
        let (mut cx, mut cy) = (self.x, y);
        for (label, color, action) in chips {
            let w = self.tm.width(&label, S_SMALL).ceil() + 20.0;
            if cx + w > self.x + self.w && cx > self.x {
                cx = self.x;
                cy += 30.0;
            }
            let r = Rect::new(cx, cy, w.min(self.w), 24.0);
            if color == TEXT_2 {
                self.prims.extend(soft::raised(r, 12.0, SURFACE, 1.5));
            } else {
                self.prims.extend(soft::well(r, 12.0));
            }
            self.prims.push(Prim::Text { rect: r, text: label.clone(), style: S_SMALL, color, align: Align::Center, wrap: false });
            self.hits.push(Hit { rect: r, kind: HitKind::Click(action), label });
            cx += w + 6.0;
        }
        cy + 24.0
    }

    /// A small hero card for the headline.
    fn hero_s(&mut self, r: Rect, accent: Color) {
        self.prims.extend(soft::raised_grad(r, 12.0, tint(accent, 0.20), tint(accent, 0.05), 2.0));
    }

    fn line(&mut self, r: Rect, t: impl Into<String>, style: TextStyle, color: Color) {
        self.text(r, t, style, color);
    }
}

/// The module's name and a short detail for the header row.
fn title(m: &Model, front: ModuleId) -> (String, Option<(String, Color)>) {
    let detail = |s: String| Some((s, TEXT_2));
    match front {
        ModuleId::Media => match m.front_media() {
            Some(s) if m.media.len() > 1 => ("Media".into(), detail(format!("{} · {} sources", s.app, m.media.len()))),
            Some(s) => ("Media".into(), detail(s.app.clone())),
            None => ("Media".into(), None),
        },
        ModuleId::Mixer => {
            let device = if m.mixer.device.is_empty() { "Default output".to_string() } else { format::ellipsize(&m.mixer.device, 30) };
            ("Mixer".into(), detail(device))
        }
        ModuleId::Commands => ("Commands".into(), detail(m.jobs.len().to_string())),
        ModuleId::Ports => ("Listening · local".into(), detail(m.ports.len().to_string())),
        ModuleId::Clipboard => match &m.clip {
            Some(c) => (
                format!("Clipboard · {}", c.kind.label()),
                clipboard::status(&c.kind, c.unix_at).map(|s| (s.clone(), if s.starts_with("expired") { PRIVACY_TEXT } else { TEXT_2 })),
            ),
            None => ("Clipboard".into(), None),
        },
        ModuleId::Timer => (focus_title(m), focus_rounds(m)),
        ModuleId::Game => {
            let now = m.now_ms;
            let today = format!(
                "today {}",
                format::hours_minutes(m.playlog.total() + m.game.as_ref().map(|g| now.saturating_sub(g.started_ms) / 60_000).unwrap_or(0))
            );
            match &m.game {
                Some(g) => (format!("Game · {}", g.exe), detail(today)),
                None => ("Games".into(), detail(today)),
            }
        }
        ModuleId::Captures => ("Captures".into(), None),
        ModuleId::Privacy => ("Mic & camera".into(), None),
        ModuleId::Prayer => {
            (format!("Sholat · {}", format::ellipsize(&m.config.location.city, 20)), detail(m.config.prayer.method.label().to_string()))
        }
        ModuleId::Weather => (format!("Weather · {}", format::ellipsize(&m.config.location.city, 20)), None),
        ModuleId::Todo => {
            let open = m.todos.iter().filter(|t| !t.done).count();
            ("To-do".into(), (!m.todos.is_empty()).then(|| (format!("{open} left"), if open > 0 { OK } else { TEXT_2 })))
        }
        ModuleId::Calendar => ("Calendar".into(), None),
        ModuleId::Finance => {
            ("Markets".into(), detail(format!("{} symbols · Yahoo Finance, may be delayed", m.config.finance.watchlist.len())))
        }
        ModuleId::Notes => ("Notes".into(), (!m.notes.is_empty()).then(|| (m.notes.len().to_string(), TEXT_2))),
        ModuleId::Power => ("Battery & network".into(), None),
        ModuleId::Launch => ("Quick launch".into(), detail("drop files here to pin".into())),
    }
}

pub(super) fn strip(m: &Model, front: ModuleId, tm: &dyn TextMeasure) -> Scene {
    let tabs = m.available_slips();
    let w = SLIP_W2;
    let mut b = B {
        prims: Vec::new(),
        hits: Vec::new(),
        tm,
        x: PAD,
        w: w - 2.0 * PAD,
        limit: TOP + ROOM,
        floor: TOP + ROOM,
        two: false,
        x0: PAD,
        full_w: w - 2.0 * PAD,
        fill: TOP + ROOM,
        vertical: false,
    };
    let bottom = match front {
        ModuleId::Media => media(m, &mut b),
        ModuleId::Mixer => mixer(m, &mut b),
        ModuleId::Commands => commands(m, &mut b),
        ModuleId::Ports => ports(m, &mut b),
        ModuleId::Clipboard => clip(m, &mut b),
        ModuleId::Timer => timer(m, &mut b),
        ModuleId::Game => game(m, &mut b),
        ModuleId::Captures => captures(m, &mut b),
        ModuleId::Privacy => privacy(m, &mut b),
        ModuleId::Prayer => prayer(m, &mut b),
        ModuleId::Weather => weather(m, &mut b),
        ModuleId::Todo => todo(m, &mut b),
        ModuleId::Calendar => calendar(m, &mut b),
        ModuleId::Finance => finance(m, &mut b),
        ModuleId::Notes => notes(m, &mut b),
        ModuleId::Power => power(m, &mut b),
        ModuleId::Launch => launch(m, &mut b),
    };
    let content: Vec<Prim> = std::mem::take(&mut b.prims);
    // As tall as the content, never more than the strip may be.
    let h = (bottom.min(TOP + ROOM) + PAD).max(TOP + 44.0 + PAD).ceil();
    let size = Size::new(w, h);
    b.prims.extend(surface(Rect::new(0.0, 0.0, w, h), 16.0));

    // Tabs: small icons on the right of the header row.
    let (tab_w, gap) = (24.0, 2.0);
    let n = tabs.len() as f32;
    let tabs_w = n * tab_w + (n - 1.0).max(0.0) * gap;
    let mut tx = w - PAD - tabs_w;
    for t in &tabs {
        let r = Rect::new(tx, PAD - 2.0, tab_w, HEAD_H + 2.0);
        let active = *t == front;
        let (icon, color) = module_icon(*t);
        if active {
            b.prims.extend(soft::well(r, 8.0));
            b.prims.push(Prim::Gradient {
                rect: r.inset(2.0),
                radius: 6.0,
                from: color.with_alpha(0x40),
                to: color.with_alpha(0x0C),
                diagonal: false,
            });
        }
        b.icon(
            icon,
            Rect::new(r.x + (r.w - 14.0) / 2.0, r.y + (r.h - 14.0) / 2.0, 14.0, 14.0),
            if active { color } else { color.with_alpha(0x90) },
        );
        b.hits.push(Hit { rect: r, kind: HitKind::Click(Action::SelectSlip(*t)), label: t.label().into() });
        tx += tab_w + gap;
    }

    // The header: the module's name and its detail, left of the tabs.
    let (accent, label_room) = (module_icon(front).1, w - 2.0 * PAD - tabs_w - 12.0);
    let (name, detail) = title(m, front);
    let name = name.to_uppercase();
    b.prims.push(Prim::Circle { cx: PAD + 4.0, cy: PAD + HEAD_H / 2.0, r: 3.5, fill: Some(accent), stroke: None });
    let name_w = tm.width(&name, TextStyle::LABEL).ceil().min(label_room - 16.0);
    b.text(Rect::new(PAD + 14.0, PAD, name_w, HEAD_H), name, TextStyle::LABEL, LABEL);
    if let Some((d, c)) = detail {
        let dx = PAD + 14.0 + name_w + 10.0;
        b.text(Rect::new(dx, PAD, (label_room - (dx - PAD)).max(0.0), HEAD_H), d, S_SMALL, c);
    }
    b.prims.extend(content);

    if m.volume_flashing() {
        let (who, v) = m.flashed_volume();
        let level = if v.muted { "muted".to_string() } else { format!("{:.0}%", v.level * 100.0) };
        let text = format!("{} {level}", format::ellipsize(&who, 12));
        let pill = Rect::new(w / 2.0 - 78.0, h - 40.0, 156.0, 30.0);
        b.prims.extend(soft::raised(pill, 15.0, SURFACE_3, 2.0));
        b.icon(if v.muted { Icon::SpeakerMuted } else { Icon::Speaker }, Rect::new(pill.x + 14.0, pill.y + 8.0, 14.0, 14.0), MEDIA);
        b.prims.push(Prim::Text {
            rect: Rect::new(pill.x + 34.0, pill.y, pill.w - 46.0, pill.h),
            text,
            style: TextStyle { size: 13.0, weight: Weight::Semibold, mono: true },
            color: TEXT,
            align: Align::End,
            wrap: false,
        });
    }
    Scene { kind: SceneKind::Slip(front), size, flush: false, clip_radius: 16.0, prims: b.prims, hits: b.hits, ring: None }
}

fn empty(b: &mut B, text: &str) -> f32 {
    b.text(Rect::new(b.x, TOP, b.w, 22.0), text, S_BODY, TEXT_2);
    TOP + 22.0
}

/// Rows of `row_h` flowing down `cols` columns; None once they are full.
fn flow(cols: &[(f32, f32)], i: usize, row_h: f32, rows: usize) -> Option<((f32, f32), f32)> {
    let rows = rows.max(1);
    let c = cols.get(i / rows)?;
    Some((*c, TOP + (i % rows) as f32 * row_h))
}

fn fit_rows(row_h: f32, gap: f32) -> usize {
    ((ROOM + gap) / row_h).floor().max(1.0) as usize
}

// ---- modules ------------------------------------------------------------

fn media(m: &Model, b: &mut B) -> f32 {
    let Some(s) = m.front_media() else { return empty(b, "Nothing is playing") };
    let now = m.now_ms;
    let side = 112.0;
    media_art(b, s, Rect::new(PAD, TOP, side, side));
    let x = PAD + side + COL_GAP;
    let w = b.full_w - side - COL_GAP;
    // Controls on the right of the title.
    let (btn, play, gap) = (38.0, 56.0, 8.0);
    let controls_w = 2.0 * btn + play + 2.0 * gap;
    let cx = x + w - controls_w;
    let prev = s.can_prev.then_some(Action::MediaPrev);
    let next = s.can_next.then_some(Action::MediaNext);
    b.icon_button(Rect::new(cx, TOP + 4.0, btn, btn), Icon::Prev, TEXT, Some(SURFACE_3), prev, "Previous track");
    let (icon, label) = if s.playing { (Icon::Pause, "Pause") } else { (Icon::Play, "Play") };
    let pc = if s.can_play_pause { MEDIA } else { TRACK };
    let r = Rect::new(cx + btn + gap, TOP + 4.0, play, btn);
    b.prims.extend(soft::raised_grad(r, btn / 2.0, accent_light(pc), accent_deep(pc), 2.0));
    b.icon(icon, Rect::new(r.x + play / 2.0 - 9.0, r.y + btn / 2.0 - 9.0, 18.0, 18.0), SURFACE);
    if s.can_play_pause {
        b.hits.push(Hit { rect: r, kind: HitKind::Click(Action::MediaPlayPause), label: label.into() });
    }
    b.icon_button(Rect::new(r.right() + gap, TOP + 4.0, btn, btn), Icon::Next, TEXT, Some(SURFACE_3), next, "Next track");
    // Title and artist.
    let tw = w - controls_w - 12.0;
    let title = if s.title.is_empty() { s.app.clone() } else { s.title.clone() };
    b.line(Rect::new(x, TOP, tw, 24.0), title, TextStyle { size: 17.0, weight: Weight::Semibold, mono: false }, TEXT);
    let artist = if s.artist.is_empty() { s.app.clone() } else { s.artist.clone() };
    b.line(Rect::new(x, TOP + 24.0, tw, 18.0), artist, S_BODY, TEXT_2);
    // Progress across the whole width, times under it.
    b.at((x, w));
    let mut y = TOP + 56.0;
    if s.duration_ms > 0 {
        let frac = s.progress_at(now).unwrap_or(0.0);
        let played = (w * frac).max(0.0);
        let cy = y + 6.0;
        if played > 2.0 {
            let pts = if s.playing { wave_points(x, cy, played, 3.0) } else { vec![Point::new(x, cy), Point::new(x + played, cy)] };
            b.prims.push(Prim::Polyline { points: pts, width: 2.0, color: MEDIA, closed: false });
        }
        b.prims.push(Prim::Circle { cx: x + played + 3.0, cy, r: 3.0, fill: Some(TEXT), stroke: None });
        if played + 8.0 < w {
            b.rect(Rect::new(x + played + 8.0, cy - 1.0, w - played - 8.0, 2.0), 1.0, TRACK);
        }
        if s.can_seek {
            b.hits.push(Hit { rect: Rect::new(x, y - 6.0, w, 24.0), kind: HitKind::Slider(SliderTarget::MediaSeek), label: "Seek".into() });
        }
        b.line(Rect::new(x, y + 14.0, 80.0, 14.0), format::clock(s.position_at(now) / 1000), S_TINY, LABEL);
        b.text_end(Rect::new(x + w - 80.0, y + 14.0, 80.0, 14.0), format::clock(s.duration_ms / 1000), S_TINY, LABEL);
        y += 34.0;
    }
    if m.media.len() > 1 {
        let chips = m
            .media
            .iter()
            .take(4)
            .enumerate()
            .map(|(i, o)| {
                let name = if o.playing { o.app.clone() } else { format!("{} · paused", o.app) };
                (format::ellipsize(&name, 16), if o.id == s.id { MEDIA } else { TEXT_2 }, Action::MediaSource(i))
            })
            .collect();
        y = b.mini_chips(y.max(TOP + 86.0), chips);
    }
    y.max(TOP + side)
}

fn mixer(m: &Model, b: &mut B) -> f32 {
    let cols = columns(PAD, b.full_w, &[1.0, 1.0]);
    let row_h = 30.0;
    let rows = fit_rows(row_h, 0.0);
    let row = |b: &mut B,
               (x, w): (f32, f32),
               y: f32,
               initial: &str,
               name: &str,
               vol: Volume,
               peak: Option<f32>,
               target: SliderTarget,
               mute: Action,
               accent: Color| {
        let tile = Rect::new(x, y + 3.0, 22.0, 22.0);
        if initial.is_empty() {
            b.icon_tile(tile, Icon::Speaker, accent);
        } else {
            b.prims.push(Prim::Gradient {
                rect: tile,
                radius: 7.0,
                from: accent.with_alpha(0x3A),
                to: accent.with_alpha(0x12),
                diagonal: true,
            });
            b.prims.push(Prim::Text {
                rect: tile,
                text: initial.into(),
                style: TextStyle { size: 11.0, weight: Weight::Semibold, mono: false },
                color: accent,
                align: Align::Center,
                wrap: false,
            });
        }
        let (slider_w, level_w, mute_w) = (104.0, 34.0, 26.0);
        let nx = x + 30.0;
        let name_w = w - 30.0 - slider_w - level_w - mute_w - 16.0;
        b.line(Rect::new(nx, y + 2.0, name_w, 20.0), name, S_BODY, TEXT);
        if let Some(p) = peak {
            b.meter(Rect::new(nx, y + 23.0, name_w, 2.0), p, accent);
        }
        let sx = nx + name_w + 6.0;
        b.slider(
            Rect::new(sx, y + 2.0, slider_w, 24.0),
            vol.level,
            if vol.muted { TRACK } else { accent },
            target,
            &format!("{name} volume"),
        );
        let level = if vol.muted { "muted".to_string() } else { format!("{:.0}%", vol.level * 100.0) };
        b.text_end(Rect::new(sx + slider_w, y, level_w, 28.0), level, S_TINY, TEXT_2);
        let (icon, fg) = if vol.muted { (Icon::SpeakerMuted, PRIVACY_TEXT) } else { (Icon::Speaker, TEXT) };
        b.mini_icon(Rect::new(x + w - mute_w, y + 1.0, mute_w, mute_w), icon, fg, Some(mute), &format!("Mute {name}"));
    };
    row(b, cols[0], TOP, "", "System", m.mixer.master, None, SliderTarget::Master, Action::MasterMute, MEDIA);
    if m.mixer.apps.is_empty() {
        b.at(cols[1]);
        b.text(Rect::new(b.x, TOP, b.w, 28.0), "No app is playing sound", S_BODY, TEXT_2);
        return TOP + row_h;
    }
    let mut bottom = TOP + row_h;
    for (i, app) in m.mixer.apps.iter().enumerate() {
        let Some((col, y)) = flow(&cols, i + 1, row_h, rows) else { break };
        bottom = bottom.max(y + row_h);
        let initial: String = app.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
        let accent = if app.has_controls { MEDIA } else { TEXT };
        row(
            b,
            col,
            y,
            &initial,
            &format::ellipsize(&app.name, 18),
            app.volume,
            Some(app.peak),
            SliderTarget::App(app.pid),
            Action::AppMute(app.pid),
            accent,
        );
    }
    bottom - 2.0
}

fn commands(m: &Model, b: &mut B) -> f32 {
    let now = m.now_ms;
    if m.jobs.is_empty() {
        let y = empty(b, "Nothing reported yet");
        b.line(Rect::new(b.x, y + 4.0, b.w, 16.0), "ngambang run -- <command>", S_MONO, LABEL);
        return y + 20.0;
    }
    let cols = columns(PAD, b.full_w, &[1.0, 1.0]);
    let row_h = 28.0;
    let rows = fit_rows(row_h, 0.0);
    let any_done = m.jobs.iter().any(|j| !matches!(j.status, JobStatus::Running { .. }));
    let slots = rows * 2 - usize::from(any_done);
    let mut bottom = TOP;
    for (i, j) in m.jobs.iter().take(slots).enumerate() {
        let Some(((x, w), y)) = flow(&cols, i, row_h, rows) else { break };
        bottom = bottom.max(y + row_h);
        let (icon, color, detail) = match &j.status {
            JobStatus::Running { progress: Some(p) } => (Icon::Terminal, CODER, format!("{:.0}%", p * 100.0)),
            JobStatus::Running { progress: None } => {
                (Icon::Terminal, CODER, format::duration_short(now.saturating_sub(j.started_ms) / 1000))
            }
            JobStatus::Done { ok: true, duration_s, .. } => {
                (Icon::Check, OK, duration_s.map(|d| format::duration_short(d.round() as u64)).unwrap_or_default())
            }
            JobStatus::Done { ok: false, code, .. } => {
                (Icon::Cross, PRIVACY, code.map(|c| format!("exit {c}")).unwrap_or_else(|| "failed".into()))
            }
            JobStatus::Note { level } => match level {
                crate::protocol::Level::Fail => (Icon::Cross, PRIVACY, String::new()),
                crate::protocol::Level::Ok => (Icon::Check, OK, String::new()),
                _ => (Icon::Terminal, CODER, String::new()),
            },
        };
        b.icon(icon, Rect::new(x, y + 7.0, 14.0, 14.0), color);
        let open = matches!(j.status, JobStatus::Done { ok: false, .. }) && j.hwnd.is_some();
        let right = if open { 104.0 } else { 60.0 };
        b.line(Rect::new(x + 22.0, y, w - 22.0 - right, row_h), j.title.clone(), S_MONO, TEXT);
        let dc = if color == PRIVACY { PRIVACY_TEXT } else { TEXT_2 };
        if open {
            b.text_end(Rect::new(x + w - right, y, 48.0, row_h), detail, S_TINY, dc);
            b.pill(Rect::new(x + w - 50.0, y + 2.0, 50.0, 24.0), None, "Open", Btn::Secondary, Action::JobOpenTerminal(j.id.clone()));
        } else {
            b.text_end(Rect::new(x + w - right, y, right, row_h), detail, S_TINY, dc);
        }
    }
    if any_done {
        let ((x, w), y) = flow(&cols, rows * 2 - 1, row_h, rows).unwrap_or((cols[1], TOP));
        b.pill(Rect::new(x + w - 132.0, y + 2.0, 132.0, 24.0), Some(Icon::Check), "Clear finished", Btn::Secondary, Action::JobsClear);
        bottom = bottom.max(y + row_h);
    }
    bottom - 2.0
}

fn ports(m: &Model, b: &mut B) -> f32 {
    let now = m.now_ms;
    if m.ports.is_empty() {
        return empty(b, "No local servers listening");
    }
    let cols = columns(PAD, b.full_w, &[1.0, 1.0]);
    let row_h = 30.0;
    let rows = fit_rows(row_h, 0.0);
    let mut bottom = TOP;
    for (i, p) in m.ports.iter().enumerate() {
        let Some(((x, w), y)) = flow(&cols, i, row_h, rows) else { break };
        bottom = bottom.max(y + row_h);
        b.line(Rect::new(x, y, 58.0, 28.0), format!(":{}", p.port), TextStyle { size: 13.0, weight: Weight::Medium, mono: true }, PORTS);
        b.line(Rect::new(x + 60.0, y, w - 60.0 - 96.0, 28.0), format!("{} · {}", p.process, p.pid), S_BODY, TEXT);
        b.mini_icon(
            Rect::new(x + w - 92.0, y + 1.0, 26.0, 26.0),
            Icon::Open,
            TEXT,
            Some(Action::PortOpen(p.port)),
            &format!("Open localhost:{}", p.port),
        );
        let sr = Rect::new(x + w - 60.0, y + 1.0, 60.0, 26.0);
        let confirming = m.port_confirm.get(&p.port).is_some_and(|u| *u > now);
        if !p.can_stop {
            b.rect(sr, 8.0, SURFACE_2);
            b.prims.push(Prim::Text { rect: sr, text: "elevated".into(), style: S_TINY, color: LABEL, align: Align::Center, wrap: false });
        } else if confirming {
            b.pill(sr, None, "Confirm", Btn::Danger, Action::PortStop(p.port));
        } else {
            b.pill(sr, Some(Icon::Stop), "Stop", Btn::Secondary, Action::PortStop(p.port));
            if let Some(h) = b.hits.last_mut() {
                h.label = format!("Stop {} on port {}", p.process, p.port);
            }
        }
    }
    bottom - 2.0
}

fn clip(m: &Model, b: &mut B) -> f32 {
    let cols = columns(PAD, b.full_w, &[1.0, 1.3]);
    b.at(cols[1]);
    let history = super::more::clip_history(m, b, TOP);
    b.at(cols[0]);
    let Some(c) = &m.clip else {
        b.text(Rect::new(b.x, TOP, b.w, 36.0), "Copy a JWT, JSON, UUID, timestamp, colour or link", S_SMALL, TEXT_2);
        return history.max(TOP + 36.0);
    };
    detected(b, c, cols[0]).max(history)
}

/// The detected format in a column: a preview and its actions.
fn detected(b: &mut B, c: &ClipState, col: (f32, f32)) -> f32 {
    let cols = [col, col];
    let (x, w) = cols[0];
    let preview = clipboard::preview(&c.text, &c.kind, c.unix_at);
    let per_line = ((w - 20.0) / 6.8).max(8.0) as usize;
    // Room for the actions under it.
    let max_lines = 4;
    let shown: String = format::ellipsize(&preview, 400).lines().take(max_lines).collect::<Vec<_>>().join("\n");
    let lines: usize = shown.lines().map(|l| l.chars().count().div_ceil(per_line).max(1)).sum::<usize>().min(max_lines);
    let box_h = (lines as f32 * 15.0 + 16.0).clamp(36.0, ROOM - 34.0);
    b.rect(Rect::new(x, TOP, w, box_h), 8.0, SURFACE_2);
    let mut tx = x + 10.0;
    if let ClipKind::HexColor { r, g, b: bl, a } = c.kind {
        let col = Color(((r as u32) << 24) | ((g as u32) << 16) | ((bl as u32) << 8) | a as u32);
        b.prims.push(Prim::RoundRect {
            rect: Rect::new(x + 10.0, TOP + 10.0, 22.0, 22.0),
            radius: 6.0,
            fill: Some(col),
            stroke: Some((HAIRLINE, 1.0)),
        });
        tx += 30.0;
    }
    b.prims.push(Prim::Text {
        rect: Rect::new(tx, TOP + 8.0, x + w - 10.0 - tx, box_h - 16.0),
        text: shown,
        style: TextStyle { size: 11.0, weight: Weight::Regular, mono: true },
        color: TEXT,
        align: Align::Start,
        wrap: true,
    });
    b.at(cols[1]);
    let chips = c.kind.actions().iter().map(|a| (a.label().to_string(), CODER, Action::Clip(*a))).collect();
    b.mini_chips(TOP + box_h + 8.0, chips)
}

fn timer(m: &Model, b: &mut B) -> f32 {
    let cols = columns(PAD, b.full_w, &[1.0, 1.3]);
    let (fx, fw) = cols[0];
    // The clock, unboxed, centred in the strip's height.
    let (end, buttons) = timer_face(m, b, fx, TOP + 14.0, fw);
    let (bx, bw) = cols[1];
    let per_row = 2;
    let rows = buttons.len().div_ceil(per_row);
    let gap = 10.0;
    let bh = 36.0;
    let top = TOP + 14.0 + (68.0 - (rows as f32 * bh + (rows as f32 - 1.0) * gap)) / 2.0;
    let cw = (bw - gap) / 2.0;
    for (i, (icon, label, kind, action)) in buttons.into_iter().enumerate() {
        let (r, c) = (i / per_row, i % per_row);
        b.pill(Rect::new(bx + c as f32 * (cw + gap), top + r as f32 * (bh + gap), cw, bh), Some(icon), &label, kind, action);
    }
    end
}

fn game(m: &Model, b: &mut B) -> f32 {
    let now = m.now_ms;
    let Some(g) = &m.game else {
        let mut games: Vec<(&String, &u64)> = m.playlog.minutes.iter().collect();
        if games.is_empty() {
            return empty(b, "No games played today");
        }
        games.sort_by(|a, b| b.1.cmp(a.1));
        let cols = columns(PAD, b.full_w, &[1.0, 1.0]);
        let rows = fit_rows(28.0, 0.0);
        let mut bottom = TOP;
        for (i, (exe, mins)) in games.into_iter().enumerate() {
            let Some(((x, w), y)) = flow(&cols, i, 28.0, rows) else { break };
            bottom = bottom.max(y + 28.0);
            b.icon(Icon::Game, Rect::new(x, y + 7.0, 14.0, 14.0), GAME);
            b.line(Rect::new(x + 22.0, y, w - 22.0 - 70.0, 28.0), exe.clone(), S_BODY_M, TEXT);
            b.text_end(Rect::new(x + w - 70.0, y, 70.0, 28.0), format::hours_minutes(*mins), S_MONO, TEXT_2);
        }
        return bottom - 2.0;
    };
    let cols = columns(PAD, b.full_w, &[1.0, 1.1, 1.0]);
    let hero_h = 104.0;
    let (hx, hw) = cols[0];
    let card = Rect::new(hx, TOP, hw, hero_h);
    b.hero_s(card, GAME);
    b.icon_tile(Rect::new(card.x + 12.0, card.y + 12.0, 22.0, 22.0), Icon::Game, GAME);
    b.line(Rect::new(card.x + 42.0, card.y + 12.0, card.w - 50.0, 22.0), "this session", S_SMALL, TEXT_2);
    b.line(
        Rect::new(card.x + 14.0, card.bottom() - 50.0, card.w - 28.0, 40.0),
        format::clock(now.saturating_sub(g.started_ms) / 1000),
        TextStyle { size: 32.0, weight: Weight::Semibold, mono: true },
        TEXT,
    );
    let tiles = [
        (Icon::Chart, "GPU", m.stats.gpu_pct.map(|v| format!("{v:.0}%"))),
        (Icon::Grid, "VRAM", m.stats.vram_mb.map(|v| if v >= 1024 { format!("{:.1}G", v as f32 / 1024.0) } else { format!("{v}M") })),
        (Icon::Thermo, "TEMP", m.stats.temp_c.map(|v| format!("{v}°"))),
        (Icon::Plug, "PING", m.stats.ping_ms.map(|v| format!("{v}ms"))),
    ];
    let (tx, tw) = cols[1];
    let gap = 8.0;
    let (cw, ch) = ((tw - gap) / 2.0, (hero_h - gap) / 2.0);
    for (i, (icon, k, v)) in tiles.iter().enumerate() {
        let r = Rect::new(tx + (i % 2) as f32 * (cw + gap), TOP + (i / 2) as f32 * (ch + gap), cw, ch);
        b.rect(r, 10.0, SURFACE_2);
        b.icon(*icon, Rect::new(r.x + 8.0, r.y + 7.0, 12.0, 12.0), GAME);
        b.line(Rect::new(r.x + 24.0, r.y + 5.0, r.w - 28.0, 14.0), *k, S_TINY, LABEL);
        b.line(
            Rect::new(r.x + 8.0, r.y + 21.0, r.w - 12.0, 24.0),
            v.clone().unwrap_or_else(|| "—".into()),
            TextStyle { size: 16.0, weight: Weight::Medium, mono: true },
            TEXT,
        );
    }
    let (rx, rw) = cols[2];
    let mut y = TOP;
    if m.config.game.break_after_min > 0 {
        let left = m.break_due_ms.map(|d| format::clock(d.saturating_sub(now).div_ceil(1000))).unwrap_or_else(|| "—".into());
        b.line(Rect::new(rx, y, rw, 16.0), "BREAK IN", S_TINY, LABEL);
        b.line(Rect::new(rx, y + 14.0, rw, 22.0), left, TextStyle { size: 15.0, weight: Weight::Medium, mono: true }, TEXT);
        b.pill(Rect::new(rx, y + 40.0, rw, 26.0), Some(Icon::Clock), "Snooze 15 min", Btn::Secondary, Action::BreakSnooze);
        y += 74.0;
    }
    if let Some(c) = m.captures.first() {
        let r = Rect::new(rx, y, rw, 30.0);
        b.rect(r, 8.0, SURFACE_2);
        let what = if c.kind == CaptureKind::Clip { "Clip saved" } else { "Screenshot" };
        b.icon(capture_icon(c.kind), Rect::new(r.x + 8.0, r.y + 8.0, 14.0, 14.0), PORTS);
        b.line(Rect::new(r.x + 28.0, r.y, r.w - 60.0, 30.0), what, S_SMALL, TEXT);
        b.mini_icon(Rect::new(r.right() - 28.0, r.y + 3.0, 24.0, 24.0), Icon::Copy, TEXT, Some(Action::CaptureCopy(0)), "Copy capture");
    }
    TOP + hero_h
}

fn captures(m: &Model, b: &mut B) -> f32 {
    let now = m.now_ms;
    if m.captures.is_empty() && m.downloads.is_empty() {
        return empty(b, "New screenshots, clips and downloads show up here");
    }
    let all = columns(PAD, b.full_w, &[1.0, 1.0]);
    // Downloads in progress take the first column.
    let cols: Vec<(f32, f32)> = if m.downloads.is_empty() {
        all.clone()
    } else {
        b.at(all[0]);
        super::more::downloads(m, b, TOP);
        vec![all[1]]
    };
    if m.captures.is_empty() {
        return TOP + 34.0 * m.downloads.len() as f32;
    }
    let row_h = 38.0;
    let rows = fit_rows(row_h, 0.0);
    let mut bottom = TOP;
    for (i, c) in m.captures.iter().enumerate() {
        let Some(((x, w), y)) = flow(&cols, i, row_h, rows) else { break };
        bottom = bottom.max(y + row_h);
        let tile = Rect::new(x, y + 2.0, 44.0, 30.0);
        b.rect(tile, 6.0, SURFACE_2);
        b.icon(capture_icon(c.kind), Rect::new(tile.x + 15.0, tile.y + 8.0, 14.0, 14.0), TEXT);
        b.hits.push(Hit { rect: tile, kind: HitKind::Click(Action::CaptureOpen(i)), label: format!("Open {}", c.name) });
        let tw = w - 52.0 - 60.0;
        b.line(Rect::new(x + 52.0, y + 1.0, tw, 18.0), c.name.clone(), S_BODY, TEXT);
        b.line(Rect::new(x + 52.0, y + 18.0, tw, 14.0), format::relative(-((now.saturating_sub(c.at_ms) / 1000) as i64)), S_TINY, LABEL);
        b.mini_icon(
            Rect::new(x + w - 56.0, y + 4.0, 26.0, 26.0),
            Icon::Copy,
            TEXT,
            Some(Action::CaptureCopy(i)),
            &format!("Copy {}", c.name),
        );
        b.mini_icon(
            Rect::new(x + w - 26.0, y + 4.0, 26.0, 26.0),
            Icon::Folder,
            TEXT,
            Some(Action::CaptureFolder(i)),
            &format!("Show {} in folder", c.name),
        );
    }
    bottom - 4.0
}

fn privacy(m: &Model, b: &mut B) -> f32 {
    let now = m.now_ms;
    let cols = columns(PAD, b.full_w, &[2.0, 1.0]);
    let (x, w) = cols[0];
    let row_h = 34.0;
    let mut y = TOP;
    for p in m.privacy.iter().take(fit_rows(row_h, 0.0)) {
        let (icon, what) = match p.device {
            Device::Microphone => (Icon::Mic, "Mic"),
            Device::Camera => (Icon::Camera, "Camera"),
        };
        b.icon_tile(Rect::new(x, y + 3.0, 26.0, 26.0), icon, PRIVACY);
        b.line(Rect::new(x + 36.0, y + 1.0, w - 36.0 - 60.0, 18.0), p.app.clone(), S_BODY_M, TEXT);
        b.line(Rect::new(x + 36.0, y + 18.0, w - 36.0 - 60.0, 14.0), what, S_TINY, PRIVACY_TEXT);
        b.text_end(Rect::new(x + w - 60.0, y, 60.0, 32.0), format::clock(now.saturating_sub(p.since_ms) / 1000), S_MONO, TEXT_2);
        y += row_h;
    }
    let (bx, bw) = cols[1];
    let mut by = TOP;
    if m.privacy.iter().any(|p| p.device == Device::Microphone) {
        b.pill(Rect::new(bx, by, bw, 30.0), Some(Icon::SpeakerMuted), "Mute mic", Btn::Danger, Action::MicMute);
        by += 38.0;
    }
    let dev = m.privacy.first().map(|p| p.device).unwrap_or(Device::Microphone);
    b.pill(Rect::new(bx, by, bw, 30.0), Some(Icon::Gear), "Settings", Btn::Secondary, Action::PrivacySettings(dev));
    (y - 2.0).max(by + 30.0)
}

fn prayer(m: &Model, b: &mut B) -> f32 {
    let (Some(day), Some(now)) = (m.prayer_day, m.local_minute()) else { return empty(b, "Waiting for the clock…") };
    let cols = columns(PAD, b.full_w, &[1.0, 2.2]);
    let (hx, hw) = cols[0];
    let end = prayer_headline(b, m, day, now, hx, TOP + 4.0, hw);
    let (tx, tw) = cols[1];
    let end = prayer_timeline(b, day, now, tx, TOP + 22.0, tw).max(end);
    prayer_footer(b, m, tx, end + 14.0, tw)
}

fn weather(m: &Model, b: &mut B) -> f32 {
    let offline = m.online == Some(false);
    let cols = columns(PAD, b.full_w, &[1.05, 1.15, 1.0]);
    let (hx, hw) = cols[0];
    let mut bottom = TOP + 22.0;
    match &m.weather {
        Some(w) => {
            bottom = weather_now(b, w, hx, TOP + 8.0, hw);
            let (mx, mw) = cols[1];
            let y = weather_hours(b, w, mx, TOP, mw, 66.0) + 14.0;
            bottom = bottom.max(weather_readings(b, w, mx, y, mw));
        }
        None => {
            let msg = if offline { "No connection. Weather loads once you're online." } else { "Loading weather…" };
            b.text(Rect::new(hx, TOP, hw + cols[1].1, 22.0), msg, S_BODY, TEXT_2);
        }
    }
    b.mini_icon(Rect::new(hx + hw - 24.0, TOP + 80.0, 24.0, 24.0), Icon::Refresh, TEXT_2, Some(Action::RefreshWeather), "Refresh weather");
    bottom = bottom.max(TOP + 104.0);
    // Right: early warnings.
    let (ax, aw) = cols[2];
    let alerts = m.relevant_alerts(2);
    let color = if alerts.is_empty() { OK } else { MEDIA };
    b.prims.push(Prim::Circle { cx: ax + 3.0, cy: TOP + 7.0, r: 3.0, fill: Some(color), stroke: None });
    b.line(Rect::new(ax + 10.0, TOP, aw - 10.0, 14.0), "EARLY WARNINGS", S_TINY, LABEL);
    let mut y = TOP + 20.0;
    if alerts.is_empty() {
        let place = format::ellipsize(&m.config.location.city, 18);
        let msg = if offline && m.alerts.is_empty() { "Unavailable offline".to_string() } else { format!("All clear near {place}") };
        b.line(Rect::new(ax, y, aw, 18.0), msg, S_SMALL, TEXT_2);
        y += 24.0;
    }
    for a in alerts {
        y = weather_alert(b, m, &a, Rect::new(ax, y, aw, 34.0)) + 6.0;
    }
    let sy = (TOP + 92.0).max(y);
    b.line(
        Rect::new(ax, sy, aw, 12.0),
        weather_source(m),
        TextStyle { size: 8.5, weight: Weight::Regular, mono: true },
        if offline { MEDIA } else { LABEL },
    );
    bottom.max(sy + 12.0)
}

fn todo(m: &Model, b: &mut B) -> f32 {
    let cols = columns(PAD, b.full_w, &[1.0, 1.0, 0.55]);
    let row_h = 28.0;
    let mut list: Vec<&TodoItem> = m.todos.iter().filter(|t| !t.done).collect();
    list.extend(m.todos.iter().filter(|t| t.done));
    let item = |b: &mut B, (x, w): (f32, f32), y: f32, t: &TodoItem| {
        let c = Point::new(x + 9.0, y + 14.0);
        if t.done {
            b.prims.extend(soft::raised_circle(c.x, c.y, 8.0, OK, 1.5));
            b.icon(Icon::Check, Rect::new(c.x - 5.0, c.y - 5.0, 10.0, 10.0), SURFACE);
        } else {
            b.prims.extend(soft::well_circle(c.x, c.y, 8.0));
        }
        let label = if t.done { format!("Mark \"{}\" as not done", t.text) } else { format!("Mark \"{}\" as done", t.text) };
        b.hits.push(Hit { rect: Rect::new(x - 2.0, y + 2.0, 24.0, 24.0), kind: HitKind::Click(Action::TodoToggle(t.id)), label });
        let (tx, tw) = (x + 26.0, w - 26.0 - 26.0);
        b.line(Rect::new(tx, y, tw, row_h), t.text.clone(), S_BODY, if t.done { LABEL } else { TEXT });
        if t.done {
            let lw = b.tm.width(&t.text, S_BODY).min(tw);
            b.prims.push(Prim::RoundRect { rect: Rect::new(tx, y + 14.0, lw, 1.0), radius: 0.0, fill: Some(LABEL), stroke: None });
        }
        b.icon_button(
            Rect::new(x + w - 24.0, y + 2.0, 24.0, 24.0),
            Icon::Trash,
            TEXT_2,
            None,
            Some(Action::TodoRemove(t.id)),
            &format!("Delete \"{}\"", t.text),
        );
    };
    let rows = fit_rows(row_h, 0.0);
    let mut bottom = TOP;
    // Typing: the field in the first column, the list in the second.
    if let Some(input) = m.typing(InputFor::Todo) {
        let (x, w) = cols[0];
        let field = Rect::new(x, TOP, w, 34.0);
        b.prims.extend(soft::well(field, 10.0));
        let (tx, tw) = (field.x + 12.0, field.w - 24.0);
        if input.is_empty() {
            b.line(Rect::new(tx, TOP, tw, 34.0), "What needs doing?", S_BODY, LABEL);
            b.rect(Rect::new(tx, TOP + 9.0, 2.0, 16.0), 1.0, OK);
        } else {
            let mut shown = input.to_string();
            while b.tm.width(&shown, S_BODY) > tw - 6.0 && !shown.is_empty() {
                shown.remove(0);
            }
            let w = b.tm.width(&shown, S_BODY).min(tw - 4.0);
            b.line(Rect::new(tx, TOP, tw, 34.0), shown, S_BODY, TEXT);
            b.rect(Rect::new(tx + w + 2.0, TOP + 9.0, 2.0, 16.0), 1.0, OK);
        }
        let bw = (w - 8.0) / 2.0;
        b.pill(Rect::new(x, TOP + 42.0, bw, 30.0), Some(Icon::Check), "Add", Btn::Primary(OK), Action::TodoSubmit);
        b.pill(Rect::new(x + bw + 8.0, TOP + 42.0, bw, 30.0), Some(Icon::Cross), "Cancel", Btn::Secondary, Action::TodoCancel);
        b.line(Rect::new(x, TOP + 78.0, w, 14.0), "Enter adds · Esc cancels", S_TINY, LABEL);
        bottom = TOP + 92.0;
        let list_cols = [cols[1], cols[2]];
        let list_cols = [(list_cols[0].0, list_cols[1].0 + list_cols[1].1 - list_cols[0].0)];
        for (i, t) in list.iter().take(rows).enumerate() {
            let Some((col, y)) = flow(&list_cols, i, row_h, rows) else { break };
            item(b, col, y, t);
            bottom = bottom.max(y + row_h);
        }
        return bottom;
    }
    // The list over two columns; a count and the buttons on the right.
    let list_cols = [cols[0], cols[1]];
    let capacity = rows * 2;
    let shown = if list.len() > capacity { capacity - 1 } else { list.len() };
    if list.is_empty() {
        b.line(Rect::new(PAD, TOP, cols[1].0 + cols[1].1 - PAD, 22.0), "Nothing to do. Add a task.", S_BODY, TEXT_2);
        bottom = TOP + 22.0;
    }
    for (i, t) in list.iter().take(shown).enumerate() {
        let Some((col, y)) = flow(&list_cols, i, row_h, rows) else { break };
        item(b, col, y, t);
        bottom = bottom.max(y + row_h);
    }
    if list.len() > shown {
        let ((x, _), y) = flow(&list_cols, shown, row_h, rows).unwrap_or((cols[1], TOP));
        b.line(Rect::new(x + 26.0, y, 120.0, row_h), format!("+{} more", list.len() - shown), S_MONO, LABEL);
        bottom = bottom.max(y + row_h);
    }
    let (bx, bw) = cols[2];
    let open = m.todos.iter().filter(|t| !t.done).count();
    b.line(
        Rect::new(bx, TOP - 4.0, bw, 34.0),
        open.to_string(),
        TextStyle { size: 26.0, weight: Weight::Semibold, mono: false },
        if open > 0 { OK } else { TEXT_2 },
    );
    b.line(Rect::new(bx, TOP + 28.0, bw, 14.0), "LEFT TO DO", S_TINY, LABEL);
    b.pill(Rect::new(bx, TOP + 48.0, bw, 28.0), Some(Icon::Plus), "Add task", Btn::Primary(OK), Action::TodoAdd);
    let mut end = TOP + 76.0;
    if m.todos.iter().any(|t| t.done) {
        b.pill(Rect::new(bx, TOP + 82.0, bw, 26.0), Some(Icon::Check), "Clear done", Btn::Secondary, Action::TodoClearDone);
        end = TOP + 108.0;
    }
    bottom.max(end) - 2.0
}

fn calendar(m: &Model, b: &mut B) -> f32 {
    use crate::calendar::{civil_from_days, day_label, weekday};
    let offset = m.clock.map(|c| c.utc_offset_min as i64 * 60).unwrap_or(0);
    let Some(now) = m.unix_now() else { return empty(b, "Waiting for the clock…") };
    let today = (now + offset).div_euclid(86_400);
    let (_, month, day) = civil_from_days(today);
    let months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let names = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let cols = columns(PAD, b.full_w, &[1.0, 1.75]);
    let (x, w) = cols[0];
    // Left: the date, large, and this week under it.
    b.line(Rect::new(x, TOP - 4.0, 52.0, 44.0), day.to_string(), TextStyle { size: 34.0, weight: Weight::Semibold, mono: false }, TEXT);
    b.line(Rect::new(x + 54.0, TOP + 2.0, w - 54.0, 18.0), names[weekday(today) as usize], S_TITLE, TEXT);
    let today_count = m.calendar.iter().filter(|e| (e.start + offset).div_euclid(86_400) == today).count();
    let line = match today_count {
        0 => format!("{} · free today", months[month as usize - 1]),
        1 => format!("{} · 1 event", months[month as usize - 1]),
        n => format!("{} · {n} events", months[month as usize - 1]),
    };
    b.line(Rect::new(x + 54.0, TOP + 20.0, w - 54.0, 16.0), line, S_TINY, CODER);
    let monday = today - weekday(today) as i64;
    let gap = 4.0;
    let cw = (w - gap * 6.0) / 7.0;
    let wy = TOP + 50.0;
    for i in 0..7 {
        let d = monday + i;
        let r = Rect::new(x + i as f32 * (cw + gap), wy, cw, 46.0);
        let is_today = d == today;
        if is_today {
            b.prims.extend(soft::raised_grad(r, 9.0, accent_light(CODER), accent_deep(CODER), 1.5));
        } else {
            b.prims.extend(soft::raised(r, 9.0, SURFACE, 1.5));
        }
        let center = |t: String, yy: f32, h: f32, style: TextStyle, c: Color| Prim::Text {
            rect: Rect::new(r.x, yy, r.w, h),
            text: t,
            style,
            color: c,
            align: Align::Center,
            wrap: false,
        };
        b.prims.push(center(
            ["M", "T", "W", "T", "F", "S", "S"][i as usize].into(),
            r.y + 4.0,
            12.0,
            S_TINY,
            if is_today { SURFACE } else { LABEL },
        ));
        b.prims.push(center(
            civil_from_days(d).2.to_string(),
            r.y + 16.0,
            18.0,
            TextStyle { size: 13.0, weight: Weight::Semibold, mono: false },
            if is_today { SURFACE } else { TEXT },
        ));
        let mut busy: Vec<usize> = m.calendar.iter().filter(|e| (e.start + offset).div_euclid(86_400) == d).map(|e| e.feed).collect();
        busy.dedup();
        busy.truncate(3);
        let dots = busy.len() as f32;
        for (k, feed) in busy.into_iter().enumerate() {
            let cx = r.x + r.w / 2.0 + (k as f32 - (dots - 1.0) / 2.0) * 6.0;
            b.prims.push(Prim::Circle { cx, cy: r.y + 39.0, r: 2.0, fill: Some(feed_color(m, feed)), stroke: None });
        }
    }
    let left_end = wy + 46.0;
    // Right: what is coming up, one line each.
    let (ex, ew) = cols[1];
    if m.config.calendar.feeds.is_empty() {
        b.line(Rect::new(ex, TOP, ew, 20.0), "No calendar linked yet.", S_BODY, TEXT_2);
        b.pill(
            Rect::new(ex, TOP + 28.0, 170.0, 30.0),
            Some(Icon::Plus),
            "Link a calendar",
            Btn::Primary(CODER),
            Action::OpenSettings(ModuleId::Calendar),
        );
        return left_end;
    }
    let horizon = now + m.config.calendar.days as i64 * 86_400;
    let upcoming: Vec<&crate::calendar::CalEvent> = m.calendar.iter().filter(|e| e.end > now && e.start < horizon).collect();
    if upcoming.is_empty() {
        b.line(Rect::new(ex, TOP, ew, 20.0), "Nothing coming up", S_BODY, TEXT_2);
        return left_end;
    }
    let row_h = 26.0;
    let mut y = TOP;
    for e in upcoming.into_iter().take(fit_rows(row_h + 2.0, 2.0)) {
        let d = (e.start + offset).div_euclid(86_400).max(today);
        let r = Rect::new(ex, y, ew, row_h);
        b.prims.extend(soft::well(r, 8.0));
        b.prims.push(Prim::RoundRect {
            rect: Rect::new(r.x + 6.0, r.y + 6.0, 3.0, r.h - 12.0),
            radius: 1.5,
            fill: Some(feed_color(m, e.feed)),
            stroke: None,
        });
        let hm = |t: i64| {
            let s = (t + offset).rem_euclid(86_400);
            format!("{:02}:{:02}", s / 3600, s % 3600 / 60)
        };
        let dl = match d - today {
            0 => String::new(),
            1 => "Tmr ".into(),
            _ => format!("{} ", day_label(d, today).split(' ').next().unwrap_or("")),
        };
        let when = if e.all_day {
            format!("{dl}all day")
        } else if e.start <= now {
            "now".to_string()
        } else {
            format!("{dl}{}", hm(e.start))
        };
        b.line(Rect::new(r.x + 16.0, r.y, 84.0, r.h), when, S_MONO, TEXT_2);
        let place = if e.location.is_empty() {
            m.config.calendar.feeds.get(e.feed).map(|f| f.name.clone()).unwrap_or_default()
        } else {
            e.location.clone()
        };
        let pw = b.tm.width(&place, S_TINY).ceil().min(ew * 0.3);
        b.line(Rect::new(r.x + 100.0, r.y, r.w - 110.0 - pw - 8.0, r.h), e.title.clone(), S_BODY_M, TEXT);
        b.text_end(Rect::new(r.right() - 8.0 - pw, r.y, pw, r.h), place, S_TINY, LABEL);
        if !e.link.is_empty() {
            // The meeting link: the whole row joins it.
            b.prims.push(Prim::Text {
                rect: Rect::new(r.right() - 64.0, r.bottom() - 18.0, 56.0, 14.0),
                text: "Join ›".into(),
                style: TextStyle { size: 10.5, weight: Weight::Semibold, mono: false },
                color: CODER,
                align: Align::End,
                wrap: false,
            });
            b.hits.push(Hit { rect: r, kind: HitKind::Click(Action::OpenUrl(e.link.clone())), label: format!("Join {}", e.title) });
        }
        y += row_h + 2.0;
    }
    (y - 2.0).max(left_end)
}

fn finance(m: &Model, b: &mut B) -> f32 {
    let list = &m.config.finance.watchlist;
    if list.is_empty() {
        let y = empty(b, "Your watchlist is empty.");
        b.pill(
            Rect::new(PAD, y + 8.0, 160.0, 30.0),
            Some(Icon::Plus),
            "Add symbols",
            Btn::Primary(GAME),
            Action::OpenSettings(ModuleId::Finance),
        );
        return y + 38.0;
    }
    let news = !m.news.is_empty();
    let cols = if news { columns(PAD, b.full_w, &[1.0, 1.15]) } else { columns(PAD, b.full_w, &[1.0, 1.0]) };
    let quote_cols: Vec<(f32, f32)> = if news { vec![cols[0]] } else { cols.clone() };
    let row_h = 36.0;
    let rows = fit_rows(row_h, 0.0);
    let capacity = rows * quote_cols.len();
    let fit = if list.len() > capacity { capacity - 1 } else { list.len() };
    let mut bottom = TOP;
    for (i, item) in list.iter().take(fit).enumerate() {
        let Some(((x, w), y)) = flow(&quote_cols, i, row_h, rows) else { break };
        bottom = bottom.max(y + row_h);
        let q = m.quotes.iter().find(|q| q.symbol.eq_ignore_ascii_case(&item.symbol));
        let r = Rect::new(x, y, w, row_h - 4.0);
        b.prims.extend(soft::raised(r, 9.0, SURFACE, 1.5));
        let name = if item.name.is_empty() { q.map(|q| q.name.clone()).unwrap_or_default() } else { item.name.clone() };
        let name_w = (w * 0.38).min(130.0);
        b.line(Rect::new(r.x + 10.0, r.y + 1.0, name_w, 18.0), name, S_BODY_M, TEXT);
        b.line(Rect::new(r.x + 10.0, r.y + 17.0, name_w, 13.0), item.symbol.clone(), S_TINY, LABEL);
        match q {
            Some(q) => {
                let up = q.change_pct >= 0.0;
                let color = if up { OK } else { PRIVACY_TEXT };
                let spark = Rect::new(r.x + 14.0 + name_w, r.y + 5.0, 48.0, r.h - 10.0);
                if q.points.len() >= 2 {
                    let (lo, hi) = q.points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
                    let span = (hi - lo).max(f32::EPSILON);
                    let n = q.points.len() as f32 - 1.0;
                    let points = q
                        .points
                        .iter()
                        .enumerate()
                        .map(|(i, v)| Point::new(spark.x + spark.w * i as f32 / n, spark.bottom() - spark.h * (v - lo) / span))
                        .collect();
                    b.prims.push(Prim::Polyline { points, width: 1.5, color, closed: false });
                }
                let px = spark.right() + 6.0;
                let pw = r.right() - 10.0 - px;
                b.text_end(
                    Rect::new(px, r.y + 1.0, pw, 18.0),
                    crate::finance::format_price(q),
                    TextStyle { size: 12.5, weight: Weight::Medium, mono: true },
                    TEXT,
                );
                b.text_end(
                    Rect::new(px, r.y + 17.0, pw, 13.0),
                    crate::finance::format_change(q.change_pct),
                    TextStyle { size: 10.0, weight: Weight::Medium, mono: true },
                    color,
                );
            }
            None => {
                let msg = if m.online == Some(false) { "offline" } else { "loading…" };
                b.text_end(Rect::new(r.x + name_w, r.y, r.w - name_w - 10.0, r.h), msg, S_TINY, LABEL);
            }
        }
    }
    if list.len() > fit {
        let ((x, w), y) = flow(&quote_cols, fit, row_h, rows).unwrap_or((quote_cols[0], TOP));
        b.text_end(Rect::new(x, y, w, row_h - 4.0), format!("+{} more in Settings", list.len() - fit), S_TINY, LABEL);
        bottom = bottom.max(y + row_h);
    }
    if news {
        let (x, w) = cols[1];
        let now = m.unix_now();
        let mut y = TOP;
        for n in m.news.iter().take(rows) {
            let r = Rect::new(x, y, w, row_h - 4.0);
            b.rect(Rect::new(r.x, r.y + 4.0, 3.0, r.h - 8.0), 1.5, GAME);
            b.line(Rect::new(r.x + 10.0, r.y, r.w - 10.0, 18.0), n.title.clone(), S_SMALL, TEXT);
            let age = now.map(|now| format!(" · {}", ago((now - n.time).max(60)))).unwrap_or_default();
            b.line(
                Rect::new(r.x + 10.0, r.y + 17.0, r.w - 10.0, 13.0),
                format!("{} · {}{age}", n.symbol.trim_start_matches('^'), n.publisher),
                S_TINY,
                LABEL,
            );
            b.hits.push(Hit { rect: r, kind: HitKind::Click(Action::OpenUrl(n.link.clone())), label: n.title.clone() });
            y += row_h;
        }
        bottom = bottom.max(y);
    }
    bottom - 4.0
}

/// "5m ago", "3h ago", "2d ago".
pub(super) fn ago(secs: i64) -> String {
    match secs / 60 {
        m if m < 60 => format!("{m}m ago"),
        m if m < 48 * 60 => format!("{}h ago", m / 60),
        m => format!("{}d ago", m / 1440),
    }
}

fn notes(m: &Model, b: &mut B) -> f32 {
    let cols = columns(PAD, b.full_w, &[1.0, 2.2]);
    let (x, w) = cols[0];
    let end = match m.typing(InputFor::Note) {
        Some(text) => {
            super::more::field(b, Rect::new(x, TOP, w, 32.0), text, "Write a note…", OK);
            let bw = (w - 8.0) / 2.0;
            b.pill(Rect::new(x, TOP + 40.0, bw, 30.0), Some(Icon::Check), "Save", Btn::Primary(OK), Action::TodoSubmit);
            b.pill(Rect::new(x + bw + 8.0, TOP + 40.0, bw, 30.0), Some(Icon::Cross), "Cancel", Btn::Secondary, Action::TodoCancel);
            b.line(Rect::new(x, TOP + 78.0, w, 14.0), "Enter saves · Esc cancels", S_TINY, LABEL);
            TOP + 92.0
        }
        None => {
            b.line(
                Rect::new(x, TOP - 4.0, w, 34.0),
                m.notes.len().to_string(),
                TextStyle { size: 26.0, weight: Weight::Semibold, mono: false },
                OK,
            );
            b.line(Rect::new(x, TOP + 28.0, w, 14.0), "NOTES", S_TINY, LABEL);
            b.pill(Rect::new(x, TOP + 48.0, w, 30.0), Some(Icon::Plus), "New note", Btn::Primary(OK), Action::NoteAdd);
            TOP + 78.0
        }
    };
    b.at(cols[1]);
    if m.notes.is_empty() {
        b.line(Rect::new(b.x, TOP, b.w, 20.0), "No notes yet. ngambang note \"…\" adds one from a terminal.", S_SMALL, TEXT_2);
        return end.max(TOP + 20.0);
    }
    // Typing: the list stays beside the field (the shared list skips its own field).
    let typing = m.typing(InputFor::Note).is_some();
    let y = if typing { notes_list(m, b) } else { super::more::notes(m, b, TOP, false) };
    y.max(end)
}

/// The notes alone (while the field in the first column is open).
fn notes_list(m: &Model, b: &mut B) -> f32 {
    let mut y = TOP;
    for n in m.notes.iter().take(fit_rows(38.0, 0.0)) {
        b.rect(Rect::new(b.x, y + 5.0, 3.0, 24.0), 1.5, OK);
        b.line(Rect::new(b.x + 12.0, y, b.w - 12.0, 34.0), n.text.clone(), S_BODY, TEXT);
        y += 38.0;
    }
    y
}

fn power(m: &Model, b: &mut B) -> f32 {
    let cols = columns(PAD, b.full_w, &[1.0, 1.2]);
    b.at(cols[0]);
    let a = super::more::power_now(m, b, TOP);
    b.at(cols[1]);
    let c = super::more::power_plans(m, b, TOP, 28.0);
    if m.power_plans.is_empty() {
        b.line(Rect::new(b.x, TOP, b.w, 18.0), "Power plans load in a moment…", S_SMALL, TEXT_2);
    }
    a.max(c)
}

fn launch(m: &Model, b: &mut B) -> f32 {
    let items = !m.config.launch.items.is_empty();
    if items {
        let label = if m.launch_edit { "Done" } else { "Edit" };
        let icon = if m.launch_edit { Icon::Check } else { Icon::Pencil };
        b.pill(Rect::new(PAD + b.full_w - 72.0, TOP, 72.0, 26.0), Some(icon), label, Btn::Secondary, Action::LaunchEdit);
    }
    b.at((PAD, if items { b.full_w - 84.0 } else { b.full_w }));
    super::more::launch(m, b, TOP, 58.0, 7)
}
