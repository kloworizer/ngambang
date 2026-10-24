//! Direct3D 11 / Direct2D / DirectWrite: one device for everything, plus the
//! primitive renderer used for every scene.

use std::cell::RefCell;
use std::collections::HashMap;

use ngambang_core::geometry::{Color, Point};
use ngambang_core::view::{Align, Prim, TextMeasure, TextStyle, Weight};
use windows::core::BOOL;
use windows::core::{Interface, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;

pub struct Gfx {
    /// Kept alive for the Direct2D device built on it.
    #[allow(dead_code)]
    pub d3d: ID3D11Device,
    pub d2d_factory: ID2D1Factory1,
    pub d2d: ID2D1Device,
    pub dwrite: IDWriteFactory,
    pub warp: bool,
    sans: HSTRING,
    mono: HSTRING,
    formats: RefCell<HashMap<(u32, Weight, bool, u8), IDWriteTextFormat>>,
    brushes: RefCell<HashMap<u32, ID2D1SolidColorBrush>>,
    stroke: ID2D1StrokeStyle,
}

fn create_d3d(driver: D3D_DRIVER_TYPE) -> Result<ID3D11Device> {
    let mut device = None;
    unsafe {
        D3D11CreateDevice(
            None,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
    }
    device.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
}

impl Gfx {
    pub fn new() -> Result<Gfx> {
        let (d3d, warp) = match create_d3d(D3D_DRIVER_TYPE_HARDWARE) {
            Ok(d) => (d, false),
            Err(_) => (create_d3d(D3D_DRIVER_TYPE_WARP)?, true),
        };
        unsafe {
            let d2d_factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&D2D1_FACTORY_OPTIONS::default()))?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let d2d = d2d_factory.CreateDevice(&dxgi)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let sans = pick_family(&dwrite, &["Segoe UI Variable Text", "Segoe UI"]);
            let mono = pick_family(&dwrite, &["Cascadia Mono", "Consolas", "Courier New"]);
            let stroke = d2d_factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES1 {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                    transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
                },
                None,
            )?;
            Ok(Gfx {
                d3d,
                d2d_factory,
                d2d,
                dwrite,
                warp,
                sans,
                mono,
                formats: RefCell::new(HashMap::new()),
                brushes: RefCell::new(HashMap::new()),
                stroke: stroke.cast()?,
            })
        }
    }

    fn format(&self, style: TextStyle, align: Align) -> Result<IDWriteTextFormat> {
        let key = ((style.size * 10.0) as u32, style.weight, style.mono, align as u8);
        if let Some(f) = self.formats.borrow().get(&key) {
            return Ok(f.clone());
        }
        let weight = match style.weight {
            Weight::Regular => DWRITE_FONT_WEIGHT_REGULAR,
            Weight::Medium => DWRITE_FONT_WEIGHT_MEDIUM,
            Weight::Semibold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
        };
        let family = if style.mono { &self.mono } else { &self.sans };
        let f = unsafe {
            let f = self.dwrite.CreateTextFormat(
                family,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                style.size,
                &HSTRING::from("en-us"),
            )?;
            f.SetTextAlignment(match align {
                Align::Start => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::End => DWRITE_TEXT_ALIGNMENT_TRAILING,
            })?;
            let sign = self.dwrite.CreateEllipsisTrimmingSign(&f)?;
            f.SetTrimming(&DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 }, &sign)?;
            f
        };
        self.formats.borrow_mut().insert(key, f.clone());
        Ok(f)
    }

    fn brush(&self, dc: &ID2D1DeviceContext, c: Color) -> Result<ID2D1SolidColorBrush> {
        if let Some(b) = self.brushes.borrow().get(&c.0) {
            return Ok(b.clone());
        }
        let [r, g, b, a] = c.to_f32();
        let brush = unsafe { dc.CreateSolidColorBrush(&D2D1_COLOR_F { r, g, b, a }, None)? };
        self.brushes.borrow_mut().insert(c.0, brush.clone());
        Ok(brush)
    }

    fn path(&self, points: &[Point], closed: bool, filled: bool) -> Result<ID2D1PathGeometry1> {
        unsafe {
            let geo = self.d2d_factory.CreatePathGeometry()?;
            let sink = geo.Open()?;
            if let Some(first) = points.first() {
                sink.BeginFigure(pt(*first), if filled { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW });
                let rest: Vec<_> = points[1..].iter().map(|p| pt(*p)).collect();
                sink.AddLines(&rest);
                sink.EndFigure(if closed || filled { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
            }
            sink.Close()?;
            Ok(geo)
        }
    }

    /// Draw primitives in DIPs. The caller has set DPI and transform.
    pub fn draw(&self, dc: &ID2D1DeviceContext, prims: &[Prim]) -> Result<()> {
        unsafe {
            for p in prims {
                match p {
                    Prim::RoundRect { rect, radius, fill, stroke } => {
                        if let Some(c) = fill {
                            let rr = D2D1_ROUNDED_RECT { rect: rf(rect.x, rect.y, rect.w, rect.h), radiusX: *radius, radiusY: *radius };
                            dc.FillRoundedRectangle(&rr, &self.brush(dc, *c)?);
                        }
                        if let Some((c, w)) = stroke {
                            let h = w / 2.0;
                            let rr = D2D1_ROUNDED_RECT {
                                rect: rf(rect.x + h, rect.y + h, rect.w - w, rect.h - w),
                                radiusX: (radius - h).max(0.0),
                                radiusY: (radius - h).max(0.0),
                            };
                            dc.DrawRoundedRectangle(&rr, &self.brush(dc, *c)?, *w, None);
                        }
                    }
                    Prim::Circle { cx, cy, r, fill, stroke } => {
                        let e = D2D1_ELLIPSE { point: pt(Point::new(*cx, *cy)), radiusX: *r, radiusY: *r };
                        if let Some(c) = fill {
                            dc.FillEllipse(&e, &self.brush(dc, *c)?);
                        }
                        if let Some((c, w)) = stroke {
                            dc.DrawEllipse(&e, &self.brush(dc, *c)?, *w, &self.stroke);
                        }
                    }
                    Prim::Polyline { points, width, color, closed } => {
                        if points.len() >= 2 {
                            let geo = self.path(points, *closed, false)?;
                            dc.DrawGeometry(&geo, &self.brush(dc, *color)?, *width, &self.stroke);
                        }
                    }
                    Prim::Polygon { points, fill } => {
                        if points.len() >= 3 {
                            let geo = self.path(points, true, true)?;
                            dc.FillGeometry(&geo, &self.brush(dc, *fill)?, None);
                        }
                    }
                    Prim::Text { rect, text, style, color, align, wrap } => {
                        if text.is_empty() || rect.w <= 1.0 || rect.h <= 1.0 {
                            continue;
                        }
                        let format = self.format(*style, *align)?;
                        let w: Vec<u16> = text.encode_utf16().collect();
                        let layout = self.dwrite.CreateTextLayout(&w, &format, rect.w, rect.h)?;
                        layout.SetWordWrapping(if *wrap { DWRITE_WORD_WRAPPING_WRAP } else { DWRITE_WORD_WRAPPING_NO_WRAP })?;
                        layout.SetParagraphAlignment(if *wrap {
                            DWRITE_PARAGRAPH_ALIGNMENT_NEAR
                        } else {
                            DWRITE_PARAGRAPH_ALIGNMENT_CENTER
                        })?;
                        dc.DrawTextLayout(
                            pt(Point::new(rect.x, rect.y)),
                            &layout,
                            &self.brush(dc, *color)?,
                            D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

impl TextMeasure for Gfx {
    fn width(&self, text: &str, style: TextStyle) -> f32 {
        let measure = || -> Result<f32> {
            let format = self.format(style, Align::Start)?;
            let w: Vec<u16> = text.encode_utf16().collect();
            unsafe {
                let layout = self.dwrite.CreateTextLayout(&w, &format, 10_000.0, 200.0)?;
                let mut m = DWRITE_TEXT_METRICS::default();
                layout.GetMetrics(&mut m)?;
                Ok(m.widthIncludingTrailingWhitespace)
            }
        };
        measure().unwrap_or_else(|_| ngambang_core::view::ApproxMeasure.width(text, style))
    }
}

fn pick_family(dwrite: &IDWriteFactory, names: &[&str]) -> HSTRING {
    unsafe {
        let mut collection = None;
        if dwrite.GetSystemFontCollection(&mut collection, false).is_ok() {
            if let Some(c) = collection {
                for name in names {
                    let mut index = 0u32;
                    let mut exists = BOOL(0);
                    let n = HSTRING::from(*name);
                    if c.FindFamilyName(PCWSTR(n.as_ptr()), &mut index, &mut exists).is_ok() && exists.as_bool() {
                        return n;
                    }
                }
            }
        }
    }
    HSTRING::from(*names.last().unwrap_or(&"Segoe UI"))
}

fn pt(p: Point) -> windows_numerics::Vector2 {
    windows_numerics::Vector2 { X: p.x, Y: p.y }
}

fn rf(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }
}

/// Render primitives into an offscreen bitmap (self-test).
pub fn draw_offscreen(gfx: &Gfx, width: u32, height: u32, prims: &[Prim]) -> Result<()> {
    unsafe {
        let dc = gfx.d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            colorContext: std::mem::ManuallyDrop::new(None),
        };
        let bmp = dc.CreateBitmap(D2D_SIZE_U { width, height }, None, 0, &props)?;
        dc.SetTarget(&bmp);
        dc.BeginDraw();
        dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
        gfx.draw(&dc, prims)?;
        dc.EndDraw(None, None)?;
    }
    Ok(())
}
