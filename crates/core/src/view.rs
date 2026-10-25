//! Turn the model into a [`Scene`]: drawing primitives plus hit regions, in
//! DIPs local to the scene. The platform renders primitives with Direct2D and
//! animates between scenes with the compositor.

use crate::clipboard::{self, ClipKind};
use crate::config::ModuleId;
use crate::format;
use crate::geometry::{Color, Edge, Point, Rect, Size};
use crate::icons::{arc_points, icon_prims, wave_points};
use crate::model::*;
use crate::soft;
use crate::theme::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub weight: Weight,
    pub mono: bool,
}

impl TextStyle {
    pub const LABEL: TextStyle = TextStyle { size: 11.0, weight: Weight::Medium, mono: true };
    pub const BODY: TextStyle = TextStyle { size: 14.0, weight: Weight::Regular, mono: false };
    pub const BODY_MEDIUM: TextStyle = TextStyle { size: 14.0, weight: Weight::Medium, mono: false };
    pub const SMALL: TextStyle = TextStyle { size: 13.0, weight: Weight::Regular, mono: false };
    pub const TITLE: TextStyle = TextStyle { size: 17.0, weight: Weight::Semibold, mono: false };
    pub const MONO: TextStyle = TextStyle { size: 13.0, weight: Weight::Regular, mono: true };
    pub const MONO_SMALL: TextStyle = TextStyle { size: 12.0, weight: Weight::Regular, mono: true };
    pub const BIG: TextStyle = TextStyle { size: 40.0, weight: Weight::Medium, mono: true };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    RoundRect {
        rect: Rect,
        radius: f32,
        fill: Option<Color>,
        stroke: Option<(Color, f32)>,
    },
    Circle {
        cx: f32,
        cy: f32,
        r: f32,
        fill: Option<Color>,
        stroke: Option<(Color, f32)>,
    },
    Polyline {
        points: Vec<Point>,
        width: f32,
        color: Color,
        closed: bool,
    },
    Polygon {
        points: Vec<Point>,
        fill: Color,
    },
    /// A picture (album art) filling `rect` with rounded corners.
    Image {
        rect: Rect,
        art: Art,
        radius: f32,
    },
    /// Single line unless `wrap`; overflow is trimmed with an ellipsis and clipped to `rect`.
    Text {
        rect: Rect,
        text: String,
        style: TextStyle,
        color: Color,
        align: Align,
        wrap: bool,
    },
}

