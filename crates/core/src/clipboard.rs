//! Developer clipboard: recognise common formats and offer one-tap transforms.
//! Pure functions; the platform decides when to call them (SPEC §6.5).

use serde_json::Value;

/// Ignore anything larger than this (characters).
pub const MAX_TEXT: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum ClipKind {
    Jwt { header: String, payload: String, exp: Option<i64> },
    Json { pretty: String },
    TrackingUrl { clean: String },
    Uuid,
    UnixTime { secs: i64 },
    HexColor { r: u8, g: u8, b: u8, a: u8 },
    Base64 { decoded: String },
}

impl ClipKind {
    pub fn label(&self) -> &'static str {
        match self {
            ClipKind::Jwt { .. } => "JWT",
            ClipKind::Json { .. } => "JSON",
            ClipKind::TrackingUrl { .. } => "TRACKING LINK",
            ClipKind::Uuid => "UUID",
            ClipKind::UnixTime { .. } => "UNIX TIME",
            ClipKind::HexColor { .. } => "COLOUR",
            ClipKind::Base64 { .. } => "BASE64",
        }
    }

    /// The transforms offered for this kind, in display order.
    pub fn actions(&self) -> &'static [ClipAction] {
        use ClipAction::*;
        match self {
            ClipKind::Jwt { .. } => &[CopyPayload, CopyHeader, NewUuid],
            ClipKind::Json { .. } => &[Pretty, Minify],
            ClipKind::TrackingUrl { .. } => &[StripTracking],
            ClipKind::Uuid => &[Upper, Lower, NewUuid],
            ClipKind::UnixTime { .. } => &[IsoUtc],
            ClipKind::HexColor { .. } => &[Rgb, Hsl],
            ClipKind::Base64 { .. } => &[DecodeBase64],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipAction {
    CopyPayload,
    CopyHeader,
    Pretty,
    Minify,
    StripTracking,
    Upper,
    Lower,
    NewUuid,
    IsoUtc,
    Rgb,
    Hsl,
    DecodeBase64,
    EncodeBase64,
}

impl ClipAction {
    pub fn label(self) -> &'static str {
        match self {
            ClipAction::CopyPayload => "Copy payload",
            ClipAction::CopyHeader => "Copy header",
            ClipAction::Pretty => "Pretty",
            ClipAction::Minify => "Minify",
            ClipAction::StripTracking => "Strip tracking",
            ClipAction::Upper => "UPPER",
            ClipAction::Lower => "lower",
            ClipAction::NewUuid => "New UUID",
            ClipAction::IsoUtc => "ISO-8601",
            ClipAction::Rgb => "rgb()",
            ClipAction::Hsl => "hsl()",
            ClipAction::DecodeBase64 => "Decode",
            ClipAction::EncodeBase64 => "Base64",
        }
    }
}

/// Detect the first matching kind, in SPEC order.
pub fn detect(text: &str) -> Option<ClipKind> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    detect_jwt(t)
        .or_else(|| detect_json(t))
        .or_else(|| detect_tracking_url(t))
        .or_else(|| is_uuid(t).then_some(ClipKind::Uuid))
        .or_else(|| detect_unix_time(t))
        .or_else(|| detect_hex_color(t))
        .or_else(|| detect_base64(t))
}

/// Apply a transform. `random` supplies 16 bytes for `NewUuid`.
pub fn apply(action: ClipAction, text: &str, kind: &ClipKind, random: [u8; 16]) -> Option<String> {
    let t = text.trim();
    Some(match (action, kind) {
        (ClipAction::CopyPayload, ClipKind::Jwt { payload, .. }) => payload.clone(),
        (ClipAction::CopyHeader, ClipKind::Jwt { header, .. }) => header.clone(),
        (ClipAction::Pretty, ClipKind::Json { pretty }) => pretty.clone(),
        (ClipAction::Minify, ClipKind::Json { .. }) => serde_json::from_str::<Value>(t).ok()?.to_string(),
        (ClipAction::StripTracking, ClipKind::TrackingUrl { clean }) => clean.clone(),
        (ClipAction::Upper, _) => t.to_uppercase(),
        (ClipAction::Lower, _) => t.to_lowercase(),
        (ClipAction::NewUuid, _) => crate::format::uuid_v4(random),
        (ClipAction::IsoUtc, ClipKind::UnixTime { secs }) => crate::format::iso_utc(*secs),
        (ClipAction::Rgb, ClipKind::HexColor { r, g, b, a }) => {
            if *a == 255 {
                format!("rgb({r}, {g}, {b})")
            } else {
                format!("rgba({r}, {g}, {b}, {:.2})", *a as f32 / 255.0)
            }
        }
        (ClipAction::Hsl, ClipKind::HexColor { r, g, b, .. }) => {
            let (h, s, l) = rgb_to_hsl(*r, *g, *b);
            format!("hsl({h:.0}, {s:.0}%, {l:.0}%)")
        }
        (ClipAction::DecodeBase64, ClipKind::Base64 { decoded }) => decoded.clone(),
        (ClipAction::EncodeBase64, _) => base64_encode(t.as_bytes()),
        _ => return None,
    })
}

