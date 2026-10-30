//! The glance: hovering the orb (or peeking at the Seam) shows everything
//! worth knowing right now, side by side along a top or bottom edge, or
//! stacked along a left or right one. Each item opens its slip. While
//! something plays the glance is the player alone (see `view::tag`).

use super::*;

/// One thing worth knowing now.
pub(super) struct Item {
    icon: Icon,
    color: Color,
    text: String,
    detail: Option<String>,
    module: ModuleId,
    /// Small round buttons riding along (join, mic…).
    buttons: Vec<(Icon, Color, Action, &'static str)>,
    /// Today's prices, drawn as a sparkline after the text.
    spark: Option<Vec<f32>>,
}

/// Docked top or bottom: the row's height and widest extent.
const ROW_H: f32 = 44.0;
const MAX_W: f32 = 680.0;
/// Docked left or right: the card's width and each line's height.
const COL_W: f32 = 290.0;
const LINE_H: f32 = 40.0;
/// A market item's sparkline.
const SPARK_W: f32 = 34.0;
/// At most this many items.
const MAX_ITEMS: usize = 6;

const TEXT_S: TextStyle = TextStyle { size: 12.5, weight: Weight::Medium, mono: false };
const DETAIL_S: TextStyle = TextStyle { size: 10.5, weight: Weight::Regular, mono: true };

/// What is worth knowing now, most pressing first.
pub(super) fn items(m: &Model) -> Vec<Item> {
    let now = m.now_ms;
    let mut v = Vec::new();
    // In a meeting: join it, mute or unmute the microphone.
    if let Some(e) = m.meeting() {
        let mic = match m.mic_muted {
            Some(true) => (Icon::SpeakerMuted, PRIVACY, Action::MicToggle, "Unmute microphone"),
            _ => (Icon::Mic, OK, Action::MicToggle, "Mute microphone"),
        };
        v.push(Item {
            icon: Icon::Calendar,
            color: CODER,
            text: e.title.clone(),
            detail: Some("meeting now".into()),
            module: ModuleId::Calendar,
            buttons: vec![(Icon::Open, CODER, Action::OpenUrl(e.link.clone()), "Join meeting"), mic],
            spark: None,
        });
    }
    let mut push = |icon, color, text: String, detail: Option<String>, module| {
        v.push(Item { icon, color, text, detail, module, buttons: Vec::new(), spark: None })
    };
    if let Some(p) = m.privacy.first() {
        let (icon, what) = match p.device {
            Device::Microphone => (Icon::Mic, "Mic"),
            Device::Camera => (Icon::Camera, "Camera"),
        };
        push(icon, PRIVACY, format!("{what} · {}", p.app), Some(format::clock(now.saturating_sub(p.since_ms) / 1000)), ModuleId::Privacy);
    }
    if let Some(g) = &m.game {
        push(Icon::Game, GAME, g.exe.clone(), Some(format::clock(now.saturating_sub(g.started_ms) / 1000)), ModuleId::Game);
    }
    if let Some(rem) = m.timer.remaining(now) {
        let text = if m.on_break {
            "Break"
        } else if m.timer.is_running() {
            "Focus"
        } else {
            "Focus paused"
        };
        push(Icon::Clock, GAME, text.into(), Some(format::clock(rem / 1000)), ModuleId::Timer);
    }
    if let Some(d) = m.downloads.first() {
        let more = if m.downloads.len() > 1 { format!(" +{}", m.downloads.len() - 1) } else { String::new() };
        push(Icon::Download, PORTS, d.name.clone(), Some(format!("{}{more}", format::bytes(d.bytes))), ModuleId::Captures);
    }
    if let Some((p, pct)) = m.power.and_then(|p| p.percent.map(|v| (p, v))).filter(|(p, v)| !p.on_ac && *v <= 30) {
        let left = p.secs_left.map(|s| format!("{} left", format::hours_minutes(s as u64 / 60)));
        push(Icon::Battery, PRIVACY_TEXT, format!("Battery {pct}%"), left, ModuleId::Power);
    }
    if let Some(j) = m.jobs.iter().find(|j| matches!(j.status, JobStatus::Running { .. })) {
        let detail = match j.status {
            JobStatus::Running { progress: Some(p) } => format!("{:.0}%", p * 100.0),
            _ => format::duration_short(now.saturating_sub(j.started_ms) / 1000),
        };
        push(Icon::Terminal, CODER, j.title.clone(), Some(detail), ModuleId::Commands);
    }
    if let (Some(e), Some(unix)) = (m.next_event_within(12 * 3600), m.unix_now()) {
        let mins = ((e.start - unix) as f64 / 60.0).ceil() as i64;
        let when = match mins {
            ..=0 => "now".to_string(),
            1..=59 => format!("in {mins}m"),
            _ => {
                let offset = m.clock.map(|c| c.utc_offset_min as i64 * 60).unwrap_or(0);
                let s = (e.start + offset).rem_euclid(86_400);
                format!("{:02}:{:02}", s / 3600, s % 3600 / 60)
            }
        };
        push(Icon::Calendar, CODER, e.title.clone(), Some(when), ModuleId::Calendar);
    }
    if let (Some((what, at)), Some(t)) = (m.fasting(), m.local_minute()) {
        let left = (at - t).max(0.0).round() as i64;
        let detail = if left < 60 { format!("in {left}m") } else { format!("in {}h{:02}", left / 60, left % 60) };
        push(Icon::Moon, OK, format!("{what} {}", crate::prayer::hhmm(at.rem_euclid(1440.0))), Some(detail), ModuleId::Prayer);
    } else if let (Some((p, at)), Some(t)) = (m.next_prayer(), m.local_minute()) {
        let left = (at - t).max(0.0).round() as i64;
        let detail = if left < 60 { format!("in {left}m") } else { format!("in {}h{:02}", left / 60, left % 60) };
        push(Icon::Moon, OK, format!("{} {}", p.name(), crate::prayer::hhmm(at.rem_euclid(1440.0))), Some(detail), ModuleId::Prayer);
    }
    if m.enabled(ModuleId::Weather) {
        if let Some(a) = m.relevant_alerts(1).into_iter().next().filter(|a| m.unix_now().is_some_and(|u| u - a.time < 6 * 3600)) {
            push(Icon::Warning, MEDIA, a.title.clone(), Some(a.hazard.label().to_string()), ModuleId::Weather);
        }
    }
    if let Some(w) = &m.weather {
        let detail = w.rain_chance.filter(|r| *r >= 40).map(|r| format!("rain {r}%"));
        push(condition_icon(w.condition), PORTS, format!("{:.0}° {}", w.temp_c, w.description), detail, ModuleId::Weather);
    }
    if m.enabled(ModuleId::Todo) {
        let open: Vec<&TodoItem> = m.todos.iter().filter(|t| !t.done).collect();
        if let Some(first) = open.first() {
            push(Icon::Todo, OK, first.text.clone(), Some(format!("{} left", open.len())), ModuleId::Todo);
        }
    }
    // The watchlist's biggest move today.
    if m.enabled(ModuleId::Finance) {
        let mover = m.quotes.iter().max_by(|a, b| a.change_pct.abs().total_cmp(&b.change_pct.abs()));
        if let Some(q) = mover {
            let name = m
                .config
                .finance
                .watchlist
                .iter()
                .find(|w| w.symbol.eq_ignore_ascii_case(&q.symbol) && !w.name.is_empty())
                .map(|w| w.name.clone())
                .unwrap_or_else(|| q.symbol.clone());
            let color = if q.change_pct >= 0.0 { OK } else { PRIVACY_TEXT };
            push(
                Icon::Chart,
                color,
                format!("{name} {}", crate::finance::format_price(q)),
                Some(crate::finance::format_change(q.change_pct)),
                ModuleId::Finance,
            );
            if let Some(it) = v.last_mut().filter(|_| q.points.len() >= 2) {
                it.spark = Some(q.points.clone());
            }
        }
    }
    v.truncate(MAX_ITEMS);
    v
}

/// The glance scene: a row docked top or bottom, a column docked left or right.
pub(super) fn glance(m: &Model, edge: Edge, tm: &dyn TextMeasure, items: Vec<Item>) -> Scene {
    if edge.is_vertical() {
        column(m, edge, tm, items)
    } else {
        row(m, tm, items)
    }
}

/// Draws one item at `r` (icon in a small well, the text, the detail under it).
fn item(b: &mut Vec<Prim>, hits: &mut Vec<Hit>, m: &Model, it: &Item, r: Rect, first: bool, text_w: f32) -> Option<RingSpec> {
    let c = Point::new(r.x + 8.0 + 13.0, r.y + r.h / 2.0);
    b.extend(soft::well_circle(c.x, c.y, 13.0));
    let ring = if first { ring_spec(m, c) } else { None };
    b.extend(icon_prims(it.icon, Rect::new(c.x - 6.5, c.y - 6.5, 13.0, 13.0), it.color));
    let tx = c.x + 13.0 + 10.0;
    let (ty, th) = if it.detail.is_some() { (r.y + r.h / 2.0 - 16.0, 18.0) } else { (r.y, r.h) };
    b.push(Prim::Text {
        rect: Rect::new(tx, ty, text_w, th),
        text: it.text.clone(),
        style: TEXT_S,
        color: TEXT,
        align: Align::Start,
        wrap: false,
    });
    if let Some(d) = &it.detail {
        let dc = if it.color == PRIVACY { PRIVACY_TEXT } else { TEXT_2 };
        b.push(Prim::Text {
            rect: Rect::new(tx, r.y + r.h / 2.0 + 1.0, text_w, 15.0),
            text: d.clone(),
            style: DETAIL_S,
            color: dc,
            align: Align::Start,
            wrap: false,
        });
    }
    if let Some(points) = &it.spark {
        let sr = Rect::new(tx + text_w + 8.0, r.y + r.h / 2.0 - 7.0, SPARK_W, 14.0);
        b.push(Prim::Polyline { points: spark_points(sr, points), width: 1.4, color: it.color, closed: false });
    }
    hits.push(Hit { rect: r, kind: HitKind::Click(Action::SelectSlip(it.module)), label: format!("Open {}", it.module.label()) });
    let mut bx = r.right() - 8.0 - 28.0 * it.buttons.len() as f32 - 4.0 * (it.buttons.len() as f32 - 1.0).max(0.0);
    for (icon, color, action, label) in &it.buttons {
        let pr = Rect::new(bx, r.y + (r.h - 28.0) / 2.0, 28.0, 28.0);
        b.extend(soft::raised_grad(pr, 14.0, accent_light(*color), accent_deep(*color), 1.5));
        b.extend(icon_prims(*icon, Rect::new(pr.x + 8.0, pr.y + 8.0, 12.0, 12.0), SURFACE));
        hits.push(Hit { rect: pr, kind: HitKind::Click(action.clone()), label: (*label).into() });
        bx += 32.0;
    }
    ring
}

/// Width an item needs in a row (capped).
fn width(tm: &dyn TextMeasure, it: &Item) -> (f32, f32) {
    let tw = tm.width(&it.text, TEXT_S).max(it.detail.as_ref().map(|d| tm.width(d, DETAIL_S)).unwrap_or(0.0)).ceil().min(126.0);
    let extra = 32.0 * it.buttons.len() as f32
        + if it.buttons.is_empty() { 0.0 } else { 4.0 }
        + if it.spark.is_some() { SPARK_W + 8.0 } else { 0.0 };
    (8.0 + 26.0 + 10.0 + tw + 14.0 + extra, tw)
}

fn row(m: &Model, tm: &dyn TextMeasure, items: Vec<Item>) -> Scene {
    let h = ROW_H;
    // Keep what fits, most pressing first.
    let mut shown = Vec::new();
    let mut w = 8.0;
    for it in items {
        let (iw, tw) = width(tm, &it);
        if w + iw + 8.0 > MAX_W && !shown.is_empty() {
            break;
        }
        w += iw;
        shown.push((it, iw, tw));
    }
    let w = w + 8.0;
    let mut prims = surface(Rect::new(0.0, 0.0, w, h), h / 2.0).to_vec();
    let mut hits = vec![Hit { rect: Rect::new(0.0, 0.0, w, h), kind: HitKind::Click(Action::ToggleSlips), label: "Open Ngambang".into() }];
    let mut ring = None;
    let mut x = 8.0;
    let n = shown.len();
    for (i, (it, iw, tw)) in shown.into_iter().enumerate() {
        let r = Rect::new(x, 0.0, iw, h);
        let rs = item(&mut prims, &mut hits, m, &it, r, i == 0, tw);
        ring = ring.or(rs);
        x += iw;
        if i + 1 < n {
            prims.push(Prim::RoundRect { rect: Rect::new(x - 0.5, 12.0, 1.0, h - 24.0), radius: 0.5, fill: Some(HAIRLINE), stroke: None });
        }
    }
    Scene { kind: SceneKind::Tag, size: Size::new(w, h), flush: false, clip_radius: 14.0, prims, hits, ring }
}

fn column(m: &Model, edge: Edge, tm: &dyn TextMeasure, items: Vec<Item>) -> Scene {
    let pad = 6.0;
    let w = items.iter().map(|it| width(tm, it).0).fold(200.0_f32, f32::max).min(COL_W) + 2.0 * pad;
    let h = items.len() as f32 * LINE_H + 2.0 * pad;
    let mut prims = surface(Rect::new(0.0, 0.0, w, h), 20.0).to_vec();
    // The side against the edge has tighter corners, like a tag's.
    let patch = if edge == Edge::Right { Rect::new(w / 2.0, 0.0, w / 2.0, h) } else { Rect::new(0.0, 0.0, w / 2.0, h) };
    prims.push(surface_patch(patch, 12.0));
    let mut hits = vec![Hit { rect: Rect::new(0.0, 0.0, w, h), kind: HitKind::Click(Action::ToggleSlips), label: "Open Ngambang".into() }];
    let mut ring = None;
    let n = items.len();
    for (i, it) in items.iter().enumerate() {
        let r = Rect::new(pad, pad + i as f32 * LINE_H, w - 2.0 * pad, LINE_H);
        let spark = if it.spark.is_some() { SPARK_W + 8.0 } else { 0.0 };
        let tw = r.w - 8.0 - 26.0 - 10.0 - 10.0 - 32.0 * it.buttons.len() as f32 - spark;
        let rs = item(&mut prims, &mut hits, m, it, r, i == 0, tw);
        ring = ring.or(rs);
        if i + 1 < n {
            prims.push(Prim::RoundRect {
                rect: Rect::new(r.x + 44.0, r.bottom() - 0.5, r.w - 52.0, 1.0),
                radius: 0.5,
                fill: Some(HAIRLINE),
                stroke: None,
            });
        }
    }
    Scene { kind: SceneKind::Tag, size: Size::new(w, h), flush: false, clip_radius: 12.0, prims, hits, ring }
}

/// The player glance's width, the height of its position and volume lines,
/// and the room for each app's dot.
const PLAYER_W: f32 = 340.0;
const LINE: f32 = 28.0;
const DOT: f32 = 28.0;

/// While something plays: the player alone. Cover, title and artist with
/// ⏮ ⏯ ⏭; the position to drag (seek); the player's volume; dots to switch
/// between apps that play; and, above it all, a camera or microphone in use.
pub(super) fn player(m: &Model, s: &MediaSession, edge: Edge, tm: &dyn TextMeasure) -> Scene {
    let w = PLAYER_W;
    let pad = 10.0;
    let privacy = m.privacy.first();
    let mut y = pad;
    let mut prims = Vec::new();
    let mut hits = Vec::new();
    // Measure first: a privacy line on top, then the three rows.
    let banner_h = if privacy.is_some() { 32.0 } else { 0.0 };
    let h = pad + banner_h + 44.0 + 2.0 * LINE + pad - 4.0;
    prims.extend(surface(Rect::new(0.0, 0.0, w, h), 20.0));
    if edge.is_vertical() {
        let patch = if edge == Edge::Right { Rect::new(w / 2.0, 0.0, w / 2.0, h) } else { Rect::new(0.0, 0.0, w / 2.0, h) };
        prims.push(surface_patch(patch, 12.0));
    }
    hits.push(Hit { rect: Rect::new(0.0, 0.0, w, h), kind: HitKind::Click(Action::ToggleSlips), label: "Open Ngambang".into() });

    if let Some(p) = privacy {
        let (icon, what) = match p.device {
            Device::Microphone => (Icon::Mic, "Microphone"),
            Device::Camera => (Icon::Camera, "Camera"),
        };
        let r = Rect::new(pad, y, w - 2.0 * pad, 28.0);
        prims.push(Prim::RoundRect { rect: r, radius: 14.0, fill: Some(PRIVACY.with_alpha(0x30)), stroke: None });
        prims.extend(icon_prims(icon, Rect::new(r.x + 9.0, r.y + 8.0, 12.0, 12.0), PRIVACY_TEXT));
        let since = format::clock(m.now_ms.saturating_sub(p.since_ms) / 1000);
        prims.push(Prim::Text {
            rect: Rect::new(r.x + LINE, r.y, r.w - 34.0, r.h),
            text: format!("{what} in use · {} · {since}", p.app),
            style: DETAIL_S,
            color: PRIVACY_TEXT,
            align: Align::Start,
            wrap: false,
        });
        hits.push(Hit { rect: r, kind: HitKind::Click(Action::SelectSlip(ModuleId::Privacy)), label: format!("{what} in use") });
        y += banner_h;
    }

    // Cover, titles and the controls.
    let art = Rect::new(pad, y + 2.0, 40.0, 40.0);
    match &s.art {
        Some(a) => prims.push(Prim::Image { rect: art, art: a.clone(), radius: 8.0 }),
        None => {
            prims.push(Prim::RoundRect { rect: art, radius: 8.0, fill: Some(Color::rgb(0x3A2E1C)), stroke: None });
            prims.extend(icon_prims(Icon::Note, art.inset(11.0), MEDIA));
        }
    }
    let btn = 30.0;
    let controls_w = 3.0 * btn + 4.0;
    let tx = art.right() + 10.0;
    let text_w = w - tx - controls_w - pad - 6.0;
    let title = if s.title.is_empty() { s.app.clone() } else { s.title.clone() };
    let sub = if s.artist.is_empty() { s.app.clone() } else { s.artist.clone() };
    let title_style = TextStyle { size: 13.0, weight: Weight::Semibold, mono: false };
    let sub_style = TextStyle { size: 11.5, weight: Weight::Regular, mono: false };
    prims.push(Prim::Text {
        rect: Rect::new(tx, y + 3.0, text_w, 19.0),
        text: title,
        style: title_style,
        color: TEXT,
        align: Align::Start,
        wrap: false,
    });
    prims.push(Prim::Text {
        rect: Rect::new(tx, y + 22.0, text_w, 17.0),
        text: sub,
        style: sub_style,
        color: TEXT_2,
        align: Align::Start,
        wrap: false,
    });
    let mut bx = w - pad - controls_w;
    let (play_icon, play_label) = if s.playing { (Icon::Pause, "Pause") } else { (Icon::Play, "Play") };
    for (icon, label, action, on) in [
        (Icon::Prev, "Previous track", Action::MediaPrev, s.can_prev),
        (play_icon, play_label, Action::MediaPlayPause, s.can_play_pause),
        (Icon::Next, "Next track", Action::MediaNext, s.can_next),
    ] {
        let r = Rect::new(bx, y + (44.0 - btn) / 2.0, btn, btn);
        let main = action == Action::MediaPlayPause;
        if main {
            let c = if on { MEDIA } else { TRACK };
            prims.extend(soft::raised_grad(r.inset(2.0), 13.0, accent_light(c), accent_deep(c), 1.5));
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
    y += 44.0;

    // Position: elapsed, a bar to drag, the length.
    let time_s = TextStyle { size: 10.0, weight: Weight::Regular, mono: true };
    let tw = tm.width("00:00", time_s).ceil().max(30.0);
    if s.duration_ms > 0 {
        let pos = format::clock(s.position_at(m.now_ms) / 1000);
        prims.push(Prim::Text {
            rect: Rect::new(pad, y, tw, LINE),
            text: pos,
            style: time_s,
            color: TEXT_2,
            align: Align::Start,
            wrap: false,
        });
        let len = format::clock(s.duration_ms / 1000);
        prims.push(Prim::Text {
            rect: Rect::new(w - pad - tw, y, tw, LINE),
            text: len,
            style: time_s,
            color: TEXT_2,
            align: Align::End,
            wrap: false,
        });
        let bar = Rect::new(pad + tw + 8.0, y, w - 2.0 * (pad + tw + 8.0), LINE);
        let target = s.can_seek.then_some(SliderTarget::MediaSeek);
        slider(&mut prims, &mut hits, bar, s.progress_at(m.now_ms).unwrap_or(0.0), MEDIA, target, "Seek");
    } else {
        prims.push(Prim::Text {
            rect: Rect::new(pad, y, w - 2.0 * pad, LINE),
            text: if s.playing { "Live".into() } else { "Paused".into() },
            style: time_s,
            color: TEXT_2,
            align: Align::Start,
            wrap: false,
        });
    }
    y += LINE;

    // Volume of this player (the system's when it can't be told apart, or
    // behind FxSound), and a dot for each app that plays.
    let (level, muted, target) = match m.volume_app() {
        Some(a) => (a.volume.level, a.volume.muted, SliderTarget::App(a.pid)),
        None => (m.mixer.master.level, m.mixer.master.muted, SliderTarget::Master),
    };
    let icon = if muted { Icon::SpeakerMuted } else { Icon::Speaker };
    prims.extend(icon_prims(icon, Rect::new(pad + 2.0, y + (LINE - 13.0) / 2.0, 13.0, 13.0), if muted { PRIVACY } else { TEXT_2 }));
    let sources: Vec<&MediaSession> = m.media.iter().take(4).collect();
    let dots_w = if sources.len() > 1 { sources.len() as f32 * DOT + 4.0 } else { 0.0 };
    let vol = Rect::new(pad + 24.0, y, 130.0_f32.min(w - 2.0 * pad - 24.0 - dots_w), LINE);
    slider(&mut prims, &mut hits, vol, level, if muted { TRACK } else { MEDIA }, Some(target), "Volume");
    if sources.len() > 1 {
        let mut dx = w - pad - sources.len() as f32 * DOT + DOT / 2.0;
        for (i, x) in sources.iter().enumerate() {
            let current = x.id == s.id;
            let (r, fill) = if current {
                (4.5, MEDIA)
            } else if x.playing {
                (3.5, TEXT_2)
            } else {
                (3.5, TRACK)
            };
            prims.push(Prim::Circle { cx: dx, cy: y + LINE / 2.0, r, fill: Some(fill), stroke: None });
            let label = if x.playing { x.app.clone() } else { format!("{} (paused)", x.app) };
            hits.push(Hit { rect: Rect::new(dx - DOT / 2.0, y, DOT, LINE), kind: HitKind::Click(Action::MediaSource(i)), label });
            dx += DOT;
        }
    } else {
        let app = format::ellipsize(&s.app, 22);
        let r = Rect::new(vol.right() + 10.0, y, w - pad - vol.right() - 10.0, LINE);
        prims.push(Prim::Text { rect: r, text: app, style: time_s, color: LABEL, align: Align::End, wrap: false });
    }
    Scene { kind: SceneKind::Tag, size: Size::new(w, h), flush: false, clip_radius: 12.0, prims, hits, ring: None }
}

/// A thin bar with a knob; `target` None draws it without a handle to drag.
fn slider(b: &mut Vec<Prim>, hits: &mut Vec<Hit>, r: Rect, value: f32, color: Color, target: Option<SliderTarget>, label: &str) {
    let cy = r.y + r.h / 2.0;
    let v = value.clamp(0.0, 1.0);
    b.extend(soft::well(Rect::new(r.x - 2.0, cy - 3.5, r.w + 4.0, 7.0), 3.5));
    if v > 0.01 {
        let fill = Rect::new(r.x, cy - 1.5, r.w * v, 3.0);
        b.push(Prim::Gradient { rect: fill, radius: 1.5, from: accent_deep(color), to: accent_light(color), diagonal: true });
    }
    if let Some(t) = target {
        let kx = (r.x + r.w * v).clamp(r.x + 3.0, r.right() - 3.0);
        b.extend(soft::raised_circle(kx, cy, 6.0, SURFACE, 1.5));
        b.push(Prim::Circle { cx: kx, cy, r: 2.0, fill: Some(color), stroke: None });
        hits.push(Hit { rect: r, kind: HitKind::Slider(t), label: label.into() });
    }
}