impl Prim {
    pub fn bounds(&self) -> Rect {
        match self {
            Prim::RoundRect { rect, .. } | Prim::Text { rect, .. } | Prim::Image { rect, .. } => *rect,
            Prim::Circle { cx, cy, r, .. } => Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r),
            Prim::Polyline { points, .. } | Prim::Polygon { points, .. } => {
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for p in points {
                    x0 = x0.min(p.x);
                    y0 = y0.min(p.y);
                    x1 = x1.max(p.x);
                    y1 = y1.max(p.y);
                }
                Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HitKind {
    Click(Action),
    Slider(SliderTarget),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub rect: Rect,
    pub kind: HitKind,
    /// Accessible name, also used for keyboard focus.
    pub label: String,
}

/// The progress ring around the orb, animated by the compositor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingSpec {
    pub center: Point,
    pub radius: f32,
    pub width: f32,
    pub color: Color,
    /// Fraction drawn now, 0..=1.
    pub value: f32,
    /// Growth per millisecond; 0 = static.
    pub rate_per_ms: f32,
}

impl RingSpec {
    /// True when `self` is what `prev` would show after `elapsed_ms` — the
    /// running animation can keep going instead of restarting.
    pub fn continues(&self, prev: &RingSpec, elapsed_ms: u64) -> bool {
        let predicted = (prev.value + prev.rate_per_ms * elapsed_ms as f32).min(1.0);
        self.color == prev.color
            && self.center == prev.center
            && (self.rate_per_ms - prev.rate_per_ms).abs() < 1e-9
            && (self.value - predicted).abs() < 0.01
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneKind {
    Hidden,
    Seam,
    Orb,
    Tag,
    Slip(ModuleId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub kind: SceneKind,
    pub size: Size,
    /// Sits directly on the screen edge (the Seam).
    pub flush: bool,
    /// Corner radius for the compositor's morph clip.
    pub clip_radius: f32,
    pub prims: Vec<Prim>,
    pub hits: Vec<Hit>,
    pub ring: Option<RingSpec>,
}

impl Scene {
    pub fn hit_at(&self, x: f32, y: f32) -> Option<&Hit> {
        // Last added wins: buttons are added after the surfaces under them.
        self.hits.iter().rev().find(|h| h.rect.contains(x, y))
    }
}

pub trait TextMeasure {
    fn width(&self, text: &str, style: TextStyle) -> f32;
}

/// Rough measure for tests and as a fallback.
pub struct ApproxMeasure;

impl TextMeasure for ApproxMeasure {
    fn width(&self, text: &str, style: TextStyle) -> f32 {
        let per = if style.mono { 0.6 } else { 0.52 };
        text.chars().count() as f32 * style.size * per
    }
}

/// What a hover tag or Seam peek says.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub icon: Icon,
    pub color: Color,
    pub text: String,
    pub detail: Option<String>,
    pub wave: bool,
}

pub fn summary(m: &Model) -> Option<Summary> {
    let now = m.now_ms;
    let game_line = || {
        m.game.as_ref().map(|g| {
            let mut parts = vec![format::clock(now.saturating_sub(g.started_ms) / 1000)];
            if let Some(v) = m.stats.gpu_pct {
                parts.push(format!("GPU {v:.0}%"));
            }
            if let Some(t) = m.stats.temp_c {
                parts.push(format!("{t}°C"));
            }
            if let Some(p) = m.stats.ping_ms {
                parts.push(format!("ping {p}ms"));
            }
            Summary { icon: Icon::Game, color: GAME, text: parts.join("  "), detail: None, wave: false }
        })
    };
    if m.fullscreen == Fullscreen::Game {
        if let Some(s) = game_line() {
            return Some(s);
        }
    }
    if let Some(p) = m.privacy.first() {
        let (icon, what) = match p.device {
            Device::Microphone => (Icon::Mic, "Microphone"),
            Device::Camera => (Icon::Camera, "Camera"),
        };
        return Some(Summary {
            icon,
            color: PRIVACY,
            text: format!("{what} · {}", p.app),
            detail: Some(format::clock(now.saturating_sub(p.since_ms) / 1000)),
            wave: false,
        });
    }
    if let Some(rem) = m.timer.remaining(now) {
        let paused = !m.timer.is_running();
        return Some(Summary {
            icon: Icon::Clock,
            color: GAME,
            text: if paused { "Focus paused".into() } else { "Focus".into() },
            detail: Some(format::clock(rem / 1000)),
            wave: false,
        });
    }
    if let Some(j) = m.jobs.iter().find(|j| matches!(j.status, JobStatus::Running { .. })) {
        let detail = match j.status {
            JobStatus::Running { progress: Some(p) } => format!("{:.0}%", p * 100.0),
            _ => format::duration_short(now.saturating_sub(j.started_ms) / 1000),
        };
        return Some(Summary { icon: Icon::Terminal, color: CODER, text: j.title.clone(), detail: Some(detail), wave: false });
    }
    if let Some(s) = m.front_media().filter(|s| !s.title.is_empty()) {
        return Some(Summary {
            icon: Icon::Note,
            color: MEDIA,
            text: s.title.clone(),
            detail: Some(format::clock(s.position_at(now) / 1000)),
            wave: s.playing,
        });
    }
    if let Some(s) = game_line() {
        return Some(s);
    }
    if let (Some(e), Some(now)) = (m.next_event_within(60 * 60), m.unix_now()) {
        let mins = ((e.start - now) as f64 / 60.0).ceil() as i64;
        return Some(Summary {
            icon: Icon::Calendar,
            color: CODER,
            text: e.title.clone(),
            detail: Some(if mins <= 0 { "now".into() } else { format!("in {mins}m") }),
            wave: false,
        });
    }
    if let Some(w) = &m.weather {
        return Some(Summary {
            icon: condition_icon(w.condition),
            color: PORTS,
            text: format!("{:.0}° · {}", w.temp_c, w.description),
            detail: None,
            wave: false,
        });
    }
    let open_todos = m.todos.iter().filter(|t| !t.done).count();
    if m.jobs.is_empty() && open_todos > 0 && m.enabled(ModuleId::Todo) {
        let first = m.todos.iter().find(|t| !t.done).map(|t| t.text.clone()).unwrap_or_default();
        return Some(Summary { icon: Icon::Todo, color: OK, text: first, detail: Some(format!("{open_todos} left")), wave: false });
    }
    m.jobs.first().map(|j| {
        let (icon, color, detail) = match &j.status {
            JobStatus::Done { ok: true, duration_s, .. } => (Icon::Check, OK, duration_s.map(|d| format::duration_short(d.round() as u64))),
            JobStatus::Done { ok: false, code, .. } => {
                (Icon::Cross, PRIVACY, Some(code.map(|c| format!("exit {c}")).unwrap_or_else(|| "failed".into())))
            }
            _ => (Icon::Terminal, CODER, None),
        };
        Summary { icon, color, text: j.title.clone(), detail, wave: false }
    })
}

/// The ring shown on the orb and tag, if any.
pub fn ring(m: &Model) -> Option<(Color, f32, f32)> {
    let now = m.now_ms;
    if now < m.volume_flash_until {
        return Some((TEXT, m.mixer.master.level, 0.0));
    }
    if !m.privacy.is_empty() {
        return Some((PRIVACY, 1.0, 0.0));
    }
    match m.timer {
        FocusTimer::Running { total_ms, .. } => return Some((GAME, m.timer.progress(now)?, 1.0 / total_ms.max(1) as f32)),
        FocusTimer::Paused { .. } => return Some((GAME, m.timer.progress(now)?, 0.0)),
        FocusTimer::Idle => {}
    }
    if let Some(j) = m.jobs.iter().find(|j| matches!(j.status, JobStatus::Running { .. })) {
        let JobStatus::Running { progress } = j.status else { unreachable!() };
        return Some((CODER, progress.unwrap_or(0.25), 0.0));
    }
    if let Some(s) = m.front_media() {
        if let Some(p) = s.progress_at(now) {
            let rate = if s.playing { 1.0 / s.duration_ms as f32 } else { 0.0 };
            return Some((MEDIA, p, rate));
        }
    }
    None
}

fn seam_colors(m: &Model) -> Vec<Color> {
    let mut v = Vec::new();
    if !m.privacy.is_empty() {
        v.push(PRIVACY);
    }
    if m.timer != FocusTimer::Idle || m.game.is_some() {
        v.push(GAME);
    }
    if m.jobs.iter().any(|j| matches!(j.status, JobStatus::Running { .. })) {
        v.push(CODER);
    }
    if m.front_media().is_some_and(|s| s.playing) {
        v.push(MEDIA);
    }
    v.truncate(3);
    v
}

pub fn build(m: &Model, edge: Edge, tm: &dyn TextMeasure) -> Scene {
    match m.state {
        IslandState::Hidden => Scene {
            kind: SceneKind::Hidden,
            size: Size::new(0.0, 0.0),
            flush: true,
            clip_radius: 0.0,
            prims: vec![],
            hits: vec![],
            ring: None,
        },
        IslandState::Seam => seam(m, edge),
        // Scrolling the volume: a glance with the level, from the orb or any tag.
        IslandState::Orb | IslandState::Tag if m.volume_flashing() => volume_tag(m, edge, tm),
        IslandState::Orb => orb(m),
        IslandState::Tag => tag(m, edge, tm),
        IslandState::Slips => match m.front {
            Some(front) => slips(m, front, tm),
            None => orb(m),
        },
    }
}

fn seam(m: &Model, edge: Edge) -> Scene {
    let size = if edge.is_vertical() { Size::new(SEAM_T, SEAM_L) } else { Size::new(SEAM_L, SEAM_T) };
    let mut colors = seam_colors(m);
    if colors.is_empty() {
        colors.push(TEXT.with_alpha(0x66));
    }
    let n = colors.len() as f32;
    let gap = 3.0;
    let len = (SEAM_L - gap * (n - 1.0)) / n;
    let prims = colors
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let o = i as f32 * (len + gap);
            let rect = if edge.is_vertical() { Rect::new(0.0, o, SEAM_T, len) } else { Rect::new(o, 0.0, len, SEAM_T) };
            Prim::RoundRect { rect, radius: 1.5, fill: Some(*c), stroke: None }
        })
        .collect();
    Scene { kind: SceneKind::Seam, size, flush: true, clip_radius: 1.5, prims, hits: vec![], ring: None }
}

fn ring_spec(m: &Model, center: Point) -> Option<RingSpec> {
    ring(m).map(|(color, value, rate)| RingSpec {
        center,
        radius: RING_R,
        width: RING_W,
        color,
        value: value.clamp(0.0, 1.0),
        rate_per_ms: rate,
    })
}

fn orb(m: &Model) -> Scene {
    let c = ORB_BOX / 2.0;
    let ring = ring_spec(m, Point::new(c, c));
    let dot = ring.map(|r| r.color).unwrap_or(TRACK);
    // A soft disc with a groove for the tide ring and a raised button in the middle.
    let mut prims = vec![Prim::Circle { cx: c, cy: c, r: c - 0.5, fill: Some(SURFACE), stroke: Some((HAIRLINE, 1.0)) }];
    prims.extend(soft::well_circle(c, c, RING_R + 2.5));
    prims.extend(soft::raised_circle(c, c, RING_R - 2.5, SURFACE, 1.5));
    prims.push(Prim::Circle { cx: c, cy: c, r: 3.5, fill: Some(dot), stroke: None });
    let hits =
        vec![Hit { rect: Rect::new(0.0, 0.0, ORB_BOX, ORB_BOX), kind: HitKind::Click(Action::ToggleSlips), label: "Ngambang".into() }];
    Scene { kind: SceneKind::Orb, size: Size::new(ORB_BOX, ORB_BOX), flush: false, clip_radius: ORB_BOX / 2.0, prims, hits, ring }
}

/// The volume glance: speaker, a bar and the level in percent.
fn volume_tag(m: &Model, edge: Edge, tm: &dyn TextMeasure) -> Scene {
    let h = TAG_H;
    let (who, v) = m.flashed_volume();
    let pct = format!("{:.0}%", v.level * 100.0);
    let who = format::ellipsize(&who, 16);
    let name_w = tm.width(&who, TextStyle::BODY_MEDIUM).ceil();
    let value_w = tm.width("100%", TextStyle::BODY_MEDIUM).ceil().max(36.0);
    let bar_w = 120.0;
    let w = 6.0 + 28.0 + 12.0 + name_w + 12.0 + bar_w + 12.0 + value_w + 16.0;
    let mut prims =
        vec![Prim::RoundRect { rect: Rect::new(0.0, 0.0, w, h), radius: 20.0, fill: Some(SURFACE), stroke: Some((HAIRLINE, 1.0)) }];
    let patch = if edge == Edge::Right { Rect::new(w / 2.0, 0.0, w / 2.0, h) } else { Rect::new(0.0, 0.0, w / 2.0, h) };
    prims.push(Prim::RoundRect { rect: patch, radius: 12.0, fill: Some(SURFACE), stroke: None });
    let c = Point::new(20.0, h / 2.0);
    prims.extend(soft::well_circle(c.x, c.y, RING_R + 2.5));
    let (icon, color) = if v.muted { (Icon::SpeakerMuted, PRIVACY) } else { (Icon::Speaker, MEDIA) };
    prims.extend(icon_prims(icon, Rect::new(c.x - 7.0, c.y - 7.0, 14.0, 14.0), color));
    prims.push(Prim::Text {
        rect: Rect::new(46.0, 0.0, name_w, h),
        text: who,
        style: TextStyle::BODY_MEDIUM,
        color: TEXT,
        align: Align::Start,
        wrap: false,
    });
    let bar = Rect::new(46.0 + name_w + 12.0, h / 2.0 - 5.0, bar_w, 10.0);
    prims.extend(soft::well(bar, 5.0));
    if v.level > 0.005 {
        let fill = Rect::new(bar.x + 2.0, bar.y + 2.5, (bar.w - 4.0) * v.level.clamp(0.0, 1.0), 5.0);
        prims.push(Prim::RoundRect { rect: fill, radius: 2.5, fill: Some(if v.muted { TRACK } else { MEDIA }), stroke: None });
    }
    prims.push(Prim::Text {
        rect: Rect::new(bar.right() + 12.0, 0.0, value_w, h),
        text: if v.muted { "Muted".into() } else { pct },
        style: TextStyle { size: 14.0, weight: Weight::Semibold, mono: true },
        color: TEXT,
        align: Align::End,
        wrap: false,
    });
    let hits = vec![Hit { rect: Rect::new(0.0, 0.0, w, h), kind: HitKind::Click(Action::ToggleSlips), label: "Volume".into() }];
    Scene { kind: SceneKind::Tag, size: Size::new(w, h), flush: false, clip_radius: 12.0, prims, hits, ring: None }
}

/// A now-playing tag: art, title and artist, and the player controls.
fn media_tag(m: &Model, s: &MediaSession, edge: Edge, tm: &dyn TextMeasure) -> Scene {
    let h = TAG_H;
    let lead = 28.0;
    let mut prims = Vec::new();
    let mut hits = Vec::new();
    let title = if s.title.is_empty() { s.app.clone() } else { s.title.clone() };
    let sub = if s.artist.is_empty() { s.app.clone() } else { s.artist.clone() };
    let title_style = TextStyle { size: 13.0, weight: Weight::Medium, mono: false };
    let sub_style = TextStyle { size: 11.5, weight: Weight::Regular, mono: false };
    let text_w = tm.width(&title, title_style).max(tm.width(&sub, sub_style)).ceil();
    let btn = 32.0;
    let controls_w = 3.0 * btn + 2.0 * 2.0;
    let x0 = 6.0 + lead + 10.0;
    let fixed = x0 + 10.0 + controls_w + 6.0;
    let w = (fixed + text_w).clamp(fixed + 80.0, MEDIA_TAG_MAX_W);
    let text_room = w - fixed;

    let (inner_r, outer_r) = (12.0, 20.0);
    prims.push(Prim::RoundRect { rect: Rect::new(0.0, 0.0, w, h), radius: outer_r, fill: Some(SURFACE), stroke: Some((HAIRLINE, 1.0)) });
    let patch = if edge == Edge::Right { Rect::new(w / 2.0, 0.0, w / 2.0, h) } else { Rect::new(0.0, 0.0, w / 2.0, h) };
    prims.push(Prim::RoundRect { rect: patch, radius: inner_r, fill: Some(SURFACE), stroke: None });

    let art = Rect::new(6.0, (h - lead) / 2.0, lead, lead);
    match &s.art {
        Some(a) => prims.push(Prim::Image { rect: art, art: a.clone(), radius: 7.0 }),
        None => {
            prims.push(Prim::RoundRect { rect: art, radius: 7.0, fill: Some(Color::rgb(0x3A2E1C)), stroke: None });
            prims.extend(icon_prims(Icon::Note, art.inset(7.0), MEDIA));
        }
    }
    prims.push(Prim::Text {
        rect: Rect::new(x0, 3.0, text_room, 19.0),
        text: title,
        style: title_style,
        color: TEXT,
        align: Align::Start,
        wrap: false,
    });
    prims.push(Prim::Text {
        rect: Rect::new(x0, 20.0, text_room, 16.0),
        text: sub,
        style: sub_style,
        color: TEXT_2,
        align: Align::Start,
        wrap: false,
    });
    // Progress along the bottom edge.
    if let Some(p) = s.progress_at(m.now_ms) {
        let track = Rect::new(x0, h - 3.5, text_room, 2.0);
        prims.push(Prim::RoundRect { rect: track, radius: 1.0, fill: Some(SURFACE_2), stroke: None });
        if p > 0.01 {
            prims.push(Prim::RoundRect {
                rect: Rect::new(track.x, track.y, track.w * p, 2.0),
                radius: 1.0,
                fill: Some(MEDIA),
                stroke: None,
            });
        }
    }
    hits.push(Hit { rect: Rect::new(0.0, 0.0, w, h), kind: HitKind::Click(Action::ToggleSlips), label: "Open Ngambang".into() });
    let mut bx = w - 6.0 - controls_w;
    let (play_icon, play_label) = if s.playing { (Icon::Pause, "Pause") } else { (Icon::Play, "Play") };
    for (icon, label, action, on) in [
        (Icon::Prev, "Previous track", Action::MediaPrev, s.can_prev),
        (play_icon, play_label, Action::MediaPlayPause, s.can_play_pause),
        (Icon::Next, "Next track", Action::MediaNext, s.can_next),
    ] {
        let r = Rect::new(bx, (h - btn) / 2.0, btn, btn);
        let main = action == Action::MediaPlayPause;
        if main {
            prims.extend(soft::raised_circle(r.x + btn / 2.0, r.y + btn / 2.0, 13.0, if on { MEDIA } else { TRACK }, 1.5));
        }
        let fg = if main {
            SURFACE
        } else if on {
            TEXT
        } else {
            TEXT.with_alpha(0x55)
        };
        let size = if main { 12.0 } else { 14.0 };
        prims.extend(icon_prims(icon, Rect::new(r.x + (btn - size) / 2.0, r.y + (btn - size) / 2.0, size, size), fg));
        if on {
            hits.push(Hit { rect: r, kind: HitKind::Click(action), label: label.into() });
        }
        bx += btn + 2.0;
    }
    Scene { kind: SceneKind::Tag, size: Size::new(w, h), flush: false, clip_radius: inner_r, prims, hits, ring: None }
}

fn tag(m: &Model, edge: Edge, tm: &dyn TextMeasure) -> Scene {
    let media_shown = match (m.hover_tag, m.current_tag()) {
        (false, Some(t)) => t.media,
        _ => summary(m).is_some_and(|s| s.icon == Icon::Note),
    };
    if let Some(s) = m.front_media().filter(|_| media_shown) {
        return media_tag(m, s, edge, tm);
    }
    let (icon, color, text, detail, wave, action) = match (m.hover_tag, m.current_tag()) {
        (false, Some(t)) => (t.icon, t.color, t.text.clone(), t.detail.clone(), false, t.action.clone()),
        _ => match summary(m) {
            Some(s) => (s.icon, s.color, s.text, s.detail, s.wave, None),
            None => return orb(m),
        },
    };
    let h = TAG_H;
    let lead = 28.0;
    let mut x = 6.0 + lead + 10.0;
    let mut prims = Vec::new();
    let mut hits = Vec::new();
    let ring = if m.hover_tag { ring_spec(m, Point::new(6.0 + lead / 2.0, h / 2.0)) } else { None };

    let wave_w = if wave { 36.0 } else { 0.0 };
    let text_w = tm.width(&text, TextStyle::BODY_MEDIUM).ceil();
    let detail_w = detail.as_ref().map(|d| tm.width(d, TextStyle::MONO_SMALL).ceil() + 10.0).unwrap_or(0.0);
    let action_w = action.as_ref().map(|(l, _)| tm.width(l, TextStyle::MONO_SMALL).ceil() + 24.0 + 10.0).unwrap_or(0.0);
    let fixed = x + wave_w + if wave { 10.0 } else { 0.0 } + detail_w + action_w + if action.is_some() { 6.0 } else { 16.0 };
    let w = (fixed + text_w).min(TAG_MAX_W);
    let text_room = (w - fixed).max(40.0);

    // Body: the large radius on the free end, a tighter one where it is tied to the edge.
    let (inner_r, outer_r) = (12.0, 20.0);
    prims.push(Prim::RoundRect { rect: Rect::new(0.0, 0.0, w, h), radius: outer_r, fill: Some(SURFACE), stroke: Some((HAIRLINE, 1.0)) });
    let patch = if edge == Edge::Right { Rect::new(w / 2.0, 0.0, w / 2.0, h) } else { Rect::new(0.0, 0.0, w / 2.0, h) };
    prims.push(Prim::RoundRect { rect: patch, radius: inner_r, fill: Some(SURFACE), stroke: None });

    // Lead: the ring (hover) or a ringed icon (event).
    let lc = Point::new(6.0 + lead / 2.0, h / 2.0);
    prims.extend(soft::well_circle(lc.x, lc.y, RING_R + 2.5));
    if ring.is_some() {
        prims.extend(icon_prims(icon, Rect::new(lc.x - 6.0, lc.y - 6.0, 12.0, 12.0), color));
    } else {
        prims.push(Prim::Circle { cx: lc.x, cy: lc.y, r: RING_R, fill: None, stroke: Some((color, RING_W)) });
        prims.extend(icon_prims(icon, Rect::new(lc.x - 6.5, lc.y - 6.5, 13.0, 13.0), color));
    }
    if wave {
        prims.push(Prim::Polyline { points: wave_points(x, h / 2.0, wave_w, 3.0), width: 2.0, color: MEDIA, closed: false });
        x += wave_w + 10.0;
    }
    prims.push(Prim::Text {
        rect: Rect::new(x, 0.0, text_room, h),
        text,
        style: TextStyle::BODY_MEDIUM,
        color: TEXT,
        align: Align::Start,
        wrap: false,
    });
    x += text_room;
    if let Some(d) = detail {
        prims.push(Prim::Text {
            rect: Rect::new(x + 10.0, 0.0, detail_w - 10.0, h),
            text: d,
            style: TextStyle::MONO_SMALL,
            color: if color == PRIVACY { PRIVACY_TEXT } else { TEXT_2 },
            align: Align::Start,
            wrap: false,
        });
        x += detail_w;
    }
    hits.push(Hit { rect: Rect::new(0.0, 0.0, w, h), kind: HitKind::Click(Action::ToggleSlips), label: "Open Ngambang".into() });
    if let Some((label, act)) = action {
        let r = Rect::new(x + 10.0, 6.0, action_w - 10.0, 28.0);
        prims.extend(soft::raised(r, 14.0, SURFACE, 1.5));
        prims.push(Prim::Text {
            rect: r,
            text: label.clone(),
            style: TextStyle::MONO_SMALL,
            color: TEXT,
            align: Align::Center,
            wrap: false,
        });
        hits.push(Hit { rect: r, kind: HitKind::Click(act), label });
    }
    Scene { kind: SceneKind::Tag, size: Size::new(w, h), flush: false, clip_radius: inner_r, prims, hits, ring }
}

/// Small drawing helper for slips.
struct B<'a> {
    prims: Vec<Prim>,
    hits: Vec<Hit>,
    tm: &'a dyn TextMeasure,
    x: f32,
    w: f32,
    /// Content must end above this y: every slip has the same height.
    limit: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Btn {
    Primary(Color),
    Secondary,
    Danger,
    Ghost,
}

impl B<'_> {
    fn text(&mut self, r: Rect, t: impl Into<String>, style: TextStyle, color: Color) {
        self.prims.push(Prim::Text { rect: r, text: t.into(), style, color, align: Align::Start, wrap: false });
    }

    fn text_end(&mut self, r: Rect, t: impl Into<String>, style: TextStyle, color: Color) {
        self.prims.push(Prim::Text { rect: r, text: t.into(), style, color, align: Align::End, wrap: false });
    }

    fn header(&mut self, y: f32, accent: Color, label: &str, right: Option<(String, Color)>) -> f32 {
        self.prims.push(Prim::Circle { cx: self.x + 4.0, cy: y + 8.0, r: 4.0, fill: Some(accent), stroke: None });
        self.text(Rect::new(self.x + 16.0, y, self.w - 16.0, 16.0), label.to_uppercase(), TextStyle::LABEL, LABEL);
        if let Some((r, c)) = right {
            self.text_end(Rect::new(self.x + self.w / 2.0, y, self.w / 2.0, 16.0), r, TextStyle::SMALL, c);
        }
        y + 16.0
    }

    /// A filled shape. The two surface tones mean the soft materials: wells
    /// for `SURFACE_2`, raised faces for `SURFACE_3`.
    fn rect(&mut self, r: Rect, radius: f32, fill: Color) {
        let thin = r.w <= 2.0 || r.h <= 2.0;
        match fill {
            SURFACE_2 if !thin => self.prims.extend(soft::well(r, radius)),
            SURFACE_3 if !thin => self.prims.extend(soft::raised(r, radius, SURFACE, 2.0)),
            SURFACE_3 => self.prims.push(Prim::RoundRect { rect: r, radius, fill: Some(SHADOW_DARK.with_alpha(0x90)), stroke: None }),
            _ => self.prims.push(Prim::RoundRect { rect: r, radius, fill: Some(fill), stroke: None }),
        }
    }

    fn raised(&mut self, r: Rect, radius: f32, face: Color) {
        self.prims.extend(soft::raised(r, radius, face, 3.0));
    }

    fn icon(&mut self, icon: Icon, r: Rect, color: Color) {
        self.prims.extend(icon_prims(icon, r, color));
    }

    fn button(&mut self, r: Rect, label: &str, kind: Btn, action: Action) {
        let (face, fg) = match kind {
            Btn::Primary(c) => (c, SURFACE),
            Btn::Secondary => (SURFACE, TEXT),
            Btn::Danger => (PRIVACY, SURFACE),
            Btn::Ghost => (SURFACE, TEXT_2),
        };
        let radius = if kind == Btn::Ghost { r.h / 2.0 } else { 12.0 };
        self.raised(r, radius, face);
        let style = if matches!(kind, Btn::Primary(_) | Btn::Danger) {
            TextStyle { weight: Weight::Semibold, ..TextStyle::BODY }
        } else {
            TextStyle::BODY
        };
        self.prims.push(Prim::Text { rect: r, text: label.into(), style, color: fg, align: Align::Center, wrap: false });
        self.hits.push(Hit { rect: r, kind: HitKind::Click(action), label: label.into() });
    }

    fn icon_button(&mut self, r: Rect, icon: Icon, fg: Color, bg: Option<Color>, action: Option<Action>, label: &str) {
        if let Some(bg) = bg {
            let face = if bg == SURFACE_3 { SURFACE } else { bg };
            self.prims.extend(soft::raised(r, 12.0, face, 2.5));
        }
        let c = Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
        let fg = if action.is_none() { fg.with_alpha(0x55) } else { fg };
        self.icon(icon, Rect::new(c.x - 8.0, c.y - 8.0, 16.0, 16.0), fg);
        if let Some(a) = action {
            self.hits.push(Hit { rect: r, kind: HitKind::Click(a), label: label.into() });
        }
    }

    /// A row of buttons that share the width.
    fn button_row(&mut self, y: f32, buttons: Vec<(String, Btn, Action)>) -> f32 {
        let n = buttons.len().max(1) as f32;
        let gap = 8.0;
        let bw = (self.w - gap * (n - 1.0)) / n;
        for (i, (label, kind, action)) in buttons.into_iter().enumerate() {
            self.button(Rect::new(self.x + i as f32 * (bw + gap), y, bw, 44.0), &label, kind, action);
        }
        y + 44.0
    }

    /// Pill chips that wrap to new lines.
    fn chips(&mut self, y: f32, chips: Vec<(String, Color, Action)>) -> f32 {
        let (mut cx, mut cy) = (self.x, y);
        for (label, color, action) in chips {
            let w = self.tm.width(&label, TextStyle::BODY).ceil() + 28.0;
            if cx + w > self.x + self.w && cx > self.x {
                cx = self.x;
                cy += 50.0;
            }
            let r = Rect::new(cx, cy, w.min(self.w), 40.0);
            if color == TEXT_2 {
                self.prims.extend(soft::raised(r, 20.0, SURFACE, 2.5));
            } else {
                self.prims.extend(soft::well(r, 20.0));
            }
            self.prims.push(Prim::Text { rect: r, text: label.clone(), style: TextStyle::BODY, color, align: Align::Center, wrap: false });
            self.hits.push(Hit { rect: r, kind: HitKind::Click(action), label });
            cx += w + 10.0;
        }
        cy + 40.0
    }

    fn slider(&mut self, r: Rect, value: f32, color: Color, target: SliderTarget, label: &str) {
        let cy = r.y + r.h / 2.0;
        let v = value.clamp(0.0, 1.0);
        self.prims.extend(soft::well(Rect::new(r.x - 2.0, cy - 4.0, r.w + 4.0, 8.0), 4.0));
        if v > 0.01 {
            self.prims.push(Prim::RoundRect { rect: Rect::new(r.x, cy - 1.5, r.w * v, 3.0), radius: 1.5, fill: Some(color), stroke: None });
        }
        let kx = (r.x + r.w * v).clamp(r.x + 4.0, r.right() - 4.0);
        self.prims.extend(soft::raised_circle(kx, cy, 8.0, SURFACE, 2.0));
        self.prims.push(Prim::Circle { cx: kx, cy, r: 2.5, fill: Some(color), stroke: None });
        self.hits.push(Hit { rect: r, kind: HitKind::Slider(target), label: label.into() });
    }

    fn meter(&mut self, r: Rect, value: f32, color: Color) {
        self.prims.push(Prim::RoundRect { rect: r, radius: r.h / 2.0, fill: Some(SURFACE_2), stroke: None });
        if value > 0.005 {
            self.rect(Rect::new(r.x, r.y, r.w * value.clamp(0.0, 1.0), r.h), r.h / 2.0, color);
        }
    }

    /// How many rows of `row_h` fit from `y`, keeping `reserve` free below them.
    fn rows(&self, y: f32, row_h: f32, reserve: f32) -> usize {
        ((self.limit - reserve - y) / row_h).floor().max(0.0) as usize
    }

    fn empty(&mut self, y: f32, text: &str) -> f32 {
        self.text(Rect::new(self.x, y, self.w, 22.0), text, TextStyle::BODY, TEXT_2);
        y + 22.0
    }
}

fn module_icon(m: ModuleId) -> (Icon, Color) {
    match m {
        ModuleId::Media => (Icon::Note, MEDIA),
        ModuleId::Mixer => (Icon::Speaker, TEXT),
        ModuleId::Commands => (Icon::Terminal, CODER),
        ModuleId::Ports => (Icon::Plug, PORTS),
        ModuleId::Clipboard => (Icon::Clipboard, CODER),
        ModuleId::Timer => (Icon::Clock, GAME),
        ModuleId::Game => (Icon::Game, GAME),
        ModuleId::Captures => (Icon::Image, PORTS),
        ModuleId::Privacy => (Icon::Mic, PRIVACY),
        ModuleId::Prayer => (Icon::Moon, OK),
        ModuleId::Weather => (Icon::Cloud, PORTS),
        ModuleId::Todo => (Icon::Todo, OK),
        ModuleId::Calendar => (Icon::Calendar, CODER),
        ModuleId::Finance => (Icon::Chart, GAME),
    }
}

fn slips(m: &Model, front: ModuleId, tm: &dyn TextMeasure) -> Scene {
    let tabs = m.available_slips();
    // The tabs always run in a rail along the bottom of the slip, on every
    // edge: one shape, the same place to look, and it never hangs past the body.
    let pad = 18.0;
    let mut b = B { prims: Vec::new(), hits: Vec::new(), tm, x: pad, w: SLIP_W - 2.0 * pad, limit: SLIP_H - pad };

    // Content first (into a separate list) so we know the height.
    let body_start = b.prims.len();
    let bottom = match front {
        ModuleId::Media => media_slip(m, &mut b, pad),
        ModuleId::Mixer => mixer_slip(m, &mut b, pad),
        ModuleId::Commands => commands_slip(m, &mut b, pad),
        ModuleId::Ports => ports_slip(m, &mut b, pad),
        ModuleId::Clipboard => clipboard_slip(m, &mut b, pad),
        ModuleId::Timer => timer_slip(m, &mut b, pad),
        ModuleId::Game => game_slip(m, &mut b, pad),
        ModuleId::Captures => captures_slip(m, &mut b, pad),
        ModuleId::Privacy => privacy_slip(m, &mut b, pad),
        ModuleId::Prayer => prayer_slip(m, &mut b, pad),
        ModuleId::Weather => weather_slip(m, &mut b, pad),
        ModuleId::Todo => todo_slip(m, &mut b, pad),
        ModuleId::Calendar => calendar_slip(m, &mut b, pad),
        ModuleId::Finance => finance_slip(m, &mut b, pad),
    };
    let content: Vec<Prim> = b.prims.drain(body_start..).collect();
    // Every module has the same height, so flipping between them keeps the
    // island under the pointer; lists show as many rows as fit.
    let _ = bottom;
    let slip_h = SLIP_H;
    let n = tabs.len().max(1);

    // One row of tabs (up to fourteen).
    let mut rects = Vec::with_capacity(n);
    let rows = n.div_ceil(14);
    let per_row = n.div_ceil(rows);
    let gap = 4.0;
    let tab_w = ((SLIP_W - 28.0 - gap * (per_row as f32 - 1.0)) / per_row as f32).min(48.0).floor();
    for row in 0..rows {
        let count = per_row.min(n - row * per_row);
        let row_w = count as f32 * tab_w + gap * (count as f32 - 1.0);
        let x0 = ((SLIP_W - row_w) / 2.0).floor();
        for i in 0..count {
            rects.push(Rect::new(x0 + i as f32 * (tab_w + gap), slip_h + 4.0 + row as f32 * TAB_ROW_H, tab_w, TAB_ROW_H - 8.0));
        }
    }
    let size = Size::new(SLIP_W, slip_h + rows as f32 * TAB_ROW_H + 8.0);

    // One soft surface for the slip and its tabs, with a groove between them.
    b.prims.push(Prim::RoundRect {
        rect: Rect::new(0.0, 0.0, size.w, size.h),
        radius: 16.0,
        fill: Some(SURFACE),
        stroke: Some((HAIRLINE, 1.0)),
    });
    let groove = Rect::new(18.0, slip_h - 1.0, SLIP_W - 36.0, 2.0);
    b.prims.push(Prim::RoundRect { rect: groove, radius: 1.0, fill: Some(SHADOW_DARK.with_alpha(0x90)), stroke: None });
    for (t, r) in tabs.iter().zip(rects) {
        let active = *t == front;
        let (icon, color) = module_icon(*t);
        // The open slip's tab is pressed in; the others stand out.
        if active {
            b.prims.extend(soft::well(r, 10.0));
        } else {
            b.prims.extend(soft::raised(r, 10.0, SURFACE, 2.5));
        }
        b.icon(
            icon,
            Rect::new(r.x + (r.w - 16.0) / 2.0, r.y + (r.h - 16.0) / 2.0, 16.0, 16.0),
            if active { color } else { color.with_alpha(0xA0) },
        );
        b.hits.push(Hit { rect: r, kind: HitKind::Click(Action::SelectSlip(*t)), label: t.label().into() });
    }
    b.prims.extend(content);
    // The volume level, while it is being changed.
    if m.volume_flashing() {
        let (who, v) = m.flashed_volume();
        let level = if v.muted { "muted".to_string() } else { format!("{:.0}%", v.level * 100.0) };
        let text = format!("{} {level}", format::ellipsize(&who, 12));
        let pill = Rect::new(SLIP_W / 2.0 - 78.0, 8.0, 156.0, 30.0);
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

fn media_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    let now = m.now_ms;
    let Some(s) = m.front_media() else {
        let y = b.header(top, MEDIA, "Media", None);
        return b.empty(y + 14.0, "Nothing is playing");
    };
    let label = if m.media.len() > 1 { format!("Media · {} sources", m.media.len()) } else { format!("Media · {}", s.app) };
    let mut y = b.header(top, MEDIA, &label, None) + 12.0;
    if m.media.len() > 1 {
        let chips: Vec<_> = m
            .media
            .iter()
            .take(4)
            .enumerate()
            .map(|(i, x)| {
                let name = if x.playing { x.app.clone() } else { format!("{} · paused", x.app) };
                (format::ellipsize(&name, 18), if x.id == s.id { MEDIA } else { TEXT_2 }, Action::MediaSource(i))
            })
            .collect();
        y = b.chips(y, chips) + 14.0;
    }
    match &s.art {
        Some(a) => b.prims.push(Prim::Image { rect: Rect::new(b.x, y, 60.0, 60.0), art: a.clone(), radius: 8.0 }),
        None => {
            b.rect(Rect::new(b.x, y, 60.0, 60.0), 8.0, Color::rgb(0x3A2E1C));
            b.icon(Icon::Note, Rect::new(b.x + 19.0, y + 19.0, 22.0, 22.0), MEDIA);
        }
    }
    let tx = b.x + 74.0;
    let tw = b.w - 74.0;
    b.text(Rect::new(tx, y + 8.0, tw, 24.0), if s.title.is_empty() { s.app.clone() } else { s.title.clone() }, TextStyle::TITLE, TEXT);
    let artist = if s.artist.is_empty() { s.app.clone() } else { s.artist.clone() };
    b.text(Rect::new(tx, y + 32.0, tw, 20.0), artist, TextStyle::SMALL, TEXT_2);
    y += 74.0;

    let pos = s.position_at(now);
    if s.duration_ms > 0 {
        let frac = s.progress_at(now).unwrap_or(0.0);
        let played = (b.w * frac).max(0.0);
        let cy = y + 8.0;
        if played > 2.0 {
            let pts = if s.playing { wave_points(b.x, cy, played, 3.5) } else { vec![Point::new(b.x, cy), Point::new(b.x + played, cy)] };
            b.prims.push(Prim::Polyline { points: pts, width: 2.0, color: MEDIA, closed: false });
        }
        b.prims.push(Prim::Circle { cx: b.x + played + 3.0, cy, r: 3.0, fill: Some(TEXT), stroke: None });
        if played + 8.0 < b.w {
            b.rect(Rect::new(b.x + played + 8.0, cy - 1.0, b.w - played - 8.0, 2.0), 1.0, TRACK);
        }
        if s.can_seek {
            b.hits.push(Hit {
                rect: Rect::new(b.x, y - 6.0, b.w, 28.0),
                kind: HitKind::Slider(SliderTarget::MediaSeek),
                label: "Seek".into(),
            });
        }
        y += 20.0;
        b.text(Rect::new(b.x, y, 80.0, 16.0), format::clock(pos / 1000), TextStyle::LABEL, LABEL);
        b.text_end(Rect::new(b.x + b.w - 80.0, y, 80.0, 16.0), format::clock(s.duration_ms / 1000), TextStyle::LABEL, LABEL);
        y += 24.0;
    } else {
        y += 4.0;
    }
    let cx = b.x + b.w / 2.0;
    let prev = s.can_prev.then_some(Action::MediaPrev);
    let next = s.can_next.then_some(Action::MediaNext);
    b.icon_button(Rect::new(cx - 28.0 - 52.0, y, 44.0, 44.0), Icon::Prev, TEXT, None, prev, "Previous track");
    let (icon, label) = if s.playing { (Icon::Pause, "Pause") } else { (Icon::Play, "Play") };
    b.raised(Rect::new(cx - 28.0, y, 56.0, 44.0), 14.0, if s.can_play_pause { MEDIA } else { TRACK });
    b.icon(icon, Rect::new(cx - 9.0, y + 13.0, 18.0, 18.0), SURFACE);
    if s.can_play_pause {
        b.hits.push(Hit { rect: Rect::new(cx - 28.0, y, 56.0, 44.0), kind: HitKind::Click(Action::MediaPlayPause), label: label.into() });
    }
    b.icon_button(Rect::new(cx + 28.0 + 8.0, y, 44.0, 44.0), Icon::Next, TEXT, None, next, "Next track");
    y + 44.0
}

fn mixer_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    let device = if m.mixer.device.is_empty() { "Default output".to_string() } else { format::ellipsize(&m.mixer.device, 28) };
    let mut y = b.header(top, TEXT, "Mixer", Some((device, TEXT_2))) + 14.0;
    let slider_w = 132.0;
    let name_w = b.w - 32.0 - 10.0 - slider_w - 10.0 - 44.0 - 10.0;
    let row = |b: &mut B,
               y: f32,
               initial: &str,
               name: &str,
               sub: Option<(&str, Color)>,
               vol: Volume,
               peak: Option<f32>,
               target: SliderTarget,
               mute: Action,
               accent: Color| {
        b.rect(Rect::new(b.x, y + 6.0, 32.0, 32.0), 8.0, SURFACE_3);
        if initial.is_empty() {
            b.icon(Icon::Speaker, Rect::new(b.x + 8.0, y + 14.0, 16.0, 16.0), TEXT);
        } else {
            b.prims.push(Prim::Text {
                rect: Rect::new(b.x, y + 6.0, 32.0, 32.0),
                text: initial.into(),
                style: TextStyle { size: 13.0, weight: Weight::Semibold, mono: false },
                color: accent,
                align: Align::Center,
                wrap: false,
            });
        }
        let nx = b.x + 42.0;
        b.text(Rect::new(nx, y + 4.0, name_w, 20.0), name, TextStyle::BODY, TEXT);
        if let Some((s, c)) = sub {
            b.text(Rect::new(nx, y + 24.0, name_w, 14.0), s, TextStyle { size: 10.0, weight: Weight::Regular, mono: true }, c);
        }
        if let Some(p) = peak {
            b.meter(Rect::new(nx, y + 40.0, name_w, 3.0), p, accent);
        }
        let sx = nx + name_w + 10.0;
        b.slider(
            Rect::new(sx, y + 8.0, slider_w, 28.0),
            vol.level,
            if vol.muted { TRACK } else { accent },
            target,
            &format!("{name} volume"),
        );
        let level = if vol.muted { "muted".to_string() } else { format!("{:.0}%", vol.level * 100.0) };
        b.text_end(Rect::new(sx, y + 31.0, slider_w, 13.0), level, TextStyle { size: 10.0, weight: Weight::Medium, mono: true }, TEXT_2);
        let (icon, bg) = if vol.muted { (Icon::SpeakerMuted, Some(PRIVACY)) } else { (Icon::Speaker, Some(SURFACE_3)) };
        let fg = if vol.muted { SURFACE } else { TEXT };
        b.icon_button(Rect::new(sx + slider_w + 10.0, y, 44.0, 44.0), icon, fg, bg, Some(mute), &format!("Mute {name}"));
    };
    row(b, y, "", "System", None, m.mixer.master, None, SliderTarget::Master, Action::MasterMute, MEDIA);
    y += 52.0;
    b.rect(Rect::new(b.x, y, b.w, 1.0), 0.0, SURFACE_3);
    y += 9.0;
    if m.mixer.apps.is_empty() {
        return b.empty(y + 4.0, "No app is playing sound");
    }
    let fit = b.rows(y, 52.0, -8.0);
    for app in m.mixer.apps.iter().take(fit) {
        let initial: String = app.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
        let sub = if app.has_controls { ("controls", MEDIA) } else { ("volume only", LABEL) };
        let accent = if app.has_controls { MEDIA } else { TEXT };
        row(
            b,
            y,
            &initial,
            &format::ellipsize(&app.name, 22),
            Some(sub),
            app.volume,
            Some(app.peak),
            SliderTarget::App(app.pid),
            Action::AppMute(app.pid),
            accent,
        );
        y += 52.0;
    }
    y - 8.0
}

fn commands_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    let now = m.now_ms;
    let mut y = b.header(top, CODER, &format!("Commands · {}", m.jobs.len()), None) + 12.0;
    if m.jobs.is_empty() {
        y = b.empty(y, "Nothing reported yet");
        b.text(Rect::new(b.x, y + 8.0, b.w, 16.0), "ngambang run -- <command>", TextStyle::MONO_SMALL, LABEL);
        return y + 24.0;
    }
    let fit = b.rows(y, 42.0, 52.0);
    for j in m.jobs.iter().take(fit) {
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
        b.icon(icon, Rect::new(b.x, y + 12.0, 18.0, 18.0), color);
        let failed_open = matches!(j.status, JobStatus::Done { ok: false, .. }) && j.hwnd.is_some();
        let right_w = if failed_open { 120.0 } else { 70.0 };
        b.text(Rect::new(b.x + 28.0, y, b.w - 28.0 - right_w, 42.0), j.title.clone(), TextStyle::MONO, TEXT);
        let dc = if color == PRIVACY { PRIVACY_TEXT } else { TEXT_2 };
        if failed_open {
            b.text_end(Rect::new(b.x + b.w - right_w, y, 50.0, 42.0), detail, TextStyle::MONO_SMALL, dc);
            let r = Rect::new(b.x + b.w - 62.0, y + 6.0, 62.0, 30.0);
            b.rect(r, 15.0, SURFACE_3);
            b.prims.push(Prim::Text {
                rect: r,
                text: "Open".into(),
                style: TextStyle::MONO_SMALL,
                color: TEXT,
                align: Align::Center,
                wrap: false,
            });
            b.hits.push(Hit {
                rect: r.inset(-7.0),
                kind: HitKind::Click(Action::JobOpenTerminal(j.id.clone())),
                label: format!("Open terminal for {}", j.title),
            });
        } else {
            b.text_end(Rect::new(b.x + b.w - right_w, y, right_w, 42.0), detail, TextStyle::MONO_SMALL, dc);
        }
        y += 42.0;
    }
    if m.jobs.iter().any(|j| !matches!(j.status, JobStatus::Running { .. })) {
        y += 8.0;
        y = b.button_row(y, vec![("Clear finished".into(), Btn::Secondary, Action::JobsClear)]);
    }
    y
}

