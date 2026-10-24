//! Basic geometry and colour types shared by layout, view and renderer.
//! All lengths are device-independent pixels (DIPs) unless stated.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    pub const fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn size(&self) -> Size {
        Size::new(self.w, self.h)
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(self.x + d, self.y + d, (self.w - 2.0 * d).max(0.0), (self.h - 2.0 * d).max(0.0))
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// True when `other` lies completely inside `self` (with a small tolerance).
    pub fn encloses(&self, other: &Rect) -> bool {
        const E: f32 = 0.01;
        other.x >= self.x - E && other.y >= self.y - E && other.right() <= self.right() + E && other.bottom() <= self.bottom() + E
    }

    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect::new(x, y, self.right().max(other.right()) - x, self.bottom().max(other.bottom()) - y)
    }
}

/// sRGB colour, packed as 0xRRGGBBAA.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color(pub u32);

impl Color {
    pub const fn rgb(hex: u32) -> Self {
        Color((hex << 8) | 0xFF)
    }

    pub const fn with_alpha(self, a: u8) -> Self {
        Color((self.0 & 0xFFFF_FF00) | a as u32)
    }

    pub fn r(self) -> u8 {
        (self.0 >> 24) as u8
    }
    pub fn g(self) -> u8 {
        (self.0 >> 16) as u8
    }
    pub fn b(self) -> u8 {
        (self.0 >> 8) as u8
    }
    pub fn a(self) -> u8 {
        self.0 as u8
    }

    /// Components as floats in 0..=1 (straight alpha).
    pub fn to_f32(self) -> [f32; 4] {
        [self.r() as f32 / 255.0, self.g() as f32 / 255.0, self.b() as f32 / 255.0, self.a() as f32 / 255.0]
    }

    /// WCAG relative luminance.
    pub fn luminance(self) -> f32 {
        fn lin(c: u8) -> f32 {
            let c = c as f32 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * lin(self.r()) + 0.7152 * lin(self.g()) + 0.0722 * lin(self.b())
    }

    /// WCAG contrast ratio between two opaque colours.
    pub fn contrast(self, other: Color) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

/// The screen edge the island docks to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    pub const ALL: [Edge; 4] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];

    pub fn name(self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Right => "right",
            Edge::Top => "top",
            Edge::Bottom => "bottom",
        }
    }

    pub fn is_vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains_and_union() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(a.contains(0.0, 0.0));
        assert!(!a.contains(10.0, 5.0));
        let b = Rect::new(5.0, 5.0, 10.0, 2.0);
        assert_eq!(a.union(&b), Rect::new(0.0, 0.0, 15.0, 10.0));
        assert!(a.union(&b).encloses(&a));
    }

    #[test]
    fn color_channels() {
        let c = Color::rgb(0xF2A93B);
        assert_eq!((c.r(), c.g(), c.b(), c.a()), (0xF2, 0xA9, 0x3B, 0xFF));
        assert_eq!(c.with_alpha(0x80).a(), 0x80);
    }
}
