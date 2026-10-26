//! Battery, network and VPN, file icons, pasting and dropped files.

use ngambang_core::model::{Art, NetState, PowerState};
use windows::core::HSTRING;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};
use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, SHGetFileInfoW, HDROP, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

/// The battery and charger now.
pub fn power_state() -> PowerState {
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return PowerState { percent: None, on_ac: true, saver: false, secs_left: None };
    }
    // 128: no system battery; 255: unknown.
    let has_battery = s.BatteryFlag != 128 && s.BatteryFlag != 255 && s.BatteryLifePercent != 255;
    PowerState {
        percent: has_battery.then_some(s.BatteryLifePercent.min(100)),
        on_ac: s.ACLineStatus != 0,
        saver: s.SystemStatusFlag == 1,
        secs_left: (s.BatteryLifeTime != u32::MAX && s.ACLineStatus == 0).then_some(s.BatteryLifeTime),
    }
}

/// Words in an adapter's name or description that mean a VPN.
const VPN_WORDS: [&str; 12] = [
    "vpn",
    "wireguard",
    "openvpn",
    "tap-windows",
    "wintun",
    "tailscale",
    "zerotier",
    "anyconnect",
    "forti",
    "globalprotect",
    "nordlynx",
    "protonvpn",
];

/// Online (Windows' own check) and the VPN adapter that is up, if any.
pub fn net_state() -> NetState {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    let online = super::net::online().unwrap_or(true);
    let mut vpn = None;
    unsafe {
        let mut size = 16 * 1024u32;
        let mut buf: Vec<u64> = vec![0; size as usize / 8];
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;
        let mut r = GetAdaptersAddresses(0, flags, None, Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH), &mut size);
        if r == 111 {
            // ERROR_BUFFER_OVERFLOW: try once more with the size asked for.
            buf = vec![0; size as usize / 8 + 1];
            r = GetAdaptersAddresses(0, flags, None, Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH), &mut size);
        }
        if r == 0 {
            let mut a = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
            while !a.is_null() {
                let ad = &*a;
                if ad.OperStatus == IfOperStatusUp {
                    let name = ad.FriendlyName.to_string().unwrap_or_default();
                    let desc = ad.Description.to_string().unwrap_or_default();
                    let text = format!("{name} {desc}").to_lowercase();
                    // 23: PPP (the built-in VPN client).
                    if ad.IfType == 23 || VPN_WORDS.iter().any(|w| text.contains(w)) {
                        vpn = Some(if name.is_empty() { desc } else { name });
                        break;
                    }
                }
                a = ad.Next;
            }
        }
    }
    NetState { online, vpn, wifi: None }
}

/// A file's or folder's icon as a 32×32 picture.
pub fn file_icon(path: &str) -> Option<Art> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static IDS: AtomicU64 = AtomicU64::new(1 << 40);
    unsafe {
        let mut info = SHFILEINFOW::default();
        let ok = SHGetFileInfoW(
            &HSTRING::from(path),
            Default::default(),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if ok == 0 || info.hIcon.is_invalid() {
            return None;
        }
        let mut ii = ICONINFO::default();
        let got = GetIconInfo(info.hIcon, &mut ii).is_ok();
        let _ = DestroyIcon(info.hIcon);
        if !got {
            return None;
        }
        let px = 32u32;
        let dc = CreateCompatibleDC(None);
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: px as i32,
                biHeight: -(px as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (px * px * 4) as usize];
        let lines = GetDIBits(dc, ii.hbmColor, 0, px, Some(pixels.as_mut_ptr().cast()), &mut bmi, DIB_RGB_COLORS);
        let _ = DeleteDC(dc);
        let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        if lines == 0 {
            return None;
        }
        // Old icons carry no alpha: treat them as opaque. Then premultiply.
        let no_alpha = pixels.chunks(4).all(|p| p[3] == 0);
        for p in pixels.chunks_mut(4) {
            if no_alpha {
                p[3] = 255;
            }
            let a = p[3] as u32;
            for c in &mut p[..3] {
                *c = (*c as u32 * a / 255) as u8;
            }
        }
        Some(Art { id: IDS.fetch_add(1, Ordering::Relaxed), w: px, h: px, pixels: std::sync::Arc::new(pixels) })
    }
}

/// Ctrl+V into the window that has the keyboard.
pub fn send_paste() {
    let key = |vk, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() }, ..Default::default() },
        },
    };
    let inputs = [key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// The paths in a WM_DROPFILES drop.
pub fn dropped_paths(drop: HDROP) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let n = DragQueryFileW(drop, u32::MAX, None);
        for i in 0..n.min(24) {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(drop, i, Some(&mut buf));
            out.push(String::from_utf16_lossy(&buf[..len]));
        }
        DragFinish(drop);
    }
    out
}

/// Let files be dropped on the island.
pub fn accept_drops(hwnd: HWND) {
    unsafe { windows::Win32::UI::Shell::DragAcceptFiles(hwnd, true) };
}