fn ports_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    let now = m.now_ms;
    let mut y = b.header(top, PORTS, "Listening · local", Some((m.ports.len().to_string(), LABEL))) + 10.0;
    if m.ports.is_empty() {
        return b.empty(y + 4.0, "No local servers listening");
    }
    let fit = b.rows(y, 50.0, 0.0);
    for p in m.ports.iter().take(fit) {
        b.text(Rect::new(b.x, y, 70.0, 48.0), format!(":{}", p.port), TextStyle { size: 15.0, weight: Weight::Regular, mono: true }, PORTS);
        let name_w = b.w - 70.0 - 44.0 - 6.0 - 64.0;
        b.text(Rect::new(b.x + 70.0, y, name_w, 48.0), format!("{} · {}", p.process, p.pid), TextStyle::BODY, TEXT);
        let ox = b.x + b.w - 64.0 - 6.0 - 44.0;
        b.icon_button(
            Rect::new(ox, y + 2.0, 44.0, 44.0),
            Icon::Open,
            TEXT,
            Some(SURFACE_3),
            Some(Action::PortOpen(p.port)),
            &format!("Open localhost:{}", p.port),
        );
        let sr = Rect::new(b.x + b.w - 64.0, y + 2.0, 64.0, 44.0);
        let confirming = m.port_confirm.get(&p.port).is_some_and(|u| *u > now);
        if !p.can_stop {
            b.rect(sr, 10.0, SURFACE_2);
            b.prims.push(Prim::Text {
                rect: sr,
                text: "elevated".into(),
                style: TextStyle { size: 11.0, weight: Weight::Regular, mono: true },
                color: LABEL,
                align: Align::Center,
                wrap: false,
            });
        } else if confirming {
            b.button(sr, "Confirm", Btn::Danger, Action::PortStop(p.port));
        } else {
            b.rect(sr, 10.0, SURFACE_3);
            b.icon(Icon::Stop, Rect::new(sr.x + 24.0, sr.y + 14.0, 16.0, 16.0), PRIVACY_TEXT);
            b.hits.push(Hit {
                rect: sr,
                kind: HitKind::Click(Action::PortStop(p.port)),
                label: format!("Stop {} on port {}", p.process, p.port),
            });
        }
        y += 50.0;
    }
    y
}

