//! Notification-area icon and its menu: turn modules on or off, move the
//! island, pause, autostart, open the config file, quit.

use ngambang_core::config::{Config, ModuleId};
use ngambang_core::geometry::Edge;
use windows::core::{w, Result, HSTRING};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::util::WM_APP_TRAY;

pub const CMD_MODULE: u32 = 1000;
pub const CMD_EDGE: u32 = 1100;
pub const CMD_PAUSE: u32 = 1200;
pub const CMD_AUTOSTART: u32 = 1201;
pub const CMD_CONFIG: u32 = 1202;
pub const CMD_QUIT: u32 = 1203;
pub const CMD_LOG: u32 = 1204;
pub const CMD_SETTINGS: u32 = 1205;

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

/// Draw the orb as a 32×32 icon (no resource files needed).
fn make_icon() -> Result<HICON> {
    const S: i32 = 32;
    let mut pixels = vec![0u32; (S * S) as usize];
    let c = (S as f32 - 1.0) / 2.0;
    for y in 0..S {
        for x in 0..S {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let px = |a: f32, r: u8, g: u8, b: u8| -> u32 {
                let a = a.clamp(0.0, 1.0);
                let pm = |v: u8| (v as f32 * a) as u32;
                ((a * 255.0) as u32) << 24 | pm(r) << 16 | pm(g) << 8 | pm(b)
            };
            let body = (15.5 - d).clamp(0.0, 1.0);
            let ring = (1.6 - (d - 10.5).abs()).clamp(0.0, 1.0);
            let dot = (4.5 - d).clamp(0.0, 1.0);
            let v = if dot > 0.0 {
                px(dot.max(body), 0xF2, 0xA9, 0x3B)
            } else if ring > 0.0 {
                // Blend the amber ring over the dark body.
                let (r, g, b) = (
                    0x15 as f32 + (0xF2 - 0x15) as f32 * ring,
                    0x17 as f32 + (0xA9 - 0x17) as f32 * ring,
                    0x1A as f32 + (0x3B - 0x1A) as f32 * ring,
                );
                px(body, r as u8, g as u8, b as u8)
            } else {
                px(body, 0x15, 0x17, 0x1A)
            };
            pixels[(y * S + x) as usize] = v;
        }
    }
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: S,
                biHeight: -S,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let color: HBITMAP = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u32, pixels.len());
        let mask = CreateBitmap(S, S, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

fn data(hwnd: HWND, icon: HICON) -> NOTIFYICONDATAW {
    let mut d = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_APP_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    let tip: Vec<u16> = "Ngambang".encode_utf16().collect();
    d.szTip[..tip.len()].copy_from_slice(&tip);
    d
}

impl Tray {
    pub fn new(hwnd: HWND) -> Result<Tray> {
        let icon = make_icon()?;
        let t = Tray { hwnd, icon };
        t.add();
        Ok(t)
    }

    /// (Re-)add the icon, e.g. after Explorer restarts.
    pub fn add(&self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &data(self.hwnd, self.icon));
        }
    }

    pub fn show_menu(&self, cfg: &Config, paused: bool, autostart: bool) -> Option<u32> {
        unsafe {
            let menu = CreatePopupMenu().ok()?;
            let modules = CreatePopupMenu().ok()?;
            for (i, m) in ModuleId::ALL.iter().enumerate() {
                let flags = if cfg.modules.get(*m) { MF_STRING | MF_CHECKED } else { MF_STRING };
                let _ = AppendMenuW(modules, flags, (CMD_MODULE + i as u32) as usize, &HSTRING::from(m.label()));
            }
            let edges = CreatePopupMenu().ok()?;
            for (i, e) in Edge::ALL.iter().enumerate() {
                let label = match e {
                    Edge::Left => "Left",
                    Edge::Right => "Right",
                    Edge::Top => "Top",
                    Edge::Bottom => "Bottom",
                };
                let flags = if cfg.dock.edge == *e { MF_STRING | MF_CHECKED } else { MF_STRING };
                let _ = AppendMenuW(edges, flags, (CMD_EDGE + i as u32) as usize, &HSTRING::from(label));
            }
            let title = HSTRING::from(format!("Ngambang {}", ngambang_core::VERSION));
            let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, &title);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(menu, MF_STRING, CMD_SETTINGS as usize, w!("Settings…"));
            let _ = AppendMenuW(menu, MF_POPUP, modules.0 as usize, w!("Modules"));
            let _ = AppendMenuW(menu, MF_POPUP, edges.0 as usize, w!("Dock edge"));
            let _ = AppendMenuW(menu, if paused { MF_STRING | MF_CHECKED } else { MF_STRING }, CMD_PAUSE as usize, w!("Pause island"));
            let _ = AppendMenuW(
                menu,
                if autostart { MF_STRING | MF_CHECKED } else { MF_STRING },
                CMD_AUTOSTART as usize,
                w!("Start with Windows"),
            );
            let _ = AppendMenuW(menu, MF_STRING, CMD_CONFIG as usize, w!("Open config file"));
            let _ = AppendMenuW(menu, MF_STRING, CMD_LOG as usize, w!("Open log folder"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT as usize, w!("Quit"));
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(self.hwnd);
            let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, pt.x, pt.y, None, self.hwnd, None);
            let _ = DestroyMenu(menu);
            (cmd.0 != 0).then_some(cmd.0 as u32)
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &data(self.hwnd, self.icon));
            let _ = DestroyIcon(self.icon);
        }
    }
}