/// A one-line preview shown in the slip.
pub fn preview(text: &str, kind: &ClipKind, unix_now: i64) -> String {
    match kind {
        ClipKind::Jwt { payload, .. } => payload.clone(),
        ClipKind::Json { pretty } => pretty.clone(),
        ClipKind::TrackingUrl { clean } => clean.clone(),
        ClipKind::UnixTime { secs } => {
            format!("{}\n{}", crate::format::iso_utc(*secs), crate::format::relative(*secs - unix_now))
        }
        ClipKind::Base64 { decoded } => decoded.clone(),
        _ => text.trim().to_string(),
    }
}

/// Short status for the tag, e.g. "expired 3h ago".
pub fn status(kind: &ClipKind, unix_now: i64) -> Option<String> {
    match kind {
        ClipKind::Jwt { exp: Some(exp), .. } => {
            let rel = crate::format::relative(exp - unix_now);
            Some(if *exp < unix_now { format!("expired {rel}") } else { format!("expires {rel}") })
        }
        ClipKind::UnixTime { secs } => Some(crate::format::relative(secs - unix_now)),
        _ => None,
    }
}

fn detect_jwt(t: &str) -> Option<ClipKind> {
    let parts: Vec<&str> = t.split('.').collect();
    if parts.len() != 3 || parts[0].len() < 10 || parts[1].is_empty() {
        return None;
    }
    let header: Value = serde_json::from_slice(&base64_decode(parts[0], true)?).ok()?;
    header.get("alg")?;
    let payload: Value = serde_json::from_slice(&base64_decode(parts[1], true)?).ok()?;
    let exp = payload.get("exp").and_then(Value::as_i64);
    Some(ClipKind::Jwt { header: serde_json::to_string_pretty(&header).ok()?, payload: serde_json::to_string_pretty(&payload).ok()?, exp })
}

fn detect_json(t: &str) -> Option<ClipKind> {
    if !(t.starts_with('{') && t.ends_with('}') || t.starts_with('[') && t.ends_with(']')) {
        return None;
    }
    let v: Value = serde_json::from_str(t).ok()?;
    Some(ClipKind::Json { pretty: serde_json::to_string_pretty(&v).ok()? })
}

const TRACKING: &[&str] = &["fbclid", "gclid", "dclid", "msclkid", "igshid", "mc_eid", "mc_cid", "si", "yclid", "_hsenc", "_hsmi"];

fn is_tracking_param(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("utm_") || TRACKING.contains(&n.as_str())
}

fn detect_tracking_url(t: &str) -> Option<ClipKind> {
    if !(t.starts_with("http://") || t.starts_with("https://")) || t.contains(char::is_whitespace) {
        return None;
    }
    let (before_frag, frag) = match t.split_once('#') {
        Some((a, b)) => (a, Some(b)),
        None => (t, None),
    };
    let (base, query) = before_frag.split_once('?')?;
    let params: Vec<&str> = query.split('&').filter(|p| !p.is_empty()).collect();
    let kept: Vec<&str> = params.iter().copied().filter(|p| !is_tracking_param(p.split('=').next().unwrap_or(""))).collect();
    if kept.len() == params.len() {
        return None;
    }
    let mut clean = base.to_string();
    if !kept.is_empty() {
        clean.push('?');
        clean.push_str(&kept.join("&"));
    }
    if let Some(f) = frag {
        clean.push('#');
        clean.push_str(f);
    }
    Some(ClipKind::TrackingUrl { clean })
}

fn is_uuid(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn detect_unix_time(t: &str) -> Option<ClipKind> {
    if !(t.len() == 10 || t.len() == 13) || !t.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: i64 = t.parse().ok()?;
    let secs = if t.len() == 13 { n / 1000 } else { n };
    // 2000-01-01 .. 2100-01-01
    (946_684_800..4_102_444_800).contains(&secs).then_some(ClipKind::UnixTime { secs })
}

fn detect_hex_color(t: &str) -> Option<ClipKind> {
    let h = t.strip_prefix('#')?;
    if !h.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let nib = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|v| v * 17);
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    let (r, g, b, a) = match h.len() {
        3 => (nib(0)?, nib(1)?, nib(2)?, 255),
        6 => (byte(0)?, byte(2)?, byte(4)?, 255),
        8 => (byte(0)?, byte(2)?, byte(4)?, byte(6)?),
        _ => return None,
    };
    Some(ClipKind::HexColor { r, g, b, a })
}