fn clipboard_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    let Some(c) = &m.clip else {
        let y = b.header(top, CODER, "Clipboard", None);
        return b.empty(y + 12.0, "Copy a JWT, JSON, UUID, timestamp, colour or link");
    };
    let status = clipboard::status(&c.kind, c.unix_at).map(|s| {
        let col = if s.starts_with("expired") { PRIVACY_TEXT } else { TEXT_2 };
        (s, col)
    });
    let mut y = b.header(top, CODER, &format!("Detected · {}", c.kind.label()), status) + 12.0;
    let preview = clipboard::preview(&c.text, &c.kind, c.unix_at);
    let lines: Vec<&str> = preview.lines().take(7).collect();
    let shown = format::ellipsize(&lines.join("\n"), 360);
    let n = shown.lines().count().max(1) as f32;
    let box_h = (n * 18.0 + 24.0).min(150.0);
    b.rect(Rect::new(b.x, y, b.w, box_h), 8.0, SURFACE_2);
    let mut text_x = b.x + 12.0;
    if let ClipKind::HexColor { r, g, b: bl, a } = c.kind {
        let col = Color(((r as u32) << 24) | ((g as u32) << 16) | ((bl as u32) << 8) | a as u32);
        b.prims.push(Prim::RoundRect {
            rect: Rect::new(b.x + 12.0, y + 12.0, 24.0, 24.0),
            radius: 6.0,
            fill: Some(col),
            stroke: Some((HAIRLINE, 1.0)),
        });
        text_x += 34.0;
    }
    b.prims.push(Prim::Text {
        rect: Rect::new(text_x, y + 12.0, b.x + b.w - 12.0 - text_x, box_h - 24.0),
        text: shown,
        style: TextStyle { size: 12.5, weight: Weight::Regular, mono: true },
        color: TEXT,
        align: Align::Start,
        wrap: true,
    });
    y += box_h + 14.0;
    let chips = c.kind.actions().iter().map(|a| (a.label().to_string(), CODER, Action::Clip(*a))).collect();
    b.chips(y, chips)
}

