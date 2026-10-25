//! Neumorphic shapes built from plain primitives: a raised shape casts a dark
//! shadow down-right and a light one up-left; an inset well is shaded the
//! other way round. Layers of low alpha stand in for a blur, so the renderer
//! needs nothing special and scenes stay cheap to draw.

use crate::geometry::{Color, Rect};
use crate::theme::{SHADOW_DARK, SHADOW_LIGHT, SURFACE_2};
use crate::view::Prim;

/// Alpha of each shadow layer, outermost first.
const LAYERS: [(f32, u8); 4] = [(3.0, 0x0E), (2.0, 0x1C), (1.0, 0x30), (0.0, 0x4C)];

fn rr(rect: Rect, radius: f32, fill: Color) -> Prim {
    Prim::RoundRect { rect, radius: radius.max(0.0), fill: Some(fill), stroke: None }
}

/// How far a raised shape's shadow reaches past it, for `depth`.
pub fn reach(depth: f32) -> f32 {
    depth * 0.5 + 3.0 * depth * 0.35
}

/// A raised shape: soft shadows, then the face.
pub fn raised(rect: Rect, radius: f32, face: Color, depth: f32) -> Vec<Prim> {
    raised_with(rect, radius, face, depth, SHADOW_DARK, SHADOW_LIGHT)
}

/// A raised shape with its own shadow colours (light surfaces use a grey and white).
pub fn raised_with(rect: Rect, radius: f32, face: Color, depth: f32, dark: Color, light: Color) -> Vec<Prim> {
    let o = depth * 0.5;
    let mut v = Vec::with_capacity(9);
    for (step, a) in LAYERS {
        let spread = step * depth * 0.35;
        v.push(rr(rect.translate(-o, -o).inset(-spread), radius + spread, light.with_alpha((a as f32 * 0.7) as u8)));
        v.push(rr(rect.translate(o, o).inset(-spread), radius + spread, dark.with_alpha(a)));
    }
    v.push(rr(rect, radius, face));
    v
}

/// A raised shape whose face is a gradient (lit top-left to deeper bottom-right).
pub fn raised_grad(rect: Rect, radius: f32, from: Color, to: Color, depth: f32) -> Vec<Prim> {
    let mut v = raised(rect, radius, from, depth);
    v.pop();
    v.push(Prim::Gradient { rect, radius: radius.max(0.0), from, to, diagonal: true });
    v
}

/// A raised circle.
pub fn raised_circle(cx: f32, cy: f32, r: f32, face: Color, depth: f32) -> Vec<Prim> {
    let o = depth * 0.5;
    let mut v = Vec::with_capacity(9);
    for (step, a) in LAYERS {
        let spread = step * depth * 0.35;
        let light = SHADOW_LIGHT.with_alpha((a as f32 * 0.7) as u8);
        v.push(Prim::Circle { cx: cx - o, cy: cy - o, r: r + spread, fill: Some(light), stroke: None });
        v.push(Prim::Circle { cx: cx + o, cy: cy + o, r: r + spread, fill: Some(SHADOW_DARK.with_alpha(a)), stroke: None });
    }
    v.push(Prim::Circle { cx, cy, r, fill: Some(face), stroke: None });
    v
}

/// An inset well: darker top-left inner edge, a faint light lip bottom-right.
pub fn well(rect: Rect, radius: f32) -> Vec<Prim> {
    if rect.w < 4.0 || rect.h < 4.0 {
        return vec![rr(rect, radius, SURFACE_2)];
    }
    vec![
        rr(rect, radius, SHADOW_LIGHT.with_alpha(0xB0)),
        rr(Rect::new(rect.x, rect.y, rect.w - 1.0, rect.h - 1.0), radius, Color::rgb(0x121417)),
        rr(Rect::new(rect.x + 1.0, rect.y + 1.0, rect.w - 1.5, rect.h - 1.5), radius - 0.5, Color::rgb(0x17191C)),
        rr(Rect::new(rect.x + 2.0, rect.y + 2.0, rect.w - 3.0, rect.h - 3.0), radius - 1.5, SURFACE_2),
    ]
}

/// An inset round well.
pub fn well_circle(cx: f32, cy: f32, r: f32) -> Vec<Prim> {
    let c = |cx: f32, cy: f32, r: f32, fill: Color| Prim::Circle { cx, cy, r, fill: Some(fill), stroke: None };
    vec![
        c(cx, cy, r, SHADOW_LIGHT.with_alpha(0xB0)),
        c(cx - 0.5, cy - 0.5, r - 0.5, Color::rgb(0x121417)),
        c(cx + 0.25, cy + 0.25, r - 1.0, Color::rgb(0x17191C)),
        c(cx + 0.5, cy + 0.5, r - 1.75, SURFACE_2),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadows_stay_within_reach() {
        let r = Rect::new(20.0, 20.0, 40.0, 30.0);
        for depth in [2.0, 4.0] {
            let outer = r.inset(-reach(depth) - 0.01);
            for p in raised(r, 8.0, SURFACE_2, depth) {
                assert!(outer.encloses(&p.bounds()), "{depth}: {:?}", p.bounds());
            }
        }
        for p in well(r, 8.0) {
            assert!(r.encloses(&p.bounds()));
        }
    }
}
