//! Where the island sits on screen (SPEC §4.2–4.3). Everything is in screen
//! DIPs; the platform multiplies by the monitor scale.
//!
//! The window is a fixed box against the docked edge, centred on the anchor
//! point. Every state is placed inside that box, so changing state never moves
//! the window — only the visuals inside it animate.

use crate::geometry::{Edge, Rect, Size};
use crate::theme::{BOX_DEPTH, BOX_SPAN, EDGE_GAP};

/// The window rectangle for a dock position inside a monitor work area.
pub fn window_box(edge: Edge, offset: f32, work: Rect) -> Rect {
    let offset = offset.clamp(0.0, 1.0);
    if edge.is_vertical() {
        let depth = BOX_DEPTH.min(work.w);
        let span = BOX_SPAN.min(work.h);
        let anchor = work.y + offset * work.h;
        let y = (anchor - span / 2.0).clamp(work.y, work.bottom() - span);
        let x = if edge == Edge::Left { work.x } else { work.right() - depth };
        Rect::new(x, y, depth, span)
    } else {
        let depth = BOX_DEPTH.min(work.h);
        let span = BOX_SPAN.min(work.w);
        let anchor = work.x + offset * work.w;
        let x = (anchor - span / 2.0).clamp(work.x, work.right() - span);
        let y = if edge == Edge::Top { work.y } else { work.bottom() - depth };
        Rect::new(x, y, span, depth)
    }
}

/// The anchor point (on the edge) in screen DIPs.
pub fn anchor(edge: Edge, offset: f32, work: Rect) -> (f32, f32) {
    let o = offset.clamp(0.0, 1.0);
    match edge {
        Edge::Left => (work.x, work.y + o * work.h),
        Edge::Right => (work.right(), work.y + o * work.h),
        Edge::Top => (work.x + o * work.w, work.y),
        Edge::Bottom => (work.x + o * work.w, work.bottom()),
    }
}

/// Place content of `size` against the edge, centred on the anchor and kept
/// inside `bx`. `flush` puts it directly on the edge (the Seam).
pub fn place(edge: Edge, offset: f32, work: Rect, bx: Rect, size: Size, flush: bool) -> Rect {
    let gap = if flush { 0.0 } else { EDGE_GAP };
    let (ax, ay) = anchor(edge, offset, work);
    let w = size.w.min(bx.w);
    let h = size.h.min(bx.h);
    let (x, y) = match edge {
        Edge::Left => (bx.x + gap, ay - h / 2.0),
        Edge::Right => (bx.right() - gap - w, ay - h / 2.0),
        Edge::Top => (ax - w / 2.0, bx.y + gap),
        Edge::Bottom => (ax - w / 2.0, bx.bottom() - gap - h),
    };
    let x = x.clamp(bx.x, bx.right() - w);
    let y = y.clamp(bx.y, bx.bottom() - h);
    Rect::new(x, y, w, h)
}

/// Convert a screen rect to coordinates local to the window box.
pub fn local(r: Rect, bx: Rect) -> Rect {
    r.translate(-bx.x, -bx.y)
}

/// Edge and offset for a drop point (after dragging the orb).
pub fn nearest_edge(work: Rect, x: f32, y: f32) -> (Edge, f32) {
    let d = [
        (Edge::Left, (x - work.x).abs()),
        (Edge::Right, (work.right() - x).abs()),
        (Edge::Top, (y - work.y).abs()),
        (Edge::Bottom, (work.bottom() - y).abs()),
    ];
    let edge = d.iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|e| e.0).unwrap_or(Edge::Left);
    let offset = if edge.is_vertical() { (y - work.y) / work.h.max(1.0) } else { (x - work.x) / work.w.max(1.0) };
    // Round to 2 decimals so the config file stays readable.
    (edge, (offset.clamp(0.0, 1.0) * 100.0).round() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect::new(0.0, 0.0, 1920.0, 1032.0);

    #[test]
    fn box_stays_in_work_area_on_every_edge() {
        for edge in Edge::ALL {
            for o in [0.0, 0.3, 0.5, 1.0] {
                let b = window_box(edge, o, WORK);
                assert!(WORK.encloses(&b), "{edge:?} {o}: {b:?}");
            }
        }
    }

    #[test]
    fn content_is_inside_box_and_against_edge() {
        let size = Size::new(416.0, 300.0);
        for edge in Edge::ALL {
            let b = window_box(edge, 0.45, WORK);
            let r = place(edge, 0.45, WORK, b, size, false);
            assert!(b.encloses(&r), "{edge:?}");
            match edge {
                Edge::Left => assert_eq!(r.x, EDGE_GAP),
                Edge::Right => assert_eq!(r.right(), WORK.right() - EDGE_GAP),
                Edge::Top => assert_eq!(r.y, EDGE_GAP),
                Edge::Bottom => assert_eq!(r.bottom(), WORK.bottom() - EDGE_GAP),
            }
            let seam = place(edge, 0.45, WORK, b, Size::new(3.0, 92.0), true);
            assert!(b.encloses(&seam));
        }
    }

    #[test]
    fn small_screens_clamp() {
        let tiny = Rect::new(100.0, 50.0, 400.0, 300.0);
        let b = window_box(Edge::Left, 0.9, tiny);
        assert!(tiny.encloses(&b));
        let r = place(Edge::Left, 0.9, tiny, b, Size::new(600.0, 600.0), false);
        assert!(b.encloses(&r));
    }

    #[test]
    fn drop_point_maps_to_nearest_edge() {
        assert_eq!(nearest_edge(WORK, 5.0, 516.0), (Edge::Left, 0.5));
        assert_eq!(nearest_edge(WORK, 1900.0, 103.2), (Edge::Right, 0.1));
        assert_eq!(nearest_edge(WORK, 960.0, 2.0), (Edge::Top, 0.5));
        assert_eq!(nearest_edge(WORK, 480.0, 1030.0), (Edge::Bottom, 0.25));
    }
}