fn timer_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    let now = m.now_ms;
    let state = match m.timer {
        FocusTimer::Idle => "ready",
        FocusTimer::Running { .. } => "running",
        FocusTimer::Paused { .. } => "paused",
    };
    let mut y = b.header(top, GAME, &format!("Focus · {state}"), None) + 12.0;
    match m.timer {
        FocusTimer::Idle => {
            b.text(Rect::new(b.x, y, b.w, 52.0), "Start a round", TextStyle { size: 22.0, weight: Weight::Semibold, mono: false }, TEXT);
            y += 60.0;
            let buttons =
                m.config.timer.presets_min.iter().map(|p| (format!("{p} min"), Btn::Primary(GAME), Action::TimerStart(*p))).collect();
            y = b.button_row(y, buttons) + 10.0;
            // Custom length: − 15 min + and its own start button.
            let custom = m.config.timer.custom_min;
            b.icon_button(Rect::new(b.x, y, 44.0, 44.0), Icon::Minus, TEXT, Some(SURFACE_3), Some(Action::TimerCustom(-1)), "Shorter");
            b.prims.push(Prim::Text {
                rect: Rect::new(b.x + 50.0, y, 96.0, 44.0),
                text: format!("{custom} min"),
                style: TextStyle { size: 17.0, weight: Weight::Medium, mono: true },
                color: TEXT,
                align: Align::Center,
                wrap: false,
            });
            b.icon_button(Rect::new(b.x + 152.0, y, 44.0, 44.0), Icon::Plus, TEXT, Some(SURFACE_3), Some(Action::TimerCustom(1)), "Longer");
            let sx = b.x + 204.0;
            b.button(Rect::new(sx, y, b.x + b.w - sx, 44.0), "Start custom", Btn::Ghost, Action::TimerStart(custom));
            y + 44.0
        }
        _ => {
            let rem = m.timer.remaining(now).unwrap_or(0);
            b.text(
                Rect::new(b.x, y, b.w, 52.0),
                format::clock(rem.div_ceil(1000)),
                TextStyle { size: 44.0, weight: Weight::Medium, mono: true },
                GAME,
            );
            y += 60.0;
            let frac = m.timer.progress(now).unwrap_or(0.0);
            b.meter(Rect::new(b.x, y, b.w, 4.0), frac, GAME);
            y += 16.0;
            let main = if m.timer.is_running() {
                ("Pause".to_string(), Btn::Primary(GAME), Action::TimerPause)
            } else {
                ("Resume".to_string(), Btn::Primary(GAME), Action::TimerResume)
            };
            b.button_row(
                y,
                vec![main, ("+5 min".into(), Btn::Secondary, Action::TimerAdd(5)), ("Stop".into(), Btn::Secondary, Action::TimerStop)],
            )
        }
    }
}

fn game_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    let now = m.now_ms;
    let today = format!(
        "today {}",
        format::hours_minutes(m.playlog.total() + m.game.as_ref().map(|g| now.saturating_sub(g.started_ms) / 60_000).unwrap_or(0))
    );
    let label = match &m.game {
        Some(g) => format!("Game · {}", g.exe),
        None => "Games".into(),
    };
    let mut y = b.header(top, GAME, &label, Some((today, TEXT_2))) + 12.0;
    match &m.game {
        Some(g) => {
            b.text(Rect::new(b.x, y, 200.0, 50.0), format::clock(now.saturating_sub(g.started_ms) / 1000), TextStyle::BIG, GAME);
            b.text(Rect::new(b.x + 200.0, y + 14.0, b.w - 200.0, 30.0), "this session", TextStyle::BODY, TEXT_2);
            y += 60.0;
            let tiles = [
                ("GPU", m.stats.gpu_pct.map(|v| format!("{v:.0}%"))),
                ("VRAM", m.stats.vram_mb.map(|v| if v >= 1024 { format!("{:.1}G", v as f32 / 1024.0) } else { format!("{v}M") })),
                ("TEMP", m.stats.temp_c.map(|v| format!("{v}°"))),
                ("PING", m.stats.ping_ms.map(|v| format!("{v}ms"))),
            ];
            let tw = (b.w - 3.0 * 8.0) / 4.0;
            for (i, (k, v)) in tiles.iter().enumerate() {
                let r = Rect::new(b.x + i as f32 * (tw + 8.0), y, tw, 56.0);
                b.rect(r, 8.0, SURFACE_2);
                b.text(
                    Rect::new(r.x + 10.0, r.y + 8.0, r.w - 12.0, 14.0),
                    *k,
                    TextStyle { size: 10.0, weight: Weight::Regular, mono: true },
                    LABEL,
                );
                b.text(
                    Rect::new(r.x + 10.0, r.y + 24.0, r.w - 12.0, 24.0),
                    v.clone().unwrap_or_else(|| "—".into()),
                    TextStyle { size: 17.0, weight: Weight::Regular, mono: true },
                    TEXT,
                );
            }
            y += 70.0;
            if let Some(c) = m.captures.first() {
                b.rect(Rect::new(b.x, y, b.w, 56.0), 10.0, SURFACE_2);
                b.icon(
                    if c.kind == CaptureKind::Clip { Icon::Play } else { Icon::Image },
                    Rect::new(b.x + 14.0, y + 18.0, 20.0, 20.0),
                    TEXT,
                );
                let what = if c.kind == CaptureKind::Clip { "Clip saved" } else { "Screenshot saved" };
                b.text(Rect::new(b.x + 46.0, y + 8.0, b.w - 46.0 - 80.0, 20.0), what, TextStyle::BODY, TEXT);
                b.text(
                    Rect::new(b.x + 46.0, y + 28.0, b.w - 46.0 - 80.0, 18.0),
                    format::relative(-((now.saturating_sub(c.at_ms) / 1000) as i64)),
                    TextStyle::MONO_SMALL,
                    LABEL,
                );
                b.button(Rect::new(b.x + b.w - 74.0, y + 6.0, 66.0, 44.0), "Copy", Btn::Secondary, Action::CaptureCopy(0));
                y += 68.0;
            }
            if m.config.game.break_after_min > 0 {
                let left = m.break_due_ms.map(|d| format::clock(d.saturating_sub(now).div_ceil(1000))).unwrap_or_else(|| "—".into());
                b.text(Rect::new(b.x, y, b.w, 20.0), format!("Next break reminder in {left}"), TextStyle::SMALL, TEXT_2);
                y += 28.0;
                y = b.button_row(y, vec![("Snooze break 15 min".into(), Btn::Primary(GAME), Action::BreakSnooze)]);
            } else {
                y -= 12.0;
            }
            y
        }
        None => {
            let mut games: Vec<(&String, &u64)> = m.playlog.minutes.iter().collect();
            games.sort_by(|a, b| b.1.cmp(a.1));
            for (exe, mins) in games.into_iter().take(4) {
                b.text(Rect::new(b.x, y, b.w - 90.0, 32.0), exe.clone(), TextStyle::MONO, TEXT);
                b.text_end(Rect::new(b.x + b.w - 90.0, y, 90.0, 32.0), format::hours_minutes(*mins), TextStyle::MONO_SMALL, TEXT_2);
                y += 32.0;
            }
            y
        }
    }
}

fn captures_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    let now = m.now_ms;
    let mut y = b.header(top, PORTS, "Captures", None) + 12.0;
    for (i, c) in m.captures.iter().take(4).enumerate() {
        let tile = Rect::new(b.x, y, 64.0, 44.0);
        b.rect(tile, 6.0, SURFACE_2);
        b.icon(
            if c.kind == CaptureKind::Clip { Icon::Play } else { Icon::Image },
            Rect::new(tile.x + 22.0, tile.y + 12.0, 20.0, 20.0),
            TEXT,
        );
        b.hits.push(Hit { rect: tile, kind: HitKind::Click(Action::CaptureOpen(i)), label: format!("Open {}", c.name) });
        let tw = b.w - 64.0 - 12.0 - 2.0 * 50.0;
        b.text(Rect::new(b.x + 76.0, y + 2.0, tw, 20.0), c.name.clone(), TextStyle::BODY, TEXT);
        b.text(
            Rect::new(b.x + 76.0, y + 24.0, tw, 16.0),
            format::relative(-((now.saturating_sub(c.at_ms) / 1000) as i64)),
            TextStyle::MONO_SMALL,
            LABEL,
        );
        b.icon_button(
            Rect::new(b.x + b.w - 94.0, y, 44.0, 44.0),
            Icon::Copy,
            TEXT,
            Some(SURFACE_3),
            Some(Action::CaptureCopy(i)),
            &format!("Copy {}", c.name),
        );
        b.icon_button(
            Rect::new(b.x + b.w - 44.0, y, 44.0, 44.0),
            Icon::Folder,
            TEXT,
            Some(SURFACE_3),
            Some(Action::CaptureFolder(i)),
            &format!("Show {} in folder", c.name),
        );
        y += 54.0;
    }
    y - 10.0
}

fn privacy_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    let now = m.now_ms;
    let mut y = b.header(top, PRIVACY, "Mic & camera", None) + 12.0;
    for p in m.privacy.iter().take(5) {
        let (icon, what) = match p.device {
            Device::Microphone => (Icon::Mic, "Microphone"),
            Device::Camera => (Icon::Camera, "Camera"),
        };
        b.icon(icon, Rect::new(b.x, y + 7.0, 18.0, 18.0), PRIVACY);
        b.text(Rect::new(b.x + 28.0, y, b.w - 28.0 - 70.0, 32.0), format!("{what} · {}", p.app), TextStyle::BODY_MEDIUM, TEXT);
        b.text_end(
            Rect::new(b.x + b.w - 70.0, y, 70.0, 32.0),
            format::clock(now.saturating_sub(p.since_ms) / 1000),
            TextStyle::MONO_SMALL,
            TEXT_2,
        );
        y += 34.0;
    }
    y += 8.0;
    let mut buttons = Vec::new();
    if m.privacy.iter().any(|p| p.device == Device::Microphone) {
        buttons.push(("Mute mic".to_string(), Btn::Danger, Action::MicMute));
    }
    let dev = m.privacy.first().map(|p| p.device).unwrap_or(Device::Microphone);
    buttons.push(("Privacy settings".to_string(), Btn::Secondary, Action::PrivacySettings(dev)));
    b.button_row(y, buttons)
}

