//! Windows.UI.Composition on a desktop window: each scene is drawn into a
//! surface once, and the compositor (inside DWM) animates the morph between
//! scenes and the tide ring — no CPU work while an animation runs.

use ngambang_core::geometry::{Color, Rect};
use ngambang_core::view::{RingSpec, Scene};
use windows::core::{Interface, Result, HSTRING};
use windows::Foundation::{Size, TimeSpan};
use windows::Graphics::DirectX::{DirectXAlphaMode, DirectXPixelFormat};
use windows::System::DispatcherQueueController;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::ID2D1DeviceContext;
use windows::Win32::System::WinRT::Composition::{ICompositionDrawingSurfaceInterop, ICompositorDesktopInterop, ICompositorInterop};
use windows::Win32::System::WinRT::{CreateDispatcherQueueController, DispatcherQueueOptions, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT};
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::*;
use windows_numerics::{Matrix3x2, Vector2, Vector3};

use super::gfx::Gfx;

/// How a new scene replaces the one on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anim {
    /// Redraw in place.
    None,
    /// Morph the shape from the old scene to the new one.
    Morph,
    /// Morph, and the new scene springs in from slightly smaller (pop-ups).
    Pop,
    /// Same shape, new content: crossfade with a short slide.
    Swap,
    /// The slips slide out of the docked edge like a drawer (`open`), or
    /// back into it. `from` points from the island towards the edge.
    Drawer { open: bool, from: (f32, f32) },
    /// Flipping between slips: the new one slides in from the side it
    /// comes from (`dir` +1 = from the right, −1 = from the left).
    Flip { dir: f32 },
}

/// One scene on screen.
struct Layer {
    sprite: SpriteVisual,
    surface: CompositionDrawingSurface,
    clip: CompositionRoundedRectangleGeometry,
    /// Where it sits, in DIPs local to the window.
    rect: Rect,
    px: (i32, i32),
}

struct Ring {
    visual: ShapeVisual,
    geometry: CompositionEllipseGeometry,
    shape: CompositionSpriteShape,
    last: Option<(RingSpec, Rect, u64)>,
}

pub struct Renderer {
    _queue: DispatcherQueueController,
    compositor: Compositor,
    _target: DesktopWindowTarget,
    root: ContainerVisual,
    device: CompositionGraphicsDevice,
    current: Option<Layer>,
    outgoing: Vec<Layer>,
    ring: Ring,
    /// Click feedback above everything: a dimmer while pressed and a ripple
    /// on release. Made once and reused.
    fx: ContainerVisual,
    press: SpriteVisual,
    press_clip: CompositionRoundedRectangleGeometry,
    ripple: ShapeVisual,
    ripple_clip: CompositionRoundedRectangleGeometry,
    ripple_dot: CompositionEllipseGeometry,
    scale: f32,
    /// Scale the current scene is animating towards (hover and press feedback).
    target_scale: f32,
}

fn ts(ms: u64) -> TimeSpan {
    TimeSpan { Duration: (ms * 10_000) as i64 }
}

fn ui_color(c: Color) -> windows::UI::Color {
    windows::UI::Color { A: c.a(), R: c.r(), G: c.g(), B: c.b() }
}

