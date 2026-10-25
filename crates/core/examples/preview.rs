//! Render island scenes to an HTML page of SVGs, to look at the design
//! without Windows: `cargo run -p ngambang-core --example preview -- out.html`.
//! Text is approximate (the browser's fonts stand in for DirectWrite).

use ngambang_core::config::{Config, ModuleId};
use ngambang_core::geometry::{Color, Edge};
use ngambang_core::model::*;
use ngambang_core::view::{build, Align, ApproxMeasure, Prim, Scene, Weight};

fn css(c: Color) -> String {
    format!("rgba({},{},{},{:.3})", c.r(), c.g(), c.b(), c.a() as f32 / 255.0)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn svg(scene: &Scene) -> String {
    let mut out = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{w}' height='{h}' viewBox='0 0 {w} {h}'>",
        w = scene.size.w.ceil(),
        h = scene.size.h.ceil()
    );
    for p in &scene.prims {
        match p {
            Prim::RoundRect { rect, radius, fill, stroke } => {
                let f = fill.map(css).unwrap_or_else(|| "none".into());
                let (s, sw) = stroke.map(|(c, w)| (css(c), w)).unwrap_or(("none".into(), 0.0));
                out += &format!(
                    "<rect x='{}' y='{}' width='{}' height='{}' rx='{}' fill='{f}' stroke='{s}' stroke-width='{sw}'/>",
                    rect.x + sw / 2.0,
                    rect.y + sw / 2.0,
                    (rect.w - sw).max(0.0),
                    (rect.h - sw).max(0.0),
                    radius.min(rect.w / 2.0).min(rect.h / 2.0)
                );
            }
            Prim::Circle { cx, cy, r, fill, stroke } => {
                let f = fill.map(css).unwrap_or_else(|| "none".into());
                let (s, sw) = stroke.map(|(c, w)| (css(c), w)).unwrap_or(("none".into(), 0.0));
                out += &format!("<circle cx='{cx}' cy='{cy}' r='{r}' fill='{f}' stroke='{s}' stroke-width='{sw}'/>");
            }
            Prim::Polyline { points, width, color, closed } => {
                let pts: Vec<String> = points.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
                let tag = if *closed { "polygon" } else { "polyline" };
                out += &format!(
                    "<{tag} points='{}' fill='none' stroke='{}' stroke-width='{width}' stroke-linecap='round' stroke-linejoin='round'/>",
                    pts.join(" "),
                    css(*color)
                );
            }
            Prim::Polygon { points, fill } => {
                let pts: Vec<String> = points.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
                out += &format!("<polygon points='{}' fill='{}'/>", pts.join(" "), css(*fill));
            }
            Prim::Gradient { rect, radius, from, to, diagonal } => {
                let id = format!("g{}", out.len());
                let (x2, y2) = if *diagonal { (1, 1) } else { (0, 1) };
                out += &format!(
                    "<defs><linearGradient id='{id}' x1='0' y1='0' x2='{x2}' y2='{y2}'><stop offset='0' stop-color='{}'/><stop offset='1' stop-color='{}'/></linearGradient></defs>\
                     <rect x='{}' y='{}' width='{}' height='{}' rx='{}' fill='url(#{id})'/>",
                    css(*from),
                    css(*to),
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    radius.min(rect.w / 2.0).min(rect.h / 2.0)
                );
            }
            Prim::Image { rect, radius, .. } => {
                out +=
                    &format!("<rect x='{}' y='{}' width='{}' height='{}' rx='{radius}' fill='url(#art)'/>", rect.x, rect.y, rect.w, rect.h);
            }
            Prim::Text { rect, text, style, color, align, wrap } => {
                let family = if style.mono { "Cascadia Mono, Consolas, monospace" } else { "Segoe UI, Inter, Arial, sans-serif" };
                let weight = match style.weight {
                    Weight::Regular => 400,
                    Weight::Medium => 500,
                    Weight::Semibold => 600,
                };
                let (x, anchor) = match align {
                    Align::Start => (rect.x, "start"),
                    Align::Center => (rect.x + rect.w / 2.0, "middle"),
                    Align::End => (rect.right(), "end"),
                };
                let lines: Vec<&str> = if *wrap { text.lines().collect() } else { vec![text.as_str()] };
                for (i, line) in lines.iter().enumerate() {
                    let y = if *wrap { rect.y + style.size * (1.3 * i as f32 + 1.0) } else { rect.y + rect.h / 2.0 };
                    let base = if *wrap { "auto" } else { "central" };
                    out += &format!(
                        "<text x='{x}' y='{y}' font-family='{family}' font-size='{}' font-weight='{weight}' fill='{}' text-anchor='{anchor}' dominant-baseline='{base}'>{}</text>",
                        style.size,
                        css(*color),
                        esc(line)
                    );
                }
            }
        }
    }
    out + "</svg>"
}