fn prayer_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    use crate::prayer::{hhmm, Prayer};
    let city = format::ellipsize(&m.config.location.city, 22);
    let mut y = b.header(top, OK, &format!("Jadwal sholat · {city}"), Some((m.config.prayer.method.label().to_string(), TEXT_2))) + 12.0;
    let (Some(day), Some(now)) = (m.prayer_day, m.local_minute()) else {
        return b.empty(y + 4.0, "Waiting for the clock…");
    };
    match day.next(now) {
        Some((p, t)) => {
            b.text(Rect::new(b.x, y, 160.0, 44.0), p.name(), TextStyle { size: 26.0, weight: Weight::Semibold, mono: false }, TEXT);
            let left = ((t - now) * 60.0).max(0.0) as u64;
            b.text_end(
                Rect::new(b.x + 160.0, y, b.w - 160.0, 44.0),
                format!("{} · in {}", hhmm(t), format::clock(left)),
                TextStyle { size: 15.0, weight: Weight::Regular, mono: true },
                OK,
            );
        }
        None => b.text(Rect::new(b.x, y, b.w, 44.0), "Isya has passed — see you at Subuh", TextStyle::BODY, TEXT_2),
    }
    y += 54.0;
    let next = day.next(now).map(|(p, _)| p);
    let cols = 4;
    let gap = 6.0;
    let w = (b.w - gap * (cols as f32 - 1.0)) / cols as f32;
    for (i, p) in Prayer::ALL.iter().enumerate() {
        let r = Rect::new(b.x + (i % cols) as f32 * (w + gap), y + (i / cols) as f32 * (52.0 + gap), w, 52.0);
        let active = Some(*p) == next;
        b.rect(r, 8.0, if active { Color::rgb(0x1F3A30) } else { SURFACE_2 });
        b.text(
            Rect::new(r.x + 10.0, r.y + 6.0, r.w - 12.0, 18.0),
            p.name(),
            TextStyle { size: 11.0, weight: Weight::Medium, mono: true },
            if active { OK } else { LABEL },
        );
        let t = day.get(*p).map(hhmm).unwrap_or_else(|| "—".into());
        b.text(
            Rect::new(r.x + 10.0, r.y + 24.0, r.w - 12.0, 22.0),
            t,
            TextStyle { size: 15.0, weight: Weight::Regular, mono: true },
            if active { OK } else { TEXT },
        );
    }
    y + 2.0 * 52.0 + gap
}

fn todo_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    let open = m.todos.iter().filter(|t| !t.done).count();
    let right = (!m.todos.is_empty()).then(|| (format!("{open} left"), if open > 0 { OK } else { TEXT_2 }));
    let mut y = b.header(top, OK, "To-do", right) + 12.0;
    // The field for a new task sits at the top while it is open.
    if let Some(input) = &m.todo_input {
        let field = Rect::new(b.x, y, b.w, 44.0);
        b.prims.extend(soft::well(field, 12.0));
        let tx = field.x + 14.0;
        let tw = field.w - 28.0;
        if input.is_empty() {
            b.text(Rect::new(tx, y, tw, 44.0), "What needs doing?", TextStyle::BODY, LABEL);
            b.rect(Rect::new(tx, y + 12.0, 2.0, 20.0), 1.0, OK);
        } else {
            // Show the end of long text, with the caret after it.
            let shown: String = {
                let mut t = input.clone();
                while b.tm.width(&t, TextStyle::BODY) > tw - 6.0 && !t.is_empty() {
                    t.remove(0);
                }
                t
            };
            let w = b.tm.width(&shown, TextStyle::BODY).min(tw - 4.0);
            b.text(Rect::new(tx, y, tw, 44.0), shown, TextStyle::BODY, TEXT);
            b.rect(Rect::new(tx + w + 2.0, y + 12.0, 2.0, 20.0), 1.0, OK);
        }
        y += 54.0;
        y = b.button_row(
            y,
            vec![("Add".into(), Btn::Primary(OK), Action::TodoSubmit), ("Cancel".into(), Btn::Secondary, Action::TodoCancel)],
        );
        b.text(Rect::new(b.x, y + 8.0, b.w, 16.0), "Enter adds · Esc cancels", TextStyle::MONO_SMALL, LABEL);
        y += 30.0;
    } else if m.todos.is_empty() {
        y = b.empty(y + 4.0, "Nothing to do. Add a task below.") + 14.0;
    }
    // Open tasks first, then the finished ones.
    let mut list: Vec<&TodoItem> = m.todos.iter().filter(|t| !t.done).collect();
    list.extend(m.todos.iter().filter(|t| t.done));
    let reserve = if m.todo_input.is_some() { 0.0 } else { 52.0 };
    let all = b.rows(y, 42.0, reserve);
    // Room for "+N more" when not everything fits.
    let shown = if list.len() > all { b.rows(y, 42.0, reserve + 22.0) } else { all };
    for t in list.iter().take(shown) {
        let check = Rect::new(b.x - 4.0, y + 2.0, 36.0, 36.0);
        let c = Point::new(check.x + 18.0, check.y + 18.0);
        if t.done {
            b.prims.extend(soft::raised_circle(c.x, c.y, 10.0, OK, 2.0));
            b.icon(Icon::Check, Rect::new(c.x - 6.0, c.y - 6.0, 12.0, 12.0), SURFACE);
        } else {
            b.prims.extend(soft::well_circle(c.x, c.y, 10.0));
        }
        let label = if t.done { format!("Mark \"{}\" as not done", t.text) } else { format!("Mark \"{}\" as done", t.text) };
        b.hits.push(Hit { rect: check, kind: HitKind::Click(Action::TodoToggle(t.id)), label });
        let tx = b.x + 40.0;
        let tw = b.w - 40.0 - 44.0;
        let color = if t.done { LABEL } else { TEXT };
        b.text(Rect::new(tx, y, tw, 40.0), t.text.clone(), TextStyle::BODY, color);
        if t.done {
            let lw = b.tm.width(&t.text, TextStyle::BODY).min(tw);
            b.prims.push(Prim::RoundRect { rect: Rect::new(tx, y + 20.0, lw, 1.0), radius: 0.0, fill: Some(LABEL), stroke: None });
        }
        b.icon_button(
            Rect::new(b.x + b.w - 36.0, y + 2.0, 36.0, 36.0),
            Icon::Trash,
            TEXT_2,
            None,
            Some(Action::TodoRemove(t.id)),
            &format!("Delete \"{}\"", t.text),
        );
        y += 42.0;
    }
    if list.len() > shown {
        b.text(Rect::new(b.x, y, b.w, 20.0), format!("+{} more", list.len() - shown), TextStyle::MONO_SMALL, LABEL);
        y += 22.0;
    }
    if m.todo_input.is_some() {
        return y - 6.0;
    }
    y += 8.0;
    let mut buttons = vec![("Add task".to_string(), Btn::Primary(OK), Action::TodoAdd)];
    if m.todos.iter().any(|t| t.done) {
        buttons.push(("Clear done".to_string(), Btn::Secondary, Action::TodoClearDone));
    }
    b.button_row(y, buttons)
}

/// "#RRGGBB" → colour, or a default per feed.
fn feed_color(m: &Model, feed: usize) -> Color {
    const DEFAULTS: [Color; 6] = [CODER, PORTS, OK, MEDIA, GAME, PRIVACY_TEXT];
    m.config
        .calendar
        .feeds
        .get(feed)
        .and_then(|f| u32::from_str_radix(f.color.trim().trim_start_matches('#'), 16).ok().filter(|_| f.color.trim().len() == 7))
        .map(Color::rgb)
        .unwrap_or(DEFAULTS[feed % DEFAULTS.len()])
}

fn calendar_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    use crate::calendar::{civil_from_days, day_label, weekday};
    let offset = m.clock.map(|c| c.utc_offset_min as i64 * 60).unwrap_or(0);
    let Some(now) = m.unix_now() else {
        let y = b.header(top, CODER, "Calendar", None);
        return b.empty(y + 12.0, "Waiting for the clock…");
    };
    let today = (now + offset).div_euclid(86_400);
    let (_, month, day) = civil_from_days(today);
    let months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let mut y = b.header(top, CODER, &format!("Calendar · {day} {}", months[month as usize - 1]), None) + 12.0;

    // This week, Monday to Sunday: today pressed in, dots for busy days.
    let monday = today - weekday(today) as i64;
    let gap = 6.0;
    let cw = (b.w - gap * 6.0) / 7.0;
    for i in 0..7 {
        let d = monday + i;
        let r = Rect::new(b.x + i as f32 * (cw + gap), y, cw, 54.0);
        let is_today = d == today;
        if is_today {
            b.prims.extend(soft::well(r, 12.0));
        } else {
            b.prims.extend(soft::raised(r, 12.0, SURFACE, 2.0));
        }
        let center = |t: String, yy: f32, h: f32, style: TextStyle, c: Color| Prim::Text {
            rect: Rect::new(r.x, yy, r.w, h),
            text: t,
            style,
            color: c,
            align: Align::Center,
            wrap: false,
        };
        let name = ["M", "T", "W", "T", "F", "S", "S"][i as usize];
        b.prims.push(center(name.into(), r.y + 5.0, 14.0, TextStyle { size: 10.0, weight: Weight::Medium, mono: true }, LABEL));
        let num = civil_from_days(d).2.to_string();
        b.prims.push(center(
            num,
            r.y + 19.0,
            20.0,
            TextStyle { size: 15.0, weight: Weight::Semibold, mono: false },
            if is_today { CODER } else { TEXT },
        ));
        let busy: Vec<usize> = {
            let mut f: Vec<usize> = m.calendar.iter().filter(|e| (e.start + offset).div_euclid(86_400) == d).map(|e| e.feed).collect();
            f.dedup();
            f.truncate(3);
            f
        };
        let dots = busy.len() as f32;
        for (k, feed) in busy.into_iter().enumerate() {
            let cx = r.x + r.w / 2.0 + (k as f32 - (dots - 1.0) / 2.0) * 7.0;
            b.prims.push(Prim::Circle { cx, cy: r.y + 45.0, r: 2.2, fill: Some(feed_color(m, feed)), stroke: None });
        }
    }
    y += 66.0;

    if m.config.calendar.feeds.is_empty() {
        b.text(Rect::new(b.x, y, b.w, 20.0), "No calendar linked yet.", TextStyle::BODY, TEXT_2);
        b.text(Rect::new(b.x, y + 20.0, b.w, 20.0), "Add Google Calendar's secret iCal address in Settings.", TextStyle::SMALL, LABEL);
        y += 50.0;
        return b.button_row(y, vec![("Link a calendar".into(), Btn::Primary(CODER), Action::OpenSettings(ModuleId::Calendar))]);
    }
    // Upcoming, grouped by day.
    let horizon = now + m.config.calendar.days as i64 * 86_400;
    let upcoming: Vec<&crate::calendar::CalEvent> = m.calendar.iter().filter(|e| e.end > now && e.start < horizon).collect();
    if upcoming.is_empty() {
        return b.empty(y, "Nothing coming up");
    }
    let mut last_day = None;
    for e in upcoming {
        let d = (e.start + offset).div_euclid(86_400).max(today);
        let header = if last_day != Some(d) { 22.0 } else { 0.0 };
        if y + header + 46.0 > b.limit {
            break;
        }
        if last_day != Some(d) {
            b.text(Rect::new(b.x, y, b.w, 18.0), day_label(d, today).to_uppercase(), TextStyle::LABEL, LABEL);
            y += 22.0;
            last_day = Some(d);
        }
        let r = Rect::new(b.x, y, b.w, 46.0);
        b.prims.extend(soft::well(r, 12.0));
        b.prims.push(Prim::RoundRect {
            rect: Rect::new(r.x + 8.0, r.y + 9.0, 4.0, r.h - 18.0),
            radius: 2.0,
            fill: Some(feed_color(m, e.feed)),
            stroke: None,
        });
        let when = if e.all_day {
            "All day".to_string()
        } else if e.start <= now {
            "Now".to_string()
        } else {
            let hm = |t: i64| {
                let s = (t + offset).rem_euclid(86_400);
                format!("{:02}:{:02}", s / 3600, s % 3600 / 60)
            };
            format!("{}–{}", hm(e.start), hm(e.end))
        };
        b.text(Rect::new(r.x + 22.0, r.y + 5.0, 92.0, 18.0), when, TextStyle { size: 11.5, weight: Weight::Medium, mono: true }, TEXT_2);
        b.text(Rect::new(r.x + 22.0, r.y + 23.0, r.w - 30.0, 18.0), e.title.clone(), TextStyle::BODY_MEDIUM, TEXT);
        let place = if e.location.is_empty() {
            m.config.calendar.feeds.get(e.feed).map(|f| f.name.clone()).unwrap_or_default()
        } else {
            e.location.clone()
        };
        b.text_end(
            Rect::new(r.x + 118.0, r.y + 5.0, r.w - 126.0, 18.0),
            place,
            TextStyle { size: 11.0, weight: Weight::Regular, mono: false },
            LABEL,
        );
        if e.location.starts_with("https://") {
            b.hits.push(Hit { rect: r, kind: HitKind::Click(Action::OpenUrl(e.location.clone())), label: format!("Join {}", e.title) });
        }
        y += 54.0;
    }
    y - 8.0
}

