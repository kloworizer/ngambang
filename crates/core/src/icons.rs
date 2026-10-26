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

/// Icons are duotone: a soft translucent fill of the colour under a crisp
/// outline, so they read at a glance and stay light.
pub fn icon_prims(icon: Icon, rect: Rect, color: Color) -> Vec<Prim> {
    let s = rect.w.min(rect.h) / 24.0;
    let ox = rect.x + (rect.w - 24.0 * s) / 2.0;
    let oy = rect.y + (rect.h - 24.0 * s) / 2.0;
    let p = |x: f32, y: f32| Point::new(ox + x * s, oy + y * s);
    let w = (1.9 * s).max(1.3);
    // The fill: the same colour, faint.
    let tone = color.with_alpha(((color.a() as u32 * 0x48) / 0xFF) as u8);
    let pts = |v: &[(f32, f32)]| v.iter().map(|&(x, y)| p(x, y)).collect::<Vec<_>>();
    let line = |v: &[(f32, f32)]| Prim::Polyline { points: pts(v), width: w, color, closed: false };
    let closed = |v: &[(f32, f32)]| Prim::Polyline { points: pts(v), width: w, color, closed: true };
    let fill = |v: &[(f32, f32)]| Prim::Polygon { points: pts(v), fill: color };
    let soft = |v: &[(f32, f32)]| Prim::Polygon { points: pts(v), fill: tone };
    let rect_at = |x: f32, y: f32, ww: f32, hh: f32| Rect::new(ox + x * s, oy + y * s, ww * s, hh * s);
    let rr = |x: f32, y: f32, ww: f32, hh: f32, r: f32| Prim::RoundRect {
        rect: rect_at(x, y, ww, hh),
        radius: r * s,
        fill: None,
        stroke: Some((color, w)),
    };
    // A shape filled faintly and outlined.
    let duo = |x: f32, y: f32, ww: f32, hh: f32, r: f32| Prim::RoundRect {
        rect: rect_at(x, y, ww, hh),
        radius: r * s,
        fill: Some(tone),
        stroke: Some((color, w)),
    };
    let rr_fill = |x: f32, y: f32, ww: f32, hh: f32, r: f32| Prim::RoundRect {
        rect: rect_at(x, y, ww, hh),
        radius: r * s,
        fill: Some(color),
        stroke: None,
    };
    let circle = |x: f32, y: f32, r: f32| Prim::Circle { cx: ox + x * s, cy: oy + y * s, r: r * s, fill: None, stroke: Some((color, w)) };
    let duo_circle =
        |x: f32, y: f32, r: f32| Prim::Circle { cx: ox + x * s, cy: oy + y * s, r: r * s, fill: Some(tone), stroke: Some((color, w)) };
    let dot = |x: f32, y: f32, r: f32| Prim::Circle { cx: ox + x * s, cy: oy + y * s, r: r * s, fill: Some(color), stroke: None };
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| Prim::Polyline {
        points: arc_points(ox + cx * s, oy + cy * s, r * s, a0, a1, 16),
        width: w,
        color,
        closed: false,
    };
    let cloud = |dy: f32| -> Vec<Point> {
        [
            arc_points(ox + 7.5 * s, oy + (14.0 + dy) * s, 3.8 * s, 90.0, 270.0, 10),
            arc_points(ox + 12.5 * s, oy + (10.0 + dy) * s, 5.2 * s, 195.0, 345.0, 12),
            arc_points(ox + 17.0 * s, oy + (14.0 + dy) * s, 4.0 * s, 270.0, 450.0, 10),
        ]
        .concat()
    };
    let cloud_prims = |dy: f32| {
        let c = cloud(dy);
        vec![Prim::Polygon { points: c.clone(), fill: tone }, Prim::Polyline { points: c, width: w, color, closed: true }]
    };
    let speaker = [(3.5, 9.0), (7.5, 9.0), (12.5, 4.5), (12.5, 19.5), (7.5, 15.0), (3.5, 15.0)];

    match icon {
        Icon::Note => vec![
            soft(&[(9.0, 5.5), (19.0, 3.0), (19.0, 7.5), (9.0, 10.0)]),
            line(&[(9.0, 17.5), (9.0, 5.5), (19.0, 3.0), (19.0, 15.5)]),
            dot(6.5, 17.5, 2.9),
            dot(16.5, 15.5, 2.9),
        ],
        Icon::Play => vec![fill(&[(7.5, 4.5), (19.5, 12.0), (7.5, 19.5)])],
        Icon::Pause => vec![rr_fill(6.0, 4.5, 4.5, 15.0, 1.5), rr_fill(13.5, 4.5, 4.5, 15.0, 1.5)],
        Icon::Prev => vec![rr_fill(4.5, 5.0, 2.5, 14.0, 1.0), fill(&[(19.5, 5.0), (9.0, 12.0), (19.5, 19.0)])],
        Icon::Next => vec![rr_fill(17.0, 5.0, 2.5, 14.0, 1.0), fill(&[(4.5, 5.0), (15.0, 12.0), (4.5, 19.0)])],
        Icon::Speaker => vec![soft(&speaker), closed(&speaker), arc(13.0, 12.0, 3.8, -48.0, 48.0), arc(13.0, 12.0, 7.5, -48.0, 48.0)],
        Icon::SpeakerMuted => {
            vec![soft(&speaker), closed(&speaker), line(&[(16.0, 9.0), (21.0, 15.0)]), line(&[(21.0, 9.0), (16.0, 15.0)])]
        }
        Icon::Mic => vec![
            duo(8.5, 2.5, 7.0, 12.0, 3.5),
            arc(12.0, 10.5, 7.0, 15.0, 165.0),
            line(&[(12.0, 17.5), (12.0, 21.0)]),
            line(&[(8.5, 21.0), (15.5, 21.0)]),
        ],
        Icon::Camera => vec![
            duo(2.5, 6.0, 14.0, 12.0, 3.0),
            soft(&[(16.5, 10.0), (21.5, 7.0), (21.5, 17.0), (16.5, 14.0)]),
            closed(&[(16.5, 10.0), (21.5, 7.0), (21.5, 17.0), (16.5, 14.0)]),
            dot(6.5, 10.0, 1.2),
        ],
        Icon::Check => vec![line(&[(5.0, 12.5), (10.0, 17.0), (19.0, 7.0)])],
        Icon::Cross => vec![line(&[(6.0, 6.0), (18.0, 18.0)]), line(&[(18.0, 6.0), (6.0, 18.0)])],
        Icon::Clock => {
            vec![duo_circle(12.0, 13.0, 8.5), line(&[(12.0, 8.5), (12.0, 13.0), (15.0, 15.0)]), line(&[(9.5, 2.0), (14.5, 2.0)])]
        }
        Icon::Open => {
            vec![duo(3.5, 6.0, 14.5, 14.5, 3.0), line(&[(14.0, 3.5), (20.5, 3.5), (20.5, 10.0)]), line(&[(20.5, 3.5), (11.5, 12.5)])]
        }
        Icon::Stop => vec![duo(5.5, 5.5, 13.0, 13.0, 3.0)],
        Icon::Terminal => {
            vec![duo(2.5, 4.0, 19.0, 16.0, 3.5), line(&[(6.5, 9.0), (9.5, 12.0), (6.5, 15.0)]), line(&[(12.0, 15.0), (17.0, 15.0)])]
        }
        Icon::Game => vec![
            duo(2.0, 7.0, 20.0, 11.5, 5.5),
            line(&[(6.0, 12.75), (10.0, 12.75)]),
            line(&[(8.0, 10.75), (8.0, 14.75)]),
            dot(15.0, 11.5, 1.3),
            dot(17.5, 14.0, 1.3),
        ],
        Icon::Folder => {
            let f = [(2.5, 6.0), (9.0, 6.0), (11.0, 8.0), (21.5, 8.0), (21.5, 19.0), (2.5, 19.0)];
            vec![soft(&f), closed(&f)]
        }
        Icon::Copy => vec![rr(3.5, 3.5, 12.0, 12.0, 2.5), duo(8.5, 8.5, 12.0, 12.0, 2.5)],
        Icon::Chevron => vec![line(&[(14.0, 6.0), (8.0, 12.0), (14.0, 18.0)])],
        Icon::Image => vec![
            duo(2.5, 4.5, 19.0, 15.0, 3.0),
            line(&[(3.0, 16.0), (9.0, 11.0), (13.0, 15.0), (16.0, 12.5), (21.0, 16.5)]),
            dot(16.0, 8.5, 1.6),
        ],
        Icon::Plug => vec![
            line(&[(9.0, 2.5), (9.0, 7.5)]),
            line(&[(15.0, 2.5), (15.0, 7.5)]),
            duo(5.5, 7.5, 13.0, 7.5, 3.0),
            line(&[(12.0, 15.0), (12.0, 21.5)]),
        ],
        Icon::Clipboard => vec![
            duo(4.5, 4.0, 15.0, 17.5, 3.0),
            rr_fill(8.5, 2.0, 7.0, 4.0, 1.5),
            line(&[(8.5, 11.5), (15.5, 11.5)]),
            line(&[(8.5, 15.5), (13.0, 15.5)]),
        ],
        // Crescent: an outer arc and an inner arc that meet at the tips, and a star.
        Icon::Moon => {
            let mut pts = arc_points(ox + 11.0 * s, oy + 12.5 * s, 8.5 * s, 45.0, 315.0, 24);
            let mut inner = arc_points(ox + 14.0 * s, oy + 12.5 * s, 6.4 * s, 300.0, 60.0 - 360.0, 24);
            pts.append(&mut inner);
            vec![Prim::Polygon { points: pts, fill: color }, dot(19.5, 5.0, 1.3)]
        }
        Icon::Download => {
            vec![line(&[(12.0, 3.0), (12.0, 14.5)]), line(&[(7.0, 9.5), (12.0, 14.5), (17.0, 9.5)]), duo(3.5, 17.0, 17.0, 4.0, 2.0)]
        }
        Icon::Sun => {
            let mut v = vec![duo_circle(12.0, 12.0, 4.6)];
            for i in 0..8 {
                let ang = (i as f32 * 45.0).to_radians();
                let (c, si) = (ang.cos(), ang.sin());
                v.push(line(&[(12.0 + 7.3 * c, 12.0 + 7.3 * si), (12.0 + 9.6 * c, 12.0 + 9.6 * si)]));
            }
            v
        }
        Icon::Cloud => cloud_prims(1.0),
        Icon::Rain => {
            let mut v = cloud_prims(-1.0);
            v.extend([line(&[(9.0, 18.5), (8.0, 21.5)]), line(&[(13.0, 18.5), (12.0, 21.5)]), line(&[(17.0, 18.5), (16.0, 21.5)])]);
            v
        }
        Icon::Storm => {
            let mut v = cloud_prims(-1.5);
            v.push(fill(&[(13.5, 15.5), (9.5, 21.0), (12.0, 21.0), (10.5, 23.5), (15.5, 18.0), (12.8, 18.0), (14.5, 15.5)]));
            v
        }
        Icon::Warning => {
            let t = [(12.0, 3.0), (22.0, 20.5), (2.0, 20.5)];
            vec![soft(&t), closed(&t), line(&[(12.0, 9.5), (12.0, 14.0)]), dot(12.0, 17.0, 1.3)]
        }
        Icon::Todo => vec![
            duo(3.0, 4.0, 6.5, 6.5, 2.0),
            line(&[(4.6, 7.2), (5.9, 8.5), (8.0, 5.8)]),
            line(&[(12.5, 7.25), (21.0, 7.25)]),
            duo(3.0, 13.5, 6.5, 6.5, 2.0),
            line(&[(12.5, 16.75), (21.0, 16.75)]),
        ],
        Icon::Plus => vec![line(&[(12.0, 5.0), (12.0, 19.0)]), line(&[(5.0, 12.0), (19.0, 12.0)])],
        Icon::Minus => vec![line(&[(5.0, 12.0), (19.0, 12.0)])],
        Icon::Gear => {
            // Eight teeth around a ring.
            let mut v = vec![duo_circle(12.0, 12.0, 5.8), circle(12.0, 12.0, 2.0)];
            for i in 0..8 {
                let a = (i as f32 * 45.0 + 22.5).to_radians();
                v.push(line(&[(12.0 + 6.8 * a.cos(), 12.0 + 6.8 * a.sin()), (12.0 + 9.3 * a.cos(), 12.0 + 9.3 * a.sin())]));
            }
            v
        }
        Icon::Pin => {
            let drop: Vec<Point> = arc_points(ox + 12.0 * s, oy + 9.5 * s, 6.5 * s, 150.0, 390.0, 20)
                .into_iter()
                .chain(std::iter::once(p(12.0, 21.0)))
                .collect();
            vec![
                Prim::Polygon { points: drop.clone(), fill: tone },
                Prim::Polyline { points: drop, width: w, color, closed: true },
                circle(12.0, 9.5, 2.3),
            ]
        }
        Icon::Calendar => vec![
            duo(3.0, 4.5, 18.0, 16.5, 3.5),
            rr_fill(3.0, 4.5, 18.0, 5.0, 2.5),
            line(&[(8.0, 2.5), (8.0, 6.0)]),
            line(&[(16.0, 2.5), (16.0, 6.0)]),
            dot(8.5, 14.0, 1.3),
            dot(12.0, 14.0, 1.3),
            dot(15.5, 14.0, 1.3),
            dot(8.5, 17.5, 1.3),
        ],
        Icon::Chart => vec![
            soft(&[(3.0, 17.0), (9.0, 11.0), (13.0, 14.5), (21.0, 6.0), (21.0, 20.5), (3.0, 20.5)]),
            line(&[(3.0, 17.0), (9.0, 11.0), (13.0, 14.5), (21.0, 6.0)]),
            line(&[(16.5, 6.0), (21.0, 6.0), (21.0, 10.5)]),
        ],
        Icon::Refresh => vec![arc(12.0, 12.0, 7.5, -60.0, 230.0), fill(&[(17.5, 3.5), (19.6, 9.0), (14.0, 8.0)])],
        // A pencil, tilted.
        Icon::Pencil => {
            let body = [(4.0, 20.0), (5.0, 15.5), (15.5, 5.0), (19.0, 8.5), (8.5, 19.0)];
            vec![soft(&body), closed(&body), line(&[(13.0, 7.5), (16.5, 11.0)])]
        }
        // A battery on its side, two-thirds full.
        Icon::Battery => vec![duo(2.5, 7.0, 17.0, 10.0, 2.5), rr_fill(5.0, 9.5, 8.5, 5.0, 1.0), line(&[(21.5, 10.5), (21.5, 13.5)])],
        // Three arcs and a dot.
        Icon::Wifi => {
            let arc = |r: f32| Prim::Polyline {
                points: arc_points(ox + 12.0 * s, oy + 19.0 * s, r * s, 225.0, 315.0, 12),
                width: w,
                color,
                closed: false,
            };
            vec![arc(14.0), arc(9.5), arc(5.0), dot(12.0, 19.0, 1.6)]
        }
        Icon::Grid => {
            vec![duo(3.5, 3.5, 7.0, 7.0, 2.0), duo(13.5, 3.5, 7.0, 7.0, 2.0), duo(3.5, 13.5, 7.0, 7.0, 2.0), duo(13.5, 13.5, 7.0, 7.0, 2.0)]
        }
        Icon::Trash => {
            let body = [(6.0, 6.5), (7.0, 20.5), (17.0, 20.5), (18.0, 6.5)];
            vec![
                soft(&body),
                closed(&body),
                line(&[(4.0, 6.5), (20.0, 6.5)]),
                line(&[(9.5, 6.5), (9.5, 3.5), (14.5, 3.5), (14.5, 6.5)]),
                line(&[(10.0, 10.5), (10.0, 16.5)]),
                line(&[(14.0, 10.5), (14.0, 16.5)]),
            ]
        }
        Icon::Drop => {
            let d: Vec<Point> =
                std::iter::once(p(12.0, 2.5)).chain(arc_points(ox + 12.0 * s, oy + 14.5 * s, 6.5 * s, -30.0, 210.0, 20)).collect();
            vec![Prim::Polygon { points: d.clone(), fill: tone }, Prim::Polyline { points: d, width: w, color, closed: true }]
        }
        Icon::Wind => vec![
            Prim::Polyline {
                points: [vec![p(3.0, 9.0), p(15.0, 9.0)], arc_points(ox + 15.0 * s, oy + 6.0 * s, 3.0 * s, 90.0, -150.0, 10)].concat(),
                width: w,
                color,
                closed: false,
            },
            Prim::Polyline {
                points: [vec![p(3.0, 14.5), p(18.0, 14.5)], arc_points(ox + 18.0 * s, oy + 17.5 * s, 3.0 * s, -90.0, 150.0, 10)].concat(),
                width: w,
                color,
                closed: false,
            },
            line(&[(3.0, 19.5), (10.0, 19.5)]),
        ],
        Icon::Thermo => {
            vec![duo(9.5, 2.5, 5.0, 13.0, 2.5), duo_circle(12.0, 17.5, 4.0), dot(12.0, 17.5, 1.8), line(&[(12.0, 9.0), (12.0, 16.0)])]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Icon; 45] = [
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
        Icon::Drop,
        Icon::Wind,
        Icon::Thermo,
        Icon::Pencil,
        Icon::Battery,
        Icon::Wifi,
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