impl Renderer {
    pub fn new(hwnd: HWND, gfx: &Gfx) -> Result<Renderer> {
        let queue = unsafe {
            CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_STA,
            })?
        };
        let compositor = Compositor::new()?;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd, true)? };
        let root = compositor.CreateContainerVisual()?;
        target.SetRoot(&root)?;
        let device = unsafe { compositor.cast::<ICompositorInterop>()?.CreateGraphicsDevice(&gfx.d2d)? };

        let visual = compositor.CreateShapeVisual()?;
        let geometry = compositor.CreateEllipseGeometry()?;
        let shape = compositor.CreateSpriteShapeWithGeometry(&geometry)?;
        shape.SetStrokeStartCap(CompositionStrokeCap::Round)?;
        shape.SetStrokeEndCap(CompositionStrokeCap::Round)?;
        visual.Shapes()?.Append(&shape)?;
        visual.SetIsVisible(false)?;
        root.Children()?.InsertAtTop(&visual)?;

        let fx = compositor.CreateContainerVisual()?;
        root.Children()?.InsertAtTop(&fx)?;
        let press = compositor.CreateSpriteVisual()?;
        press.SetBrush(&compositor.CreateColorBrushWithColor(ui_color(Color(0x000000FF)))?)?;
        let press_clip = compositor.CreateRoundedRectangleGeometry()?;
        press.SetClip(&compositor.CreateGeometricClipWithGeometry(&press_clip)?)?;
        press.SetOpacity(0.0)?;
        fx.Children()?.InsertAtTop(&press)?;
        let ripple = compositor.CreateShapeVisual()?;
        let ripple_dot = compositor.CreateEllipseGeometry()?;
        let dot = compositor.CreateSpriteShapeWithGeometry(&ripple_dot)?;
        dot.SetFillBrush(&compositor.CreateColorBrushWithColor(ui_color(Color(0xFFFFFF55)))?)?;
        ripple.Shapes()?.Append(&dot)?;
        let ripple_clip = compositor.CreateRoundedRectangleGeometry()?;
        ripple.SetClip(&compositor.CreateGeometricClipWithGeometry(&ripple_clip)?)?;
        ripple.SetOpacity(0.0)?;
        fx.Children()?.InsertAtTop(&ripple)?;

        Ok(Renderer {
            _queue: queue,
            compositor,
            _target: target,
            root,
            device,
            current: None,
            outgoing: Vec::new(),
            ring: Ring { visual, geometry, shape, last: None },
            fx,
            press,
            press_clip,
            ripple,
            ripple_clip,
            ripple_dot,
            scale: 1.0,
            target_scale: 1.0,
        })
    }

    pub fn set_scale(&mut self, scale: f32) -> Result<()> {
        self.scale = scale;
        self.root.SetScale(Vector3 { X: scale, Y: scale, Z: 1.0 })
    }

    /// Size of the root, so the ring visual can be placed anywhere.
    pub fn set_box(&self, w: f32, h: f32) -> Result<()> {
        self.ring.visual.SetSize(Vector2 { X: w, Y: h })
    }

    fn draw(&self, gfx: &Gfx, surface: &CompositionDrawingSurface, scene: &Scene) -> Result<()> {
        let interop: ICompositionDrawingSurfaceInterop = surface.cast()?;
        unsafe {
            let mut offset = POINT::default();
            let dc: ID2D1DeviceContext = interop.BeginDraw(None, &mut offset)?;
            let dpi = 96.0 * self.scale;
            dc.SetDpi(dpi, dpi);
            // The update offset is in pixels; our drawing is in DIPs.
            let t = Matrix3x2::translation(offset.x as f32 / self.scale, offset.y as f32 / self.scale);
            dc.SetTransform(&t);
            dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            let res = gfx.draw(&dc, &scene.prims);
            interop.EndDraw()?;
            res
        }
    }

    /// A spring-like move: an overshooting curve when growing, a calm
    /// ease-out when shrinking (SPEC §3.5).
    fn spring(&self, grow: bool, to: Vector2) -> Result<Vector2KeyFrameAnimation> {
        let a = self.compositor.CreateVector2KeyFrameAnimation()?;
        let (p1, p2, ms) = if grow {
            (Vector2 { X: 0.34, Y: 1.56 }, Vector2 { X: 0.64, Y: 1.0 }, 340)
        } else {
            (Vector2 { X: 0.22, Y: 1.0 }, Vector2 { X: 0.36, Y: 1.0 }, 240)
        };
        let ease = self.compositor.CreateCubicBezierEasingFunction(p1, p2)?;
        a.InsertKeyFrameWithEasingFunction(1.0, to, &ease)?;
        a.SetDuration(ts(ms))?;
        Ok(a)
    }

    fn fade(&self, to: f32, ms: u64) -> Result<ScalarKeyFrameAnimation> {
        let a = self.compositor.CreateScalarKeyFrameAnimation()?;
        a.InsertKeyFrame(1.0, to)?;
        a.SetDuration(ts(ms))?;
        Ok(a)
    }

    /// Scale from `from` to `to` (both axes), overshooting a little when growing.
    fn scale_anim(&self, from: Option<f32>, to: f32, ms: u64) -> Result<Vector3KeyFrameAnimation> {
        let a = self.compositor.CreateVector3KeyFrameAnimation()?;
        let ease = self.compositor.CreateCubicBezierEasingFunction(Vector2 { X: 0.34, Y: 1.4 }, Vector2 { X: 0.64, Y: 1.0 })?;
        if let Some(f) = from {
            a.InsertKeyFrame(0.0, Vector3 { X: f, Y: f, Z: 1.0 })?;
        }
        a.InsertKeyFrameWithEasingFunction(1.0, Vector3 { X: to, Y: to, Z: 1.0 }, &ease)?;
        a.SetDuration(ts(ms))?;
        Ok(a)
    }

    fn slide(&self, from: Vector3, to: Vector3, ms: u64) -> Result<Vector3KeyFrameAnimation> {
        let a = self.compositor.CreateVector3KeyFrameAnimation()?;
        let ease = self.compositor.CreateCubicBezierEasingFunction(Vector2 { X: 0.22, Y: 1.0 }, Vector2 { X: 0.36, Y: 1.0 })?;
        a.InsertKeyFrame(0.0, from)?;
        a.InsertKeyFrameWithEasingFunction(1.0, to, &ease)?;
        a.SetDuration(ts(ms))?;
        Ok(a)
    }

    /// Dim the control under the pointer while it is held (`Some`), or let go.
    /// `rect` is window-local; `radius` its corner.
    pub fn press(&self, at: Option<(Rect, f32)>, reduce_motion: bool) -> Result<()> {
        match at {
            Some((r, radius)) => {
                self.press.SetOffset(Vector3 { X: r.x, Y: r.y, Z: 0.0 })?;
                self.press.SetSize(Vector2 { X: r.w, Y: r.h })?;
                self.press_clip.SetSize(Vector2 { X: r.w, Y: r.h })?;
                self.press_clip.SetCornerRadius(Vector2 { X: radius, Y: radius })?;
                if reduce_motion {
                    return self.press.SetOpacity(0.22);
                }
                self.press.StartAnimation(&HSTRING::from("Opacity"), &self.fade(0.22, 70)?)
            }
            None if reduce_motion => self.press.SetOpacity(0.0),
            None => self.press.StartAnimation(&HSTRING::from("Opacity"), &self.fade(0.0, 220)?),
        }
    }

    /// A ripple spreading from `point` inside `rect` (window-local) after a click.
    pub fn ripple(&self, rect: Rect, radius: f32, point: (f32, f32), reduce_motion: bool) -> Result<()> {
        if reduce_motion {
            return Ok(());
        }
        self.ripple.SetOffset(Vector3 { X: rect.x, Y: rect.y, Z: 0.0 })?;
        self.ripple.SetSize(Vector2 { X: rect.w, Y: rect.h })?;
        self.ripple_clip.SetSize(Vector2 { X: rect.w, Y: rect.h })?;
        self.ripple_clip.SetCornerRadius(Vector2 { X: radius, Y: radius })?;
        self.ripple_dot.SetCenter(Vector2 { X: point.0 - rect.x, Y: point.1 - rect.y })?;
        let reach = (rect.w * rect.w + rect.h * rect.h).sqrt();
        let grow = self.compositor.CreateVector2KeyFrameAnimation()?;
        let ease = self.compositor.CreateCubicBezierEasingFunction(Vector2 { X: 0.2, Y: 0.8 }, Vector2 { X: 0.3, Y: 1.0 })?;
        grow.InsertKeyFrame(0.0, Vector2 { X: 4.0, Y: 4.0 })?;
        grow.InsertKeyFrameWithEasingFunction(1.0, Vector2 { X: reach, Y: reach }, &ease)?;
        grow.SetDuration(ts(420))?;
        self.ripple_dot.StartAnimation(&HSTRING::from("Radius"), &grow)?;
        let fade = self.compositor.CreateScalarKeyFrameAnimation()?;
        fade.InsertKeyFrame(0.0, 1.0)?;
        fade.InsertKeyFrame(1.0, 0.0)?;
        fade.SetDuration(ts(460))?;
        self.ripple.StartAnimation(&HSTRING::from("Opacity"), &fade)
    }

    /// A calm ease-out move of a visual's offset.
    fn glide(&self, from: Vector3, to: Vector3, ms: u64) -> Result<Vector3KeyFrameAnimation> {
        let a = self.compositor.CreateVector3KeyFrameAnimation()?;
        let ease = self.compositor.CreateCubicBezierEasingFunction(Vector2 { X: 0.16, Y: 1.0 }, Vector2 { X: 0.3, Y: 1.0 })?;
        a.InsertKeyFrame(0.0, from)?;
        a.InsertKeyFrameWithEasingFunction(1.0, to, &ease)?;
        a.SetDuration(ts(ms))?;
        Ok(a)
    }

    /// Grow or shrink the current scene a little around its centre: hover and
    /// press feedback. Runs in the compositor; repeated calls are free.
    pub fn feedback(&mut self, scale: f32, reduce_motion: bool) -> Result<()> {
        if (self.target_scale - scale).abs() < 0.001 {
            return Ok(());
        }
        self.target_scale = scale;
        let Some(cur) = &self.current else { return Ok(()) };
        if reduce_motion {
            return cur.sprite.SetScale(Vector3 { X: scale, Y: scale, Z: 1.0 });
        }
        cur.sprite.StartAnimation(&HSTRING::from("Scale"), &self.scale_anim(None, scale, 180)?)
    }

    /// Show `scene` at `rect` (window-local DIPs), replacing the previous
    /// scene as `anim` says.
    pub fn show(&mut self, gfx: &Gfx, scene: &Scene, rect: Rect, anim: Anim, reduce_motion: bool) -> Result<()> {
        let px = ((rect.w * self.scale).ceil().max(1.0) as i32, (rect.h * self.scale).ceil().max(1.0) as i32);
        let animate = matches!(anim, Anim::Morph | Anim::Pop | Anim::Drawer { .. } | Anim::Flip { .. });
        if let Some(cur) = &mut self.current {
            if anim == Anim::None && cur.px == px {
                // Only when it moved: setting the offset would cut a slide short.
                if (cur.rect.x, cur.rect.y) != (rect.x, rect.y) {
                    cur.sprite.SetOffset(Vector3 { X: rect.x, Y: rect.y, Z: 0.0 })?;
                }
                cur.rect = rect;
                let surface = cur.surface.clone();
                return self.draw(gfx, &surface, scene);
            }
        }

        let surface = self.device.CreateDrawingSurface(
            Size { Width: px.0 as f32, Height: px.1 as f32 },
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            DirectXAlphaMode::Premultiplied,
        )?;
        self.draw(gfx, &surface, scene)?;
        let brush = self.compositor.CreateSurfaceBrushWithSurface(&surface)?;
        let sprite = self.compositor.CreateSpriteVisual()?;
        sprite.SetBrush(&brush)?;
        sprite.SetSize(Vector2 { X: rect.w, Y: rect.h })?;
        sprite.SetOffset(Vector3 { X: rect.x, Y: rect.y, Z: 0.0 })?;
        sprite.SetCenterPoint(Vector3 { X: rect.w / 2.0, Y: rect.h / 2.0, Z: 0.0 })?;
        // A new scene starts without hover or press feedback.
        self.target_scale = 1.0;
        let clip = self.compositor.CreateRoundedRectangleGeometry()?;
        clip.SetCornerRadius(Vector2 { X: scene.clip_radius, Y: scene.clip_radius })?;
        sprite.SetClip(&self.compositor.CreateGeometricClipWithGeometry(&clip)?)?;
        let children = self.root.Children()?;
        children.InsertAtTop(&sprite)?;
        // Keep the ring and the click feedback above the content.
        children.Remove(&self.ring.visual)?;
        children.InsertAtTop(&self.ring.visual)?;
        children.Remove(&self.fx)?;
        children.InsertAtTop(&self.fx)?;

        let full = Vector2 { X: rect.w, Y: rect.h };
        let prev = self.current.take();
        match prev {
            Some(prev) if animate && !reduce_motion => {
                // Flips and drawers move calmly; other morphs spring.
                let grow = rect.w * rect.h > prev.rect.w * prev.rect.h && !matches!(anim, Anim::Flip { .. });
                // New scene: the clip starts as the old shape and springs open.
                clip.SetOffset(Vector2 { X: prev.rect.x - rect.x, Y: prev.rect.y - rect.y })?;
                clip.SetSize(Vector2 { X: prev.rect.w, Y: prev.rect.h })?;
                clip.StartAnimation(&HSTRING::from("Offset"), &self.spring(grow, Vector2 { X: 0.0, Y: 0.0 })?)?;
                clip.StartAnimation(&HSTRING::from("Size"), &self.spring(grow, full)?)?;
                sprite.SetOpacity(0.0)?;
                sprite.StartAnimation(&HSTRING::from("Opacity"), &self.fade(1.0, 110)?)?;
                // Old scene: its clip follows the new shape while it fades.
                let to_off = Vector2 { X: rect.x - prev.rect.x, Y: rect.y - prev.rect.y };
                prev.clip.StartAnimation(&HSTRING::from("Offset"), &self.spring(grow, to_off)?)?;
                prev.clip.StartAnimation(&HSTRING::from("Size"), &self.spring(grow, full)?)?;
                prev.sprite.StartAnimation(&HSTRING::from("Opacity"), &self.fade(0.0, 130)?)?;
                let at = Vector3 { X: rect.x, Y: rect.y, Z: 0.0 };
                let was = Vector3 { X: prev.rect.x, Y: prev.rect.y, Z: 0.0 };
                match anim {
                    Anim::Pop => sprite.StartAnimation(&HSTRING::from("Scale"), &self.scale_anim(Some(0.86), 1.0, 360)?)?,
                    // Out of the edge: the content follows the shape from the edge side.
                    Anim::Drawer { open: true, from } => {
                        let start = Vector3 { X: rect.x + from.0 * 28.0, Y: rect.y + from.1 * 28.0, Z: 0.0 };
                        sprite.StartAnimation(&HSTRING::from("Offset"), &self.glide(start, at, 380)?)?;
                    }
                    // Back into the edge: the old content slides that way as it goes.
                    Anim::Drawer { open: false, from } => {
                        let end = Vector3 { X: prev.rect.x + from.0 * 28.0, Y: prev.rect.y + from.1 * 28.0, Z: 0.0 };
                        prev.sprite.StartAnimation(&HSTRING::from("Offset"), &self.glide(was, end, 260)?)?;
                    }
                    // Wheel flips: in from one side, the old one out the other.
                    Anim::Flip { dir } => {
                        let d = 36.0 * dir;
                        sprite.StartAnimation(
                            &HSTRING::from("Offset"),
                            &self.glide(Vector3 { X: rect.x + d, Y: rect.y, Z: 0.0 }, at, 300)?,
                        )?;
                        let end = Vector3 { X: prev.rect.x - d, Y: prev.rect.y, Z: 0.0 };
                        prev.sprite.StartAnimation(&HSTRING::from("Offset"), &self.glide(was, end, 260)?)?;
                    }
                    _ => {}
                }
                self.outgoing.push(prev);
            }
            Some(prev) if anim == Anim::Swap && !reduce_motion => {
                // Same place, new content: the new one slides in a few pixels while the old fades.
                clip.SetSize(full)?;
                let at = Vector3 { X: rect.x, Y: rect.y, Z: 0.0 };
                sprite.SetOpacity(0.0)?;
                sprite.StartAnimation(&HSTRING::from("Opacity"), &self.fade(1.0, 160)?)?;
                sprite.StartAnimation(&HSTRING::from("Offset"), &self.slide(Vector3 { X: rect.x, Y: rect.y + 6.0, Z: 0.0 }, at, 240)?)?;
                sprite.StartAnimation(&HSTRING::from("Scale"), &self.scale_anim(Some(0.97), 1.0, 260)?)?;
                prev.sprite.StartAnimation(&HSTRING::from("Opacity"), &self.fade(0.0, 140)?)?;
                self.outgoing.push(prev);
            }
            Some(prev) => {
                clip.SetSize(full)?;
                if anim != Anim::None {
                    // Reduced motion: a short crossfade only.
                    sprite.SetOpacity(0.0)?;
                    sprite.StartAnimation(&HSTRING::from("Opacity"), &self.fade(1.0, 80)?)?;
                }
                children.Remove(&prev.sprite)?;
            }
            None => clip.SetSize(full)?,
        }
        self.current = Some(Layer { sprite, surface, clip, rect, px });
        Ok(())
    }

    /// Remove scenes that finished fading out. Called from a timer.
    pub fn drop_outgoing(&mut self) -> Result<()> {
        let children = self.root.Children()?;
        for l in self.outgoing.drain(..) {
            children.Remove(&l.sprite)?;
        }
        Ok(())
    }

    pub fn has_outgoing(&self) -> bool {
        !self.outgoing.is_empty()
    }

    /// Update the ring. `rect` is the scene's window-local rect.
    pub fn ring(&mut self, spec: Option<RingSpec>, rect: Rect, now_ms: u64) -> Result<()> {
        let Some(spec) = spec else {
            if self.ring.last.take().is_some() {
                self.ring.visual.SetIsVisible(false)?;
                self.ring.geometry.StopAnimation(&HSTRING::from("TrimEnd"))?;
            }
            return Ok(());
        };
        if let Some((prev, prev_rect, at)) = &self.ring.last {
            if *prev_rect == rect && spec.continues(prev, now_ms.saturating_sub(*at)) {
                return Ok(());
            }
        }
        let center = Vector2 { X: rect.x + spec.center.x, Y: rect.y + spec.center.y };
        self.ring.geometry.SetCenter(center)?;
        self.ring.geometry.SetRadius(Vector2 { X: spec.radius, Y: spec.radius })?;
        self.ring.shape.SetCenterPoint(center)?;
        self.ring.shape.SetRotationAngleInDegrees(-90.0)?;
        self.ring.shape.SetStrokeThickness(spec.width)?;
        self.ring.shape.SetStrokeBrush(&self.compositor.CreateColorBrushWithColor(ui_color(spec.color))?)?;
        self.ring.geometry.StopAnimation(&HSTRING::from("TrimEnd"))?;
        self.ring.geometry.SetTrimEnd(spec.value.max(0.001))?;
        if spec.rate_per_ms > 0.0 && spec.value < 1.0 {
            let remaining = ((1.0 - spec.value) / spec.rate_per_ms) as u64;
            let a = self.compositor.CreateScalarKeyFrameAnimation()?;
            let linear = self.compositor.CreateLinearEasingFunction()?;
            a.InsertKeyFrameWithEasingFunction(0.0, spec.value, &linear)?;
            a.InsertKeyFrameWithEasingFunction(1.0, 1.0, &linear)?;
            a.SetDuration(ts(remaining.max(1)))?;
            self.ring.geometry.StartAnimation(&HSTRING::from("TrimEnd"), &a)?;
        }
        self.ring.visual.SetIsVisible(true)?;
        self.ring.last = Some((spec, rect, now_ms));
        Ok(())
    }

    /// Forget the ring state so the next update reapplies it.
    pub fn reset_ring(&mut self) {
        self.ring.last = None;
    }

    /// Point the compositor at a recreated Direct2D device.
    pub fn set_device(&mut self, gfx: &Gfx) -> Result<()> {
        use windows::Win32::System::WinRT::Composition::ICompositionGraphicsDeviceInterop;
        unsafe { self.device.cast::<ICompositionGraphicsDeviceInterop>()?.SetRenderingDevice(&gfx.d2d) }
    }
}