fn finance_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    let list = &m.config.finance.watchlist;
    let mut y = b.header(top, GAME, "Markets", Some((format!("{} symbols", list.len()), TEXT_2))) + 12.0;
    if list.is_empty() {
        y = b.empty(y, "Your watchlist is empty.") + 10.0;
        return b.button_row(y, vec![("Add symbols".into(), Btn::Primary(GAME), Action::OpenSettings(ModuleId::Finance))]);
    }
    let all = b.rows(y, 62.0, 20.0);
    let fit = if list.len() > all { b.rows(y, 62.0, 42.0) } else { all };
    for item in list.iter().take(fit) {
        let q = m.quotes.iter().find(|q| q.symbol.eq_ignore_ascii_case(&item.symbol));
        let r = Rect::new(b.x, y, b.w, 52.0);
        b.raised(r, 14.0, SURFACE);
        let name = if item.name.is_empty() { q.map(|q| q.name.clone()).unwrap_or_default() } else { item.name.clone() };
        b.text(Rect::new(r.x + 14.0, r.y + 7.0, 130.0, 20.0), name, TextStyle::BODY_MEDIUM, TEXT);
        b.text(
            Rect::new(r.x + 14.0, r.y + 27.0, 130.0, 16.0),
            item.symbol.clone(),
            TextStyle { size: 11.0, weight: Weight::Regular, mono: true },
            LABEL,
        );
        match q {
            Some(q) => {
                let up = q.change_pct >= 0.0;
                let color = if up { OK } else { PRIVACY_TEXT };
                // Sparkline in a groove.
                let spark = Rect::new(r.x + 150.0, r.y + 10.0, 66.0, 32.0);
                b.prims.extend(soft::well(spark, 8.0));
                if q.points.len() >= 2 {
                    let (lo, hi) = q.points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
                    let span = (hi - lo).max(f32::EPSILON);
                    let n = q.points.len() as f32 - 1.0;
                    let inner = spark.inset(6.0);
                    let points = q
                        .points
                        .iter()
                        .enumerate()
                        .map(|(i, v)| Point::new(inner.x + inner.w * i as f32 / n, inner.bottom() - inner.h * (v - lo) / span))
                        .collect();
                    b.prims.push(Prim::Polyline { points, width: 1.5, color, closed: false });
                }
                b.text_end(
                    Rect::new(r.x + 220.0, r.y + 7.0, r.w - 234.0, 20.0),
                    crate::finance::format_price(q),
                    TextStyle { size: 14.0, weight: Weight::Medium, mono: true },
                    TEXT,
                );
                b.text_end(
                    Rect::new(r.x + 220.0, r.y + 27.0, r.w - 234.0, 16.0),
                    crate::finance::format_change(q.change_pct),
                    TextStyle { size: 11.5, weight: Weight::Medium, mono: true },
                    color,
                );
            }
            None => {
                let msg = if m.online == Some(false) { "offline" } else { "loading…" };
                b.text_end(Rect::new(r.x + 150.0, r.y, r.w - 164.0, r.h), msg, TextStyle::MONO_SMALL, LABEL);
            }
        }
        y += 62.0;
    }
    if list.len() > fit {
        b.text(Rect::new(b.x, y, b.w, 18.0), format!("+{} more in Settings", list.len() - fit), TextStyle::MONO_SMALL, LABEL);
        y += 22.0;
    }
    b.text(
        Rect::new(b.x, y, b.w, 16.0),
        "Yahoo Finance · may be delayed",
        TextStyle { size: 10.5, weight: Weight::Regular, mono: true },
        LABEL,
    );
    y + 16.0
}

pub fn condition_icon(c: crate::weather::Condition) -> Icon {
    use crate::weather::Condition::*;
    match c {
        Clear => Icon::Sun,
        PartlyCloudy | Cloudy | Haze | Fog => Icon::Cloud,
        Drizzle | Rain | HeavyRain => Icon::Rain,
        Thunder => Icon::Storm,
    }
}

fn level_color(l: crate::weather::Level) -> Color {
    match l {
        crate::weather::Level::Red => PRIVACY,
        crate::weather::Level::Orange => MEDIA,
        crate::weather::Level::Green => OK,
    }
}

fn weather_slip(m: &Model, b: &mut B, top: f32) -> f32 {
    let unix = m.unix_now();
    let offline = m.online == Some(false);
    let place = format::ellipsize(&m.config.location.city, 24);
    let mut y = b.header(top, PORTS, &format!("Weather · {place}"), None);
    // Refresh: an icon in the header's corner.
    b.icon_button(
        Rect::new(b.x + b.w - 36.0, top - 10.0, 36.0, 36.0),
        Icon::Refresh,
        TEXT_2,
        Some(SURFACE_3),
        Some(Action::RefreshWeather),
        "Refresh weather",
    );
    y += 16.0;
    match &m.weather {
        Some(w) => {
            // Hero: the sky in a well, the temperature, and what it feels like.
            b.prims.extend(soft::well_circle(b.x + 34.0, y + 34.0, 34.0));
            b.icon(condition_icon(w.condition), Rect::new(b.x + 16.0, y + 16.0, 36.0, 36.0), PORTS);
            let tx = b.x + 84.0;
            b.text(
                Rect::new(tx, y + 2.0, 120.0, 44.0),
                format!("{:.0}°", w.temp_c),
                TextStyle { size: 38.0, weight: Weight::Medium, mono: false },
                TEXT,
            );
            b.text(Rect::new(tx, y + 44.0, b.w - 84.0, 22.0), w.description.clone(), TextStyle::BODY_MEDIUM, TEXT);
            if let (Some(lo), Some(hi)) = (w.min_c, w.max_c) {
                b.text_end(
                    Rect::new(b.x + b.w - 120.0, y + 6.0, 120.0, 20.0),
                    format!("↓{lo:.0}°  ↑{hi:.0}°"),
                    TextStyle::MONO_SMALL,
                    TEXT_2,
                );
            }
            if let Some(r) = w.rain_chance {
                b.text_end(
                    Rect::new(b.x + b.w - 120.0, y + 26.0, 120.0, 20.0),
                    format!("rain {r}%"),
                    TextStyle::MONO_SMALL,
                    if r >= 60 { PORTS } else { TEXT_2 },
                );
            }
            y += 80.0;
            // Three small readings in wells.
            let stats: Vec<(String, String)> = [
                w.feels_c.map(|f| ("FEELS".to_string(), format!("{f:.0}°"))),
                w.humidity.map(|h| ("HUMIDITY".to_string(), format!("{h}%"))),
                w.wind_kmh.map(|k| ("WIND".to_string(), format!("{k:.0} km/h"))),
            ]
            .into_iter()
            .flatten()
            .collect();
            if !stats.is_empty() {
                let n = stats.len() as f32;
                let gap = 10.0;
                let sw = (b.w - gap * (n - 1.0)) / n;
                for (i, (k, v)) in stats.into_iter().enumerate() {
                    let r = Rect::new(b.x + i as f32 * (sw + gap), y, sw, 50.0);
                    b.prims.extend(soft::well(r, 12.0));
                    b.text(
                        Rect::new(r.x + 12.0, r.y + 7.0, r.w - 16.0, 14.0),
                        k,
                        TextStyle { size: 10.0, weight: Weight::Medium, mono: true },
                        LABEL,
                    );
                    b.text(
                        Rect::new(r.x + 12.0, r.y + 22.0, r.w - 16.0, 22.0),
                        v,
                        TextStyle { size: 15.0, weight: Weight::Medium, mono: false },
                        TEXT,
                    );
                }
                y += 60.0;
            }
            // The next hours as raised cards.
            if !w.next.is_empty() {
                let n = w.next.len().min(4);
                let gap = 10.0;
                let tw = (b.w - gap * (n as f32 - 1.0)) / n as f32;
                for (i, slot) in w.next.iter().take(n).enumerate() {
                    let r = Rect::new(b.x + i as f32 * (tw + gap), y, tw, 74.0);
                    b.raised(r, 14.0, SURFACE);
                    let center = |t: String, yy: f32, h: f32, style: TextStyle, c: Color| Prim::Text {
                        rect: Rect::new(r.x, yy, r.w, h),
                        text: t,
                        style,
                        color: c,
                        align: Align::Center,
                        wrap: false,
                    };
                    b.prims.push(center(
                        slot.time.clone(),
                        r.y + 6.0,
                        16.0,
                        TextStyle { size: 10.5, weight: Weight::Regular, mono: true },
                        LABEL,
                    ));
                    b.icon(condition_icon(slot.condition), Rect::new(r.x + r.w / 2.0 - 10.0, r.y + 25.0, 20.0, 20.0), TEXT_2);
                    b.prims.push(center(
                        format!("{:.0}°", slot.temp_c),
                        r.y + 48.0,
                        20.0,
                        TextStyle { size: 15.0, weight: Weight::Medium, mono: false },
                        TEXT,
                    ));
                }
                y += 86.0;
            }
        }
        None => {
            let msg = if offline { "No connection. Weather loads once you're online." } else { "Loading weather…" };
            y = b.empty(y, msg) + 14.0;
        }
    }
    // Early warnings.
    let alerts = m.relevant_alerts(3);
    let count = (!alerts.is_empty()).then(|| (alerts.len().to_string(), MEDIA));
    y = b.header(y, if alerts.is_empty() { OK } else { MEDIA }, "Early warnings", count) + 10.0;
    if alerts.is_empty() {
        let msg = if offline && m.alerts.is_empty() { "Unavailable while offline".to_string() } else { format!("All clear near {place}") };
        b.text(Rect::new(b.x, y, b.w, 20.0), msg, TextStyle::SMALL, TEXT_2);
        y += 26.0;
    }
    let l = &m.config.location;
    let fit = b.rows(y, 56.0, 20.0);
    for a in alerts.into_iter().take(fit) {
        let r = Rect::new(b.x, y, b.w, 48.0);
        b.prims.extend(soft::well(r, 12.0));
        b.prims.push(Prim::RoundRect {
            rect: Rect::new(r.x + 8.0, r.y + 10.0, 4.0, r.h - 20.0),
            radius: 2.0,
            fill: Some(level_color(a.level)),
            stroke: None,
        });
        let km = crate::weather::distance_km(a.lat, a.lon, l.latitude, l.longitude);
        let ago = unix.map(|u| format::relative(a.time - u)).unwrap_or_default();
        let title = if a.tsunami { format!("Tsunami · {}", a.title) } else { a.title.clone() };
        b.text(Rect::new(r.x + 22.0, r.y + 6.0, r.w - 30.0, 20.0), title, TextStyle::SMALL, TEXT);
        b.text(
            Rect::new(r.x + 22.0, r.y + 25.0, r.w - 30.0, 16.0),
            format!("{km:.0} km · {ago} · {}", a.source),
            TextStyle { size: 11.0, weight: Weight::Regular, mono: true },
            LABEL,
        );
        b.hits.push(Hit { rect: r, kind: HitKind::Click(Action::OpenUrl(a.url.clone())), label: format!("Open report: {}", a.title) });
        y += 56.0;
    }
    // Where the data comes from and how fresh it is.
    let age = m.weather.as_ref().and_then(|w| unix.map(|u| u - w.fetched));
    let fresh = match age {
        _ if offline => "offline".to_string(),
        Some(a) if a < 90 => "updated just now".to_string(),
        Some(a) => format!("updated {}", format::relative(-a)),
        None => String::new(),
    };
    let line = ["Open-Meteo · GDACS · USGS".to_string(), fresh].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    b.text(
        Rect::new(b.x, y + 2.0, b.w, 16.0),
        line,
        TextStyle { size: 10.5, weight: Weight::Regular, mono: true },
        if offline { MEDIA } else { LABEL },
    );
    y + 18.0
}

