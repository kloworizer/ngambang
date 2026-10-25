//! Colour tokens and fixed metrics from SPEC §3.

use crate::geometry::Color;

/// The island's base: every raised and inset shape is the same material.
pub const SURFACE: Color = Color::rgb(0x212428);
/// Inset wells (tiles, fields, grooves).
pub const SURFACE_2: Color = Color::rgb(0x1B1D21);
/// A raised control's face.
pub const SURFACE_3: Color = Color::rgb(0x25292E);
/// Soft shadows: dark falls bottom-right, light top-left (neumorphism).
pub const SHADOW_DARK: Color = Color::rgb(0x0E1012);
pub const SHADOW_LIGHT: Color = Color::rgb(0x363B42);
pub const HAIRLINE: Color = Color(0xFFFFFF14);
pub const TRACK: Color = Color::rgb(0x3A3F45);
pub const TEXT: Color = Color::rgb(0xECEDEE);
pub const TEXT_2: Color = Color::rgb(0x9BA1A8);
pub const LABEL: Color = Color::rgb(0x8E959C);

pub const MEDIA: Color = Color::rgb(0xF2A93B);
pub const PRIVACY: Color = Color::rgb(0xFF6B4A);
pub const PRIVACY_TEXT: Color = Color::rgb(0xFF8A70);
pub const GAME: Color = Color::rgb(0xB7E04A);
pub const OK: Color = Color::rgb(0x7FD1AE);
pub const CODER: Color = Color::rgb(0xA99BFF);
pub const PORTS: Color = Color::rgb(0x4CC3D9);

/// Gap between the screen edge and the orb / tag / slip.
pub const EDGE_GAP: f32 = 8.0;
pub const ORB_BOX: f32 = 44.0;
pub const ORB_R: f32 = 14.0;
pub const RING_R: f32 = 11.0;
pub const RING_W: f32 = 2.0;
pub const TAG_H: f32 = 40.0;
pub const TAG_MAX_W: f32 = 360.0;
pub const SLIP_W: f32 = 372.0;
/// Height of one row of tabs under the slip.
pub const TAB_ROW_H: f32 = 40.0;
/// A now-playing tag is wider: it carries the player controls.
pub const MEDIA_TAG_MAX_W: f32 = 420.0;
pub const SEAM_T: f32 = 3.0;
pub const SEAM_L: f32 = 92.0;
/// The fixed window box around the anchor (SPEC §4.2): every state fits inside.
pub const BOX_DEPTH: f32 = 580.0;
pub const BOX_SPAN: f32 = 640.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_tokens_meet_contrast() {
        for c in [TEXT, TEXT_2, LABEL, MEDIA, PRIVACY_TEXT, GAME, OK, CODER, PORTS] {
            assert!(c.contrast(SURFACE) >= 4.5, "{:08x} on surface: {}", c.0, c.contrast(SURFACE));
        }
        // Labels also sit on inner tiles.
        for c in [TEXT, TEXT_2, LABEL] {
            assert!(c.contrast(SURFACE_2) >= 4.5, "{:08x} on surface-2", c.0);
        }
    }
}
