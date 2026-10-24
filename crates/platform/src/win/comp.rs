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
    scale: f32,
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

        Ok(Renderer {
            _queue: queue,
            compositor,
            _target: target,
            root,
            device,
            current: None,
            outgoing: Vec::new(),
            ring: Ring { visual, geometry, shape, last: None },
            scale: 1.0,
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

    /// Show `scene` at `rect` (window-local DIPs). `animate` morphs from the
    /// previous scene; otherwise the scene is redrawn in place.
    pub fn show(&mut self, gfx: &Gfx, scene: &Scene, rect: Rect, animate: bool, reduce_motion: bool) -> Result<()> {
        let px = ((rect.w * self.scale).ceil().max(1.0) as i32, (rect.h * self.scale).ceil().max(1.0) as i32);
        let same_kind = !animate;
        if let Some(cur) = &mut self.current {
            if same_kind && cur.px == px {
                cur.sprite.SetOffset(Vector3 { X: rect.x, Y: rect.y, Z: 0.0 })?;
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
        let clip = self.compositor.CreateRoundedRectangleGeometry()?;
        clip.SetCornerRadius(Vector2 { X: scene.clip_radius, Y: scene.clip_radius })?;
        sprite.SetClip(&self.compositor.CreateGeometricClipWithGeometry(&clip)?)?;
        let children = self.root.Children()?;
        children.InsertAtTop(&sprite)?;
        // Keep the ring above the content.
        children.Remove(&self.ring.visual)?;
        children.InsertAtTop(&self.ring.visual)?;

        let full = Vector2 { X: rect.w, Y: rect.h };
        let prev = self.current.take();
        match prev {
            Some(prev) if animate && !reduce_motion => {
                let grow = rect.w * rect.h > prev.rect.w * prev.rect.h;
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
                self.outgoing.push(prev);
            }
            Some(prev) => {
                clip.SetSize(full)?;
                if animate {
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
