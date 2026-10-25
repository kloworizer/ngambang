//! Line icons drawn from primitives on a 24×24 grid (no image assets).

use crate::geometry::{Color, Point, Rect};
use crate::model::Icon;
use crate::view::Prim;

/// Points along a circular arc; angles in degrees, 0° = 3 o'clock, clockwise.
pub fn arc_points(cx: f32, cy: f32, r: f32, from: f32, to: f32, segments: usize) -> Vec<Point> {
    let n = segments.max(1);
    (0..=n)
        .map(|i| {
            let a = (from + (to - from) * i as f32 / n as f32).to_radians();
            Point::new(cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// The tide wave: a sine line of `width` centred on `cy`.
pub fn wave_points(x: f32, cy: f32, width: f32, amp: f32) -> Vec<Point> {
    const PERIOD: f32 = 11.0;
    let steps = (width / 1.5).ceil().max(1.0) as usize;
    (0..=steps)
        .map(|i| {
            let dx = width * i as f32 / steps as f32;
            Point::new(x + dx, cy - amp * (dx / PERIOD * std::f32::consts::TAU).sin())
        })
        .collect()
}

pub fn icon_prims(icon: Icon, rect: Rect, color: Color) -> Vec<Prim> {
    let s = rect.w.min(rect.h) / 24.0;
    let ox = rect.x + (rect.w - 24.0 * s) / 2.0;
    let oy = rect.y + (rect.h - 24.0 * s) / 2.0;
    let p = |x: f32, y: f32| Point::new(ox + x * s, oy + y * s);
    let w = (2.0 * s).max(1.4);
    let line = |pts: &[(f32, f32)]| Prim::Polyline { points: pts.iter().map(|&(x, y)| p(x, y)).collect(), width: w, color, closed: false };
    let closed = |pts: &[(f32, f32)]| Prim::Polyline { points: pts.iter().map(|&(x, y)| p(x, y)).collect(), width: w, color, closed: true };
    let fill = |pts: &[(f32, f32)]| Prim::Polygon { points: pts.iter().map(|&(x, y)| p(x, y)).collect(), fill: color };
    let rr = |x: f32, y: f32, ww: f32, hh: f32, r: f32| Prim::RoundRect {
        rect: Rect::new(ox + x * s, oy + y * s, ww * s, hh * s),
        radius: r * s,
        fill: None,
        stroke: Some((color, w)),
    };
    let rr_fill = |x: f32, y: f32, ww: f32, hh: f32, r: f32| Prim::RoundRect {
        rect: Rect::new(ox + x * s, oy + y * s, ww * s, hh * s),
        radius: r * s,
        fill: Some(color),
        stroke: None,
    };
    let circle = |x: f32, y: f32, r: f32| Prim::Circle { cx: ox + x * s, cy: oy + y * s, r: r * s, fill: None, stroke: Some((color, w)) };
    let dot = |x: f32, y: f32, r: f32| Prim::Circle { cx: ox + x * s, cy: oy + y * s, r: r * s, fill: Some(color), stroke: None };
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| Prim::Polyline {
        points: arc_points(ox + cx * s, oy + cy * s, r * s, a0, a1, 16),
        width: w,
        color,
        closed: false,
    };

    match icon {
        Icon::Note => vec![line(&[(9.0, 18.0), (9.0, 5.0), (19.0, 3.0), (19.0, 16.0)]), circle(6.5, 18.0, 2.5), circle(16.5, 16.0, 2.5)],
        Icon::Play => vec![fill(&[(8.0, 5.0), (19.0, 12.0), (8.0, 19.0)])],
        Icon::Pause => vec![rr_fill(6.5, 5.0, 4.0, 14.0, 1.0), rr_fill(13.5, 5.0, 4.0, 14.0, 1.0)],
        Icon::Prev => vec![line(&[(6.0, 5.0), (6.0, 19.0)]), fill(&[(19.0, 5.0), (9.0, 12.0), (19.0, 19.0)])],
        Icon::Next => vec![line(&[(18.0, 5.0), (18.0, 19.0)]), fill(&[(5.0, 5.0), (15.0, 12.0), (5.0, 19.0)])],
        Icon::Speaker => {
            vec![closed(&[(4.0, 9.0), (8.0, 9.0), (13.0, 5.0), (13.0, 19.0), (8.0, 15.0), (4.0, 15.0)]), arc(14.0, 12.0, 4.5, -50.0, 50.0)]
        }
        Icon::SpeakerMuted => vec![
            closed(&[(4.0, 9.0), (8.0, 9.0), (13.0, 5.0), (13.0, 19.0), (8.0, 15.0), (4.0, 15.0)]),
            line(&[(16.0, 9.0), (21.0, 15.0)]),
            line(&[(21.0, 9.0), (16.0, 15.0)]),
        ],
        Icon::Mic => vec![rr(9.0, 3.0, 6.0, 11.0, 3.0), arc(12.0, 11.0, 7.0, 0.0, 180.0), line(&[(12.0, 18.0), (12.0, 21.0)])],
        Icon::Camera => vec![rr(3.0, 6.0, 13.0, 12.0, 2.0), closed(&[(16.0, 10.0), (21.0, 7.0), (21.0, 17.0), (16.0, 14.0)])],
        Icon::Check => vec![line(&[(5.0, 12.5), (10.0, 17.0), (19.0, 7.0)])],
        Icon::Cross => vec![line(&[(6.0, 6.0), (18.0, 18.0)]), line(&[(18.0, 6.0), (6.0, 18.0)])],
        Icon::Clock => vec![circle(12.0, 13.0, 8.0), line(&[(12.0, 9.0), (12.0, 13.0), (15.0, 15.0)]), line(&[(9.0, 2.0), (15.0, 2.0)])],
        Icon::Open => vec![
            line(&[(14.0, 4.0), (20.0, 4.0), (20.0, 10.0)]),
            line(&[(20.0, 4.0), (11.0, 13.0)]),
            line(&[(18.0, 14.0), (18.0, 20.0), (4.0, 20.0), (4.0, 6.0), (10.0, 6.0)]),
        ],
        Icon::Stop => vec![rr(6.0, 6.0, 12.0, 12.0, 2.0)],
        Icon::Terminal => vec![line(&[(5.0, 8.0), (9.0, 12.0), (5.0, 16.0)]), line(&[(12.0, 16.0), (19.0, 16.0)])],
        Icon::Game => vec![
            rr(2.0, 7.0, 20.0, 11.0, 5.0),
            line(&[(6.0, 12.5), (10.0, 12.5)]),
            line(&[(8.0, 10.5), (8.0, 14.5)]),
            dot(15.0, 11.5, 1.2),
            dot(17.5, 14.0, 1.2),
        ],
        Icon::Folder => vec![closed(&[(3.0, 6.0), (9.0, 6.0), (11.0, 8.0), (21.0, 8.0), (21.0, 19.0), (3.0, 19.0)])],
        Icon::Copy => vec![rr(8.0, 8.0, 12.0, 12.0, 2.0), line(&[(16.0, 8.0), (16.0, 4.0), (4.0, 4.0), (4.0, 16.0), (8.0, 16.0)])],
        Icon::Chevron => vec![line(&[(14.0, 6.0), (8.0, 12.0), (14.0, 18.0)])],
        Icon::Image => vec![
            rr(3.0, 5.0, 18.0, 14.0, 2.0),
            line(&[(3.0, 16.0), (9.0, 11.0), (13.0, 15.0), (16.0, 12.0), (21.0, 16.0)]),
            dot(16.0, 9.0, 1.5),
        ],
        Icon::Plug => vec![
            line(&[(9.0, 3.0), (9.0, 8.0)]),
            line(&[(15.0, 3.0), (15.0, 8.0)]),
            rr(6.0, 8.0, 12.0, 6.0, 2.0),
            line(&[(12.0, 14.0), (12.0, 21.0)]),
        ],
        Icon::Clipboard => vec![rr(5.0, 4.0, 14.0, 17.0, 2.0), rr_fill(9.0, 2.0, 6.0, 4.0, 1.0)],
        // Crescent: an outer arc and an inner arc that meet at the tips.
        Icon::Moon => {
            let mut pts = arc_points(ox + 12.0 * s, oy + 12.0 * s, 8.5 * s, 45.0, 315.0, 24);
            let mut inner = arc_points(ox + 15.0 * s, oy + 12.0 * s, 6.4 * s, 300.0, 60.0 - 360.0, 24);
            pts.append(&mut inner);
            vec![Prim::Polygon { points: pts, fill: color }]
        }
        Icon::Download => {
            vec![line(&[(12.0, 3.0), (12.0, 15.0)]), line(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)]), line(&[(4.0, 20.0), (20.0, 20.0)])]
        }
        Icon::Sun => {
            let mut v = vec![circle(12.0, 12.0, 4.5)];
            for i in 0..8 {
                let ang = (i as f32 * 45.0).to_radians();
                let (c, si) = (ang.cos(), ang.sin());
                v.push(line(&[(12.0 + 7.0 * c, 12.0 + 7.0 * si), (12.0 + 9.5 * c, 12.0 + 9.5 * si)]));
            }
            v
        }
        Icon::Cloud => vec![Prim::Polyline {
            points: [
                arc_points(ox + 8.0 * s, oy + 14.0 * s, 4.0 * s, 90.0, 270.0, 10),
                arc_points(ox + 12.5 * s, oy + 10.0 * s, 5.0 * s, 200.0, 340.0, 12),
                arc_points(ox + 17.0 * s, oy + 14.0 * s, 4.0 * s, 270.0, 450.0, 10),
            ]
            .concat(),
            width: w,
            color,
            closed: true,
        }],
        Icon::Rain => vec![
            Prim::Polyline {
                points: [
                    arc_points(ox + 8.0 * s, oy + 14.0 * s, 4.0 * s, 90.0, 270.0, 10),
                    arc_points(ox + 12.5 * s, oy + 10.0 * s, 5.0 * s, 200.0, 340.0, 12),
                    arc_points(ox + 17.0 * s, oy + 14.0 * s, 4.0 * s, 270.0, 450.0, 10),
                ]
                .concat(),
                width: w,
                color,
                closed: true,
            },
            line(&[(9.0, 20.0), (8.0, 22.5)]),
            line(&[(13.0, 20.0), (12.0, 22.5)]),
            line(&[(17.0, 20.0), (16.0, 22.5)]),
        ],
        Icon::Storm => vec![
            Prim::Polyline {
                points: [
                    arc_points(ox + 8.0 * s, oy + 14.0 * s, 4.0 * s, 90.0, 270.0, 10),
                    arc_points(ox + 12.5 * s, oy + 10.0 * s, 5.0 * s, 200.0, 340.0, 12),
                    arc_points(ox + 17.0 * s, oy + 14.0 * s, 4.0 * s, 270.0, 450.0, 10),
                ]
                .concat(),
                width: w,
                color,
                closed: true,
            },
            fill(&[(13.0, 17.5), (9.5, 22.5), (12.0, 22.5), (10.5, 24.0), (15.0, 19.5), (12.5, 19.5), (14.0, 17.5)]),
        ],
        Icon::Warning => vec![closed(&[(12.0, 3.0), (22.0, 20.5), (2.0, 20.5)]), line(&[(12.0, 9.5), (12.0, 14.0)]), dot(12.0, 17.0, 1.3)],
        Icon::Todo => vec![
            rr(3.0, 4.0, 6.0, 6.0, 1.5),
            line(&[(4.5, 7.0), (5.8, 8.3), (7.8, 5.6)]),
            line(&[(12.0, 7.0), (21.0, 7.0)]),
            rr(3.0, 14.0, 6.0, 6.0, 1.5),
            line(&[(12.0, 17.0), (21.0, 17.0)]),
        ],
        Icon::Plus => vec![line(&[(12.0, 5.0), (12.0, 19.0)]), line(&[(5.0, 12.0), (19.0, 12.0)])],
        Icon::Minus => vec![line(&[(5.0, 12.0), (19.0, 12.0)])],
        Icon::Gear => {
            // Eight teeth around a ring.
            let mut v = vec![circle(12.0, 12.0, 5.5), circle(12.0, 12.0, 2.0)];
            for i in 0..8 {
                let a = (i as f32 * 45.0).to_radians();
                v.push(line(&[(12.0 + 6.5 * a.cos(), 12.0 + 6.5 * a.sin()), (12.0 + 9.0 * a.cos(), 12.0 + 9.0 * a.sin())]));
            }
            v
        }
        Icon::Pin => vec![
            Prim::Polyline {
                points: arc_points(ox + 12.0 * s, oy + 9.5 * s, 6.5 * s, 150.0, 390.0, 20)
                    .into_iter()
                    .chain(std::iter::once(p(12.0, 21.0)))
                    .collect(),
                width: w,
                color,
                closed: true,
            },
            circle(12.0, 9.5, 2.3),
        ],
        Icon::Calendar => vec![
            rr(3.5, 5.0, 17.0, 15.5, 2.5),
            line(&[(3.5, 10.0), (20.5, 10.0)]),
            line(&[(8.0, 3.0), (8.0, 7.0)]),
            line(&[(16.0, 3.0), (16.0, 7.0)]),
            dot(8.5, 14.5, 1.3),
            dot(12.0, 14.5, 1.3),
            dot(15.5, 14.5, 1.3),
        ],
        Icon::Chart => vec![line(&[(3.0, 17.0), (9.0, 11.0), (13.0, 14.5), (21.0, 6.0)]), line(&[(16.0, 6.0), (21.0, 6.0), (21.0, 11.0)])],
        Icon::Refresh => vec![arc(12.0, 12.0, 7.5, -60.0, 230.0), fill(&[(17.5, 3.5), (19.6, 9.0), (14.0, 8.0)])],
        Icon::Grid => {
            vec![rr(3.5, 3.5, 7.0, 7.0, 1.5), rr(13.5, 3.5, 7.0, 7.0, 1.5), rr(3.5, 13.5, 7.0, 7.0, 1.5), rr(13.5, 13.5, 7.0, 7.0, 1.5)]
        }
        Icon::Trash => vec![
            line(&[(4.0, 6.5), (20.0, 6.5)]),
            line(&[(9.5, 6.5), (9.5, 3.5), (14.5, 3.5), (14.5, 6.5)]),
            closed(&[(6.0, 6.5), (7.0, 20.5), (17.0, 20.5), (18.0, 6.5)]),
            line(&[(10.0, 10.5), (10.0, 16.5)]),
            line(&[(14.0, 10.5), (14.0, 16.5)]),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Icon; 39] = [
        Icon::Note,
        Icon::Play,
        Icon::Pause,
        Icon::Prev,
        Icon::Next,
        Icon::Speaker,
        Icon::SpeakerMuted,
        Icon::Mic,
        Icon::Camera,
        Icon::Check,
        Icon::Cross,
        Icon::Clock,
        Icon::Open,
        Icon::Stop,
        Icon::Terminal,
        Icon::Game,
        Icon::Folder,
        Icon::Copy,
        Icon::Chevron,
        Icon::Image,
        Icon::Plug,
        Icon::Clipboard,
        Icon::Moon,
        Icon::Download,
        Icon::Sun,
        Icon::Cloud,
        Icon::Rain,
        Icon::Storm,
        Icon::Warning,
        Icon::Todo,
        Icon::Plus,
        Icon::Minus,
        Icon::Trash,
        Icon::Gear,
        Icon::Pin,
        Icon::Grid,
        Icon::Calendar,
        Icon::Chart,
        Icon::Refresh,
    ];

    #[test]
    fn icons_stay_inside_their_box() {
        let r = Rect::new(10.0, 20.0, 16.0, 16.0);
        let outer = r.inset(-2.0);
        for icon in ALL {
            for prim in icon_prims(icon, r, Color::rgb(0xFFFFFF)) {
                assert!(outer.encloses(&prim.bounds()), "{icon:?}: {:?}", prim.bounds());
            }
        }
    }

    #[test]
    fn arcs_and_waves() {
        let a = arc_points(0.0, 0.0, 10.0, 0.0, 90.0, 4);
        assert_eq!(a.len(), 5);
        assert!((a[4].y - 10.0).abs() < 1e-4);
        let w = wave_points(0.0, 8.0, 132.0, 3.5);
        assert!((w.last().unwrap().x - 132.0).abs() < 1e-3);
        assert!(w.iter().all(|p| (p.y - 8.0).abs() <= 3.5 + 1e-3));
    }
}
