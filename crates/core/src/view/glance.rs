//! The glance: hovering the orb (or peeking at the Seam) shows everything
//! worth knowing right now, side by side along a top or bottom edge, or
//! stacked along a left or right one. Each item opens its slip.

use super::*;

/// One thing worth knowing now.
pub(super) struct Item {
    icon: Icon,
    color: Color,
    text: String,
    detail: Option<String>,
    module: ModuleId,
    /// The playing track: a play/pause button rides along.
    media: bool,
}

/// Docked top or bottom: the row's height and widest extent.
const ROW_H: f32 = 44.0;
const MAX_W: f32 = 680.0;
/// Docked left or right: the card's width and each line's height.
const COL_W: f32 = 290.0;
const LINE_H: f32 = 40.0;
/// At most this many items.
const MAX_ITEMS: usize = 6;

const TEXT_S: TextStyle = TextStyle { size: 12.5, weight: Weight::Medium, mono: false };
const DETAIL_S: TextStyle = TextStyle { size: 10.5, weight: Weight::Regular, mono: true };

/// What is worth knowing now, most pressing first.
pub(super) fn items(m: &Model) -> Vec<Item> {
    let now = m.now_ms;
    let mut v = Vec::new();
    let mut push =
        |icon, color, text: String, detail: Option<String>, module, media| v.push(Item { icon, color, text, detail, module, media });
    if let Some(p) = m.privacy.first() {
        let (icon, what) = match p.device {
            Device::Microphone => (Icon::Mic, "Mic"),
            Device::Camera => (Icon::Camera, "Camera"),
        };
        push(
            icon,
            PRIVACY,
            format!("{what} · {}", p.app),
            Some(format::clock(now.saturating_sub(p.since_ms) / 1000)),
            ModuleId::Privacy,
            false,
        );
    }
    if let Some(g) = &m.game {
        push(Icon::Game, GAME, g.exe.clone(), Some(format::clock(now.saturating_sub(g.started_ms) / 1000)), ModuleId::Game, false);
    }
    if let Some(rem) = m.timer.remaining(now) {
        let text = if m.timer.is_running() { "Focus" } else { "Focus paused" };
        push(Icon::Clock, GAME, text.into(), Some(format::clock(rem / 1000)), ModuleId::Timer, false);
    }
    if let Some(j) = m.jobs.iter().find(|j| matches!(j.status, JobStatus::Running { .. })) {
        let detail = match j.status {
            JobStatus::Running { progress: Some(p) } => format!("{:.0}%", p * 100.0),
            _ => format::duration_short(now.saturating_sub(j.started_ms) / 1000),
        };
        push(Icon::Terminal, CODER, j.title.clone(), Some(detail), ModuleId::Commands, false);
    }
    if let Some(s) = m.front_media().filter(|s| !s.title.is_empty()) {
        let detail = if s.artist.is_empty() { None } else { Some(s.artist.clone()) };
        push(Icon::Note, MEDIA, s.title.clone(), detail, ModuleId::Media, true);
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
        push(Icon::Calendar, CODER, e.title.clone(), Some(when), ModuleId::Calendar, false);
    }
    if let (Some(day), Some(t)) = (m.prayer_day, m.local_minute()) {
        if let Some((p, at)) = day.next(t) {
            let left = (at - t).max(0.0).round() as i64;
            let detail = if left < 60 { format!("in {left}m") } else { format!("in {}h{:02}", left / 60, left % 60) };
            push(Icon::Moon, OK, format!("{} {}", p.name(), crate::prayer::hhmm(at)), Some(detail), ModuleId::Prayer, false);
        }
    }
    if m.enabled(ModuleId::Weather) {
        if let Some(a) = m.relevant_alerts(1).into_iter().next().filter(|a| m.unix_now().is_some_and(|u| u - a.time < 6 * 3600)) {
            push(Icon::Warning, MEDIA, a.title.clone(), Some(a.hazard.label().to_string()), ModuleId::Weather, false);
        }
    }
    if let Some(w) = &m.weather {
        let detail = w.rain_chance.filter(|r| *r >= 40).map(|r| format!("rain {r}%"));
        push(condition_icon(w.condition), PORTS, format!("{:.0}° {}", w.temp_c, w.description), detail, ModuleId::Weather, false);
    }
    if m.enabled(ModuleId::Todo) {
        let open: Vec<&TodoItem> = m.todos.iter().filter(|t| !t.done).collect();
        if let Some(first) = open.first() {
            push(Icon::Todo, OK, first.text.clone(), Some(format!("{} left", open.len())), ModuleId::Todo, false);
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
                false,
            );
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
    hits.push(Hit { rect: r, kind: HitKind::Click(Action::SelectSlip(it.module)), label: format!("Open {}", it.module.label()) });
    if it.media {
        if let Some(s) = m.front_media() {
            let pr = Rect::new(r.right() - 36.0, r.y + (r.h - 28.0) / 2.0, 28.0, 28.0);
            let pc = if s.can_play_pause { MEDIA } else { TRACK };
            b.extend(soft::raised_grad(pr, 14.0, accent_light(pc), accent_deep(pc), 1.5));
            let (icon, label) = if s.playing { (Icon::Pause, "Pause") } else { (Icon::Play, "Play") };
            b.extend(icon_prims(icon, Rect::new(pr.x + 8.0, pr.y + 8.0, 12.0, 12.0), SURFACE));
            if s.can_play_pause {
                hits.push(Hit { rect: pr, kind: HitKind::Click(Action::MediaPlayPause), label: label.into() });
            }
        }
    }
    ring
}

/// Width an item needs in a row (capped).
fn width(tm: &dyn TextMeasure, it: &Item) -> (f32, f32) {
    let tw = tm.width(&it.text, TEXT_S).max(it.detail.as_ref().map(|d| tm.width(d, DETAIL_S)).unwrap_or(0.0)).ceil().min(126.0);
    let extra = if it.media { 36.0 } else { 0.0 };
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
        let tw = r.w - 8.0 - 26.0 - 10.0 - 10.0 - if it.media { 36.0 } else { 0.0 };
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