fn detect_base64(t: &str) -> Option<ClipKind> {
    if t.len() < 12 || t.len() % 4 != 0 || t.contains(char::is_whitespace) {
        return None;
    }
    let bytes = base64_decode(t, false)?;
    let decoded = String::from_utf8(bytes).ok()?;
    // Require mostly printable text so random identifiers don't match.
    let printable = decoded.chars().filter(|c| !c.is_control() || c.is_whitespace()).count();
    (decoded.chars().count() >= 8 && printable == decoded.chars().count()).then_some(ClipKind::Base64 { decoded })
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decode standard or (when `url` is set) URL-safe base64; padding optional for URL-safe.
pub fn base64_decode(s: &str, url: bool) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' if !url => 62,
            b'/' if !url => 63,
            b'-' if url => 62,
            b'_' if url => 63,
            _ => return None,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // A single leftover sextet cannot encode a byte.
    if s.len() % 4 == 1 {
        return None;
    }
    Some(out)
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l * 100.0);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s * 100.0, l * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // {"alg":"HS256","typ":"JWT"}.{"sub":"42","exp":1700000000}.sig
    const JWT: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiI0MiIsImV4cCI6MTcwMDAwMDAwMH0.c2lnbmF0dXJl";

    #[test]
    fn jwt() {
        let k = detect(JWT).unwrap();
        let ClipKind::Jwt { exp, payload, .. } = &k else { panic!("{k:?}") };
        assert_eq!(*exp, Some(1_700_000_000));
        assert!(payload.contains("\"sub\": \"42\""));
        assert_eq!(status(&k, 1_700_010_800).unwrap(), "expired 3h ago");
        assert!(apply(ClipAction::CopyPayload, JWT, &k, [0; 16]).unwrap().contains("exp"));
    }

    #[test]
    fn json_keeps_key_order() {
        let k = detect(r#"{"b":1,"a":[1,2]}"#).unwrap();
        let ClipKind::Json { pretty } = &k else { panic!() };
        assert!(pretty.find("\"b\"").unwrap() < pretty.find("\"a\"").unwrap());
        assert_eq!(apply(ClipAction::Minify, "{ \"b\" : 1 }", &k, [0; 16]).unwrap(), r#"{"b":1}"#);
        assert_eq!(detect("{not json}"), None);
    }

    #[test]
    fn tracking_urls() {
        let k = detect("https://example.com/a?id=7&utm_source=x&fbclid=abc#top").unwrap();
        assert_eq!(k, ClipKind::TrackingUrl { clean: "https://example.com/a?id=7#top".into() });
        let k = detect("https://example.com/?utm_medium=y").unwrap();
        assert_eq!(k, ClipKind::TrackingUrl { clean: "https://example.com/".into() });
        assert_eq!(detect("https://example.com/?id=1"), None);
    }

    #[test]
    fn uuid_time_color() {
        assert_eq!(detect("123e4567-e89b-12d3-a456-426614174000"), Some(ClipKind::Uuid));
        assert_eq!(detect("1700000000"), Some(ClipKind::UnixTime { secs: 1_700_000_000 }));
        assert_eq!(detect("1700000000123"), Some(ClipKind::UnixTime { secs: 1_700_000_000 }));
        assert_eq!(detect("0000000001"), None);
        let c = detect("#F2A93B").unwrap();
        assert_eq!(c, ClipKind::HexColor { r: 0xF2, g: 0xA9, b: 0x3B, a: 255 });
        assert_eq!(apply(ClipAction::Rgb, "#F2A93B", &c, [0; 16]).unwrap(), "rgb(242, 169, 59)");
        assert_eq!(apply(ClipAction::Hsl, "#ff0000", &detect("#ff0000").unwrap(), [0; 16]).unwrap(), "hsl(0, 100%, 50%)");
        assert_eq!(detect("#fff"), Some(ClipKind::HexColor { r: 255, g: 255, b: 255, a: 255 }));
        assert_eq!(detect("#ggg"), None);
    }

    #[test]
    fn base64_roundtrip_and_detection() {
        for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            let e = base64_encode(s.as_bytes());
            assert_eq!(base64_decode(&e, false).unwrap(), s.as_bytes());
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        let k = detect("aGVsbG8gd29ybGQgZnJvbSBuZ2FtYmFuZw==").unwrap();
        assert_eq!(k, ClipKind::Base64 { decoded: "hello world from ngambang".into() });
        // Plain words must not be mistaken for base64.
        assert_eq!(detect("abcdefghijkl"), None);
        assert_eq!(detect("hello world"), None);
    }

    #[test]
    fn rejects_huge_and_plain_text() {
        assert_eq!(detect(&"a".repeat(MAX_TEXT + 1)), None);
        assert_eq!(detect("just a sentence."), None);
        assert_eq!(detect("   "), None);
    }

    #[test]
    fn every_kind_has_working_actions() {
        let samples = [
            JWT,
            r#"{"a":1}"#,
            "https://x.io/?utm_a=1",
            "123e4567-e89b-12d3-a456-426614174000",
            "1700000000",
            "#abc",
            "aGVsbG8gd29ybGQgZnJvbSBuZ2FtYmFuZw==",
        ];
        for s in samples {
            let k = detect(s).unwrap_or_else(|| panic!("{s}"));
            for a in k.actions() {
                assert!(apply(*a, s, &k, [7; 16]).is_some(), "{a:?} on {s}");
            }
        }
    }
}