fn sample() -> Model {
    let now = 1_000_000;
    let mut m = Model::new(Config::default(), 3, now);
    m.update(now, Event::Clock { year: 2026, month: 10, day: 25, minutes: 14.0 * 60.0 + 5.0, utc_offset_min: 420, unix: 1_792_911_900 });
    m.update(
        now,
        Event::Media(vec![MediaSession {
            id: "Spotify.exe".into(),
            app: "Spotify".into(),
            title: "Bermuda Love Triangle".into(),
            artist: "Raisa".into(),
            playing: true,
            position_ms: 81_000,
            duration_ms: 214_000,
            at_ms: now,
            can_play_pause: true,
            can_next: true,
            can_prev: true,
            can_seek: true,
            art: Some(Art { id: 1, w: 1, h: 1, pixels: std::sync::Arc::new(vec![0; 4]) }),
        }]),
    );
    m.update(
        now,
        Event::Mixer(MixerSnapshot {
            device: "Headphones".into(),
            master: Volume { level: 0.62, muted: false },
            apps: ["Spotify", "Discord", "Chrome"]
                .iter()
                .enumerate()
                .map(|(i, n)| AudioApp {
                    pid: 10 + i as u32,
                    name: n.to_string(),
                    exe: format!("{}.exe", n.to_lowercase()),
                    volume: Volume { level: 0.4 + i as f32 * 0.2, muted: i == 1 },
                    peak: 0.5,
                    ..Default::default()
                })
                .collect(),
        }),
    );
    m.update(now, Event::SetModule(ModuleId::Prayer, true));
    m.update(
        now,
        Event::Weather(
            ngambang_core::weather::parse_open_meteo(include_str!("../testdata/openmeteo.json"), "Jakarta", 1_792_911_900).unwrap(),
        ),
    );
    let mut alerts = ngambang_core::weather::parse_usgs(include_str!("../testdata/usgs.json")).unwrap();
    alerts[0].lat = -6.9;
    alerts[0].lon = 106.6;
    alerts[0].time = 1_792_905_000;
    m.update(now, Event::Alerts(alerts));
    for t in ["Review the release notes", "Reply to Dimas about the API", "Buy groceries"] {
        m.update(now, Event::TodoAdded(t.into()));
    }
    m.update(now, Event::Click(Action::TodoToggle(1)));
    m.update(now, Event::Ports(vec![PortEntry { port: 3000, pid: 4120, process: "node.exe".into(), can_stop: true }]));
    m.config.calendar.feeds = vec![
        ngambang_core::config::CalendarFeed { name: "Work".into(), url: "x".into(), color: "#A99BFF".into() },
        ngambang_core::config::CalendarFeed { name: "Family".into(), url: "y".into(), color: "#7FD1AE".into() },
    ];
    let mut events =
        ngambang_core::calendar::parse(include_str!("../testdata/calendar.ics"), 0, 1_792_886_400, 1_792_886_400 + 10 * 86_400, 420);
    events[1].feed = 1;
    m.update(now, Event::Calendar(events));
    let mut q = ngambang_core::finance::parse_chart(include_str!("../testdata/yahoo_chart.json")).unwrap();
    q.symbol = "^JKSE".into();
    q.kind = "INDEX".into();
    q.price = 7123.4;
    let mut btc = q.clone();
    btc.symbol = "BTC-USD".into();
    btc.kind = "CRYPTOCURRENCY".into();
    btc.currency = "USD".into();
    btc.price = 80785.0;
    btc.change_pct = 1.4;
    btc.points.reverse();
    m.update(now, Event::Quotes(vec![q, btc]));
    m.tags.clear();
    m
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "preview.html".into());
    let mut m = sample();
    let mut html = String::from(
        "<!doctype html><meta charset=utf-8><style>body{background:#6b7280;font:12px sans-serif;color:#fff;margin:16px}\
         .row{display:flex;flex-wrap:wrap;gap:24px;align-items:flex-start}figure{margin:0}figcaption{margin:4px 0}</style>\
         <svg width=0 height=0><defs><linearGradient id='art' x1='0' y1='0' x2='1' y2='1'><stop offset='0' stop-color='#d9534f'/><stop offset='1' stop-color='#f2a93b'/></linearGradient></defs></svg><div class=row>",
    );
    let mut add = |name: &str, s: &Scene| html += &format!("<figure><figcaption>{name}</figcaption>{}</figure>", svg(s));
    for edge in [Edge::Left, Edge::Top] {
        m.state = IslandState::Orb;
        add(&format!("orb {edge:?}"), &build(&m, edge, &ApproxMeasure));
        m.state = IslandState::Tag;
        m.hover_tag = true;
        add(&format!("tag {edge:?}"), &build(&m, edge, &ApproxMeasure));
        m.state = IslandState::Slips;
        for slip in m.available_slips() {
            m.front = Some(slip);
            add(&format!("{slip:?} {edge:?}"), &build(&m, edge, &ApproxMeasure));
        }
    }
    m.state = IslandState::Slips;
    m.front = Some(ModuleId::Todo);
    m.todo_input = Some("Call the bank about the card".into());
    add("Todo input Left", &build(&m, Edge::Left, &ApproxMeasure));
    html += "</div>";
    std::fs::write(&out, html).expect("write");
    println!("{out}");
}
