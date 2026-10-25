//! Embeds the Windows resources into both executables: the app icon (so
//! Explorer, the taskbar and Task Manager show it), version information
//! (Task Manager's name and the file's Details tab) and a manifest that turns
//! on modern common controls for the Settings window.
//!
//! The `.res` file is written directly (the format is simple), so building
//! needs no resource compiler; the MSVC linker takes it as an input.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const LANG: u16 = 0x0409;
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
const RT_MANIFEST: u16 = 24;

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
  <assemblyIdentity type="win32" name="Ngambang" version="1.0.0.0"/>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <asmv3:application>
    <asmv3:windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </asmv3:windowsSettings>
  </asmv3:application>
</assembly>
"#;

fn main() {
    let ico = Path::new("assets/ngambang.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    println!("cargo:rerun-if-changed=build.rs");
    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("windows-msvc") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let icon = fs::read(ico).expect("assets/ngambang.ico");
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    for (bin, description) in [("ngambang", "Ngambang command line"), ("ngambangw", "Ngambang")] {
        let path = out.join(format!("{bin}.res"));
        fs::write(&path, res_file(&icon, &version, bin, description)).expect("write .res");
        println!("cargo:rustc-link-arg-bin={bin}={}", path.display());
    }
}

/// One resource entry: header, data, padding to a DWORD.
fn entry(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, data: &[u8]) {
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes()); // header size
    out.extend([0xFF, 0xFF]);
    out.extend(kind.to_le_bytes());
    out.extend([0xFF, 0xFF]);
    out.extend(id.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // data version
    out.extend(flags.to_le_bytes());
    out.extend(LANG.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // version
    out.extend(0u32.to_le_bytes()); // characteristics
    out.extend(data);
    pad(out);
}

fn pad(out: &mut Vec<u8>) {
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn res_file(ico: &[u8], version: &str, bin: &str, description: &str) -> Vec<u8> {
    let mut out = Vec::new();
    // A .res file starts with an empty entry.
    out.extend(0u32.to_le_bytes());
    out.extend(32u32.to_le_bytes());
    out.extend([0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0]);
    out.extend([0u8; 16]);

    // Icons: every image of the .ico as RT_ICON 1..n, and the directory as RT_GROUP_ICON 1.
    let u16_at = |i: usize| u16::from_le_bytes([ico[i], ico[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([ico[i], ico[i + 1], ico[i + 2], ico[i + 3]]);
    let count = u16_at(4) as usize;
    let mut group = Vec::new();
    group.extend(0u16.to_le_bytes());
    group.extend(1u16.to_le_bytes());
    group.extend((count as u16).to_le_bytes());
    for i in 0..count {
        let e = 6 + 16 * i;
        let (size, offset) = (u32_at(e + 8) as usize, u32_at(e + 12) as usize);
        entry(&mut out, RT_ICON, i as u16 + 1, 0x1010, &ico[offset..offset + size]);
        group.extend(&ico[e..e + 8]); // width, height, colours, reserved, planes, bit count
        group.extend((size as u32).to_le_bytes());
        group.extend((i as u16 + 1).to_le_bytes());
    }
    entry(&mut out, RT_GROUP_ICON, 1, 0x1030, &group);
    entry(&mut out, RT_VERSION, 1, 0x0030, &version_info(version, bin, description));
    entry(&mut out, RT_MANIFEST, 1, 0x0030, MANIFEST.as_bytes());
    out
}

fn wide(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(std::iter::once(0)).flat_map(|c| c.to_le_bytes()).collect()
}

/// A version-resource block: length, value length, type, key, value, children.
fn block(key: &str, value: &[u8], value_len: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0, 0];
    b.extend(value_len.to_le_bytes());
    b.extend((text as u16).to_le_bytes());
    b.extend(wide(key));
    pad(&mut b);
    b.extend(value);
    for c in children {
        pad(&mut b);
        b.extend(c);
    }
    let len = b.len() as u16;
    b[0..2].copy_from_slice(&len.to_le_bytes());
    b
}

fn version_info(version: &str, bin: &str, description: &str) -> Vec<u8> {
    let mut parts = version.split(['.', '-']).map(|p| p.parse::<u16>().unwrap_or(0));
    let (major, minor, patch) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    let ms = ((major as u32) << 16) | minor as u32;
    let ls = (patch as u32) << 16;
    let mut fixed = Vec::new();
    for v in [0xFEEF04BDu32, 0x0001_0000, ms, ls, ms, ls, 0x3F, 0, 0x0004_0004, 1, 0, 0, 0] {
        fixed.extend(v.to_le_bytes());
    }
    let string = |k: &str, v: &str| {
        let w = wide(v);
        block(k, &w, (w.len() / 2) as u16, true, &[])
    };
    let exe = format!("{bin}.exe");
    let strings = [
        string("CompanyName", "Ngambang"),
        string("FileDescription", description),
        string("FileVersion", version),
        string("InternalName", bin),
        string("LegalCopyright", "MIT License with Commons Clause"),
        string("OriginalFilename", &exe),
        string("ProductName", "Ngambang"),
        string("ProductVersion", version),
    ];
    let table = block("040904B0", &[], 0, true, &strings);
    let string_info = block("StringFileInfo", &[], 0, true, &[table]);
    let mut translation = Vec::new();
    translation.extend(LANG.to_le_bytes());
    translation.extend(1200u16.to_le_bytes());
    let var = block("Translation", &translation, 4, false, &[]);
    let var_info = block("VarFileInfo", &[], 0, true, &[var]);
    block("VS_VERSION_INFO", &fixed, fixed.len() as u16, false, &[string_info, var_info])
}