/// Points for a static ring arc (used by tests and fallback renderers).
pub fn ring_points(r: &RingSpec) -> Vec<Point> {
    arc_points(r.center.x, r.center.y, r.radius, -90.0, -90.0 + 360.0 * r.value, 48)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::protocol::Request;

    fn all_scenes() -> Vec<(String, Scene)> {
        let mut out = Vec::new();
        let mut m = Model::new(Config::default(), 3, 1_000_000);
        let now = 1_000_000;
        m.update(
            now,
            Event::Media(vec![
                MediaSession {
                    id: "Spotify.exe".into(),
                    app: "Spotify".into(),
                    title: "A very long track title that will not fit".into(),
                    artist: "Artist".into(),
                    playing: true,
                    position_ms: 40_000,
                    duration_ms: 200_000,
                    at_ms: now,
                    can_play_pause: true,
                    can_next: true,
                    can_prev: true,
                    can_seek: true,
                    art: None,
                },
                MediaSession { id: "Chrome".into(), app: "Chrome".into(), title: "Video".into(), playing: false, ..Default::default() },
            ]),
        );
        m.update(
            now,
            Event::Mixer(MixerSnapshot {
                device: "Headphones".into(),
                master: Volume { level: 0.6, muted: false },
                apps: (0..8)
                    .map(|i| AudioApp {
                        pid: i + 10,
                        name: format!("App {i}"),
                        exe: format!("app{i}.exe"),
                        volume: Volume { level: 0.5, muted: i == 2 },
                        peak: 0.3,
                        ..Default::default()
                    })
                    .collect(),
            }),
        );
        for i in 0..7 {
            m.update(
                now,
                Event::Pipe(Request::Done {
                    title: None,
                    id: format!("job {i}"),
                    ok: i % 2 == 0,
                    code: Some(1),
                    duration_s: Some(12.0),
                    hwnd: Some("0x10".into()),
                }),
            );
        }
        m.update(
            now,
            Event::Pipe(Request::Notify {
                id: None,
                title: "deploy".into(),
                body: None,
                level: crate::protocol::Level::Info,
                progress: Some(0.4),
                sticky: true,
                ttl_ms: None,
                hwnd: None,
            }),
        );
        m.update(
            now,
            Event::Ports(
                (0..9).map(|i| PortEntry { port: 3000 + i, pid: 100 + i as u32, process: "node.exe".into(), can_stop: i != 1 }).collect(),
            ),
        );
        m.update(now, Event::Click(Action::PortStop(3002)));
        m.update(now, Event::Clipboard { text: "#F2A93B".into(), unix_now: 1_700_000_000 });
        m.update(now, Event::Click(Action::TimerStart(25)));
        m.update(
            now,
            Event::Fullscreen {
                active: true,
                exe: "game.exe".into(),
                signals: crate::games::Signals { exclusive_d3d: true, gpu_3d_pct: None },
            },
        );
        m.update(now, Event::Stats(GpuStats { gpu_pct: Some(87.0), vram_mb: Some(6200), temp_c: Some(71), ping_ms: Some(18) }));
        m.update(now, Event::Capture(Capture { path: "C:\\c.mp4".into(), name: "c.mp4".into(), kind: CaptureKind::Clip, at_ms: now }));
        m.update(now, Event::Privacy(vec![(Device::Microphone, "Discord".into()), (Device::Camera, "Zoom".into())]));
        m.update(now, Event::Fullscreen { active: false, exe: String::new(), signals: Default::default() });
        m.update(now, Event::SetModule(ModuleId::Prayer, true));
        m.update(
            now,
            Event::Weather(crate::weather::parse_open_meteo(include_str!("../testdata/openmeteo.json"), "Jakarta", 1_792_825_200).unwrap()),
        );
        let mut alerts = crate::weather::parse_usgs(include_str!("../testdata/usgs.json")).unwrap();
        alerts[0].lat = -6.5;
        alerts[0].lon = 106.9;
        alerts[0].title = "M5.1 · a very long place description that has to be shortened somewhere".into();
        m.update(now, Event::Alerts(alerts));
        m.update(now, Event::Online(false));
        m.update(now, Event::Clock { year: 2026, month: 10, day: 24, minutes: 14.0 * 60.0, utc_offset_min: 420, unix: 1_792_825_200 });
        for i in 0..9 {
            m.update(now, Event::TodoAdded(format!("Task number {i} with a fairly long description to trim")));
        }
        m.update(now, Event::Click(Action::TodoToggle(2)));
        for edge in Edge::ALL {
            m.state = IslandState::Orb;
            out.push((format!("orb {edge:?}"), build(&m, edge, &ApproxMeasure)));
            m.state = IslandState::Seam;
            out.push((format!("seam {edge:?}"), build(&m, edge, &ApproxMeasure)));
            m.state = IslandState::Tag;
            m.hover_tag = false;
            out.push((format!("tag {edge:?}"), build(&m, edge, &ApproxMeasure)));
            m.hover_tag = true;
            out.push((format!("hover tag {edge:?}"), build(&m, edge, &ApproxMeasure)));
            m.state = IslandState::Slips;
            for slip in m.available_slips() {
                m.front = Some(slip);
                out.push((format!("{slip:?} {edge:?}"), build(&m, edge, &ApproxMeasure)));
            }
        }
        out
    }

    #[test]
    fn every_scene_fits_its_bounds_and_the_window_box() {
        for (name, s) in all_scenes() {
            let bounds = Rect::new(0.0, 0.0, s.size.w, s.size.h).inset(-1.5);
            for p in &s.prims {
                assert!(bounds.encloses(&p.bounds()), "{name}: {p:?} outside {:?}", s.size);
            }
            for h in &s.hits {
                assert!(bounds.inset(-8.0).encloses(&h.rect), "{name}: hit {:?} outside", h.label);
                assert!(!h.label.is_empty(), "{name}: unlabeled hit");
            }
            assert!(s.size.w <= BOX_DEPTH.max(BOX_SPAN) && s.size.h <= BOX_SPAN, "{name}: {:?} too big", s.size);
            if matches!(s.kind, SceneKind::Slip(_) | SceneKind::Tag) {
                let horizontal = name.ends_with("Top") || name.ends_with("Bottom");
                let (depth, span) = if horizontal { (s.size.h, s.size.w) } else { (s.size.w, s.size.h) };
                assert!(depth + EDGE_GAP <= BOX_DEPTH, "{name}: depth {depth}");
                assert!(span <= BOX_SPAN, "{name}: span {span}");
            }
        }
    }

    #[test]
    fn every_slip_has_the_same_size() {
        let scenes = all_scenes();
        let slips: Vec<&(String, Scene)> =
            scenes.iter().filter(|(n, s)| matches!(s.kind, SceneKind::Slip(_)) && n.ends_with("Left")).collect();
        assert!(slips.len() > 10);
        assert!(slips.iter().all(|(_, s)| s.size == slips[0].1.size), "{:?}", slips.iter().map(|(n, s)| (n, s.size)).collect::<Vec<_>>());
    }

    #[test]
    fn buttons_meet_touch_target() {
        for (name, s) in all_scenes() {
            for h in &s.hits {
                if let HitKind::Click(Action::SelectSlip(_) | Action::ToggleSlips) = h.kind {
                    continue;
                }
                assert!(h.rect.h >= 28.0 && h.rect.w >= 28.0, "{name}: {} is {:?}", h.label, h.rect);
            }
        }
    }

    #[test]
    fn ring_reflects_priority() {
        let mut m = Model::new(Config::default(), 3, 0);
        assert!(ring(&m).is_none());
        m.update(
            0,
            Event::Media(vec![MediaSession {
                id: "a".into(),
                title: "t".into(),
                playing: true,
                duration_ms: 100_000,
                ..Default::default()
            }]),
        );
        assert_eq!(ring(&m).unwrap().0, MEDIA);
        m.update(0, Event::Click(Action::TimerStart(25)));
        assert_eq!(ring(&m).unwrap().0, GAME);
        m.update(0, Event::Privacy(vec![(Device::Camera, "x".into())]));
        assert_eq!(ring(&m).unwrap().0, PRIVACY);
        m.update(10, Event::Wheel { notches: 1, shift: false });
        assert_eq!(ring(&m).unwrap().0, TEXT);
    }

    #[test]
    fn ring_animation_continuity() {
        let a =
            RingSpec { center: Point::new(22.0, 22.0), radius: 11.0, width: 2.0, color: MEDIA, value: 0.2, rate_per_ms: 1.0 / 200_000.0 };
        let later = RingSpec { value: 0.2 + 1000.0 / 200_000.0, ..a };
        assert!(later.continues(&a, 1000));
        let seeked = RingSpec { value: 0.6, ..a };
        assert!(!seeked.continues(&a, 1000));
        assert_eq!(ring_points(&RingSpec { value: 1.0, ..a }).len(), 49);
    }

    #[test]
    fn hits_resolve_topmost() {
        let mut m = Model::new(Config::default(), 3, 0);
        m.update(
            0,
            Event::Pipe(Request::Done {
                title: None,
                id: "x".into(),
                ok: false,
                code: Some(2),
                duration_s: None,
                hwnd: Some("0x5".into()),
            }),
        );
        let s = build(&m, Edge::Left, &ApproxMeasure);
        assert_eq!(s.kind, SceneKind::Tag);
        let action_hit = s.hits.iter().find(|h| matches!(h.kind, HitKind::Click(Action::JobOpenTerminal(_)))).unwrap();
        let c = (action_hit.rect.x + 2.0, action_hit.rect.y + 2.0);
        assert!(matches!(s.hit_at(c.0, c.1).unwrap().kind, HitKind::Click(Action::JobOpenTerminal(_))));
        assert!(matches!(s.hit_at(3.0, 3.0).unwrap().kind, HitKind::Click(Action::ToggleSlips)));
    }

    #[test]
    fn tabs_run_under_the_slip_on_every_edge() {
        let mut m = Model::new(Config::default(), 3, 0);
        m.update(0, Event::Click(Action::ToggleSlips));
        for edge in Edge::ALL {
            let s = build(&m, edge, &ApproxMeasure);
            let tabs: Vec<&Hit> = s.hits.iter().filter(|h| matches!(h.kind, HitKind::Click(Action::SelectSlip(_)))).collect();
            assert!(tabs.len() > 1);
            assert!(tabs.iter().all(|t| t.rect.y == tabs[0].rect.y && t.rect.bottom() <= s.size.h + 0.01), "{edge:?}");
            assert!(tabs[0].rect.y > s.size.h / 2.0, "{edge:?}: tabs at the bottom");
        }
    }

    #[test]
    fn media_tag_has_player_controls() {
        let mut m = Model::new(Config::default(), 3, 0);
        let art = Art { id: 7, w: 2, h: 2, pixels: std::sync::Arc::new(vec![0; 16]) };
        m.update(
            0,
            Event::Media(vec![MediaSession {
                id: "a".into(),
                app: "Spotify".into(),
                title: "Song".into(),
                artist: "Band".into(),
                playing: true,
                duration_ms: 100_000,
                can_play_pause: true,
                can_next: true,
                can_prev: true,
                art: Some(art),
                ..Default::default()
            }]),
        );
        assert_eq!(m.state, IslandState::Tag);
        let s = build(&m, Edge::Left, &ApproxMeasure);
        for want in [Action::MediaPrev, Action::MediaPlayPause, Action::MediaNext] {
            assert!(s.hits.iter().any(|h| h.kind == HitKind::Click(want.clone())), "{want:?}");
        }
        assert!(s.prims.iter().any(|p| matches!(p, Prim::Image { .. })), "album art");
        assert!(s.prims.iter().any(|p| matches!(p, Prim::Text { text, .. } if text == "Band")));
        // The hover summary for media shows the same controls.
        m.update(10_000, Event::Timer(TimerId::TagHide));
        m.update(10_000, Event::PointerEnter);
        m.update(10_200, Event::Timer(TimerId::HoverDelay));
        assert!(m.hover_tag);
        let s = build(&m, Edge::Right, &ApproxMeasure);
        assert!(s.hits.iter().any(|h| h.kind == HitKind::Click(Action::MediaNext)));
    }

    #[test]
    fn volume_level_shows_in_every_form() {
        let mut m = Model::new(Config::default(), 3, 0);
        m.update(0, Event::Mixer(MixerSnapshot { master: Volume { level: 0.5, muted: false }, ..Default::default() }));
        // No player: the wheel changes the system volume.
        m.update(10, Event::Wheel { notches: 2, shift: false });
        let text = |s: &Scene| s.prims.iter().any(|p| matches!(p, Prim::Text { text, .. } if text.ends_with("54%")));
        let glance = build(&m, Edge::Left, &ApproxMeasure);
        assert_eq!(glance.kind, SceneKind::Tag, "the orb turns into a volume glance");
        assert!(text(&glance));
        m.update(20, Event::Click(Action::ToggleSlips));
        m.update(20, Event::Click(Action::SelectSlip(ModuleId::Todo)));
        assert!(text(&build(&m, Edge::Top, &ApproxMeasure)), "a pill on the slip");
        m.update(5000, Event::Timer(TimerId::VolumeFlash));
        assert!(!text(&build(&m, Edge::Top, &ApproxMeasure)));
        m.update(5000, Event::Click(Action::SelectSlip(ModuleId::Mixer)));
        assert!(
            build(&m, Edge::Left, &ApproxMeasure).prims.iter().any(|p| matches!(p, Prim::Text { text, .. } if text == "54%")),
            "mixer rows show percent"
        );
    }

    #[test]
    fn wheel_changes_the_players_volume() {
        let mut m = Model::new(Config::default(), 3, 0);
        m.update(
            0,
            Event::Media(vec![MediaSession {
                id: "Spotify.exe".into(),
                app: "Spotify".into(),
                title: "Song".into(),
                playing: true,
                ..Default::default()
            }]),
        );
        let spotify = AudioApp {
            pid: 77,
            name: "Spotify".into(),
            exe: "spotify.exe".into(),
            volume: Volume { level: 0.3, muted: false },
            ..Default::default()
        };
        m.update(0, Event::Mixer(MixerSnapshot { master: Volume { level: 0.8, muted: false }, apps: vec![spotify], ..Default::default() }));
        let fx = m.update(10, Event::Wheel { notches: 1, shift: false });
        assert!(fx.iter().any(|e| matches!(e, Effect::SetAppVolume(77, v) if (*v - 0.32).abs() < 1e-6)), "{fx:?}");
        assert!(!fx.iter().any(|e| matches!(e, Effect::SetMasterVolume(_))));
        assert_eq!(m.mixer.master.level, 0.8);
        let s = build(&m, Edge::Left, &ApproxMeasure);
        assert!(s.prims.iter().any(|p| matches!(p, Prim::Text { text, .. } if text == "Spotify")));
        assert!(s.prims.iter().any(|p| matches!(p, Prim::Text { text, .. } if text == "32%")));
    }

    #[test]
    fn seam_orientation() {
        let mut m = Model::new(Config::default(), 3, 0);
        m.state = IslandState::Seam;
        assert_eq!(build(&m, Edge::Left, &ApproxMeasure).size, Size::new(SEAM_T, SEAM_L));
        assert_eq!(build(&m, Edge::Top, &ApproxMeasure).size, Size::new(SEAM_L, SEAM_T));
    }
}
