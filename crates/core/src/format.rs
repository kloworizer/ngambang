//! Text formatting helpers: durations, times, slugs, GUIDs, UUIDs, app names.

/// "2m14s", "45s", "1h03m".
pub fn duration_short(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Media/timer clock: "1:42", "12:05", "1:32:08".
/// "512 KB", "34.2 MB", "1.4 GB".
pub fn bytes(n: u64) -> String {
    let f = n as f64;
    if f >= 1e9 {
        format!("{:.1} GB", f / 1e9)
    } else if f >= 1e6 {
        format!("{:.1} MB", f / 1e6)
    } else {
        format!("{:.0} KB", (f / 1e3).max(1.0))
    }
}

pub fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// "2h 05m" for daily totals.
pub fn hours_minutes(mins: u64) -> String {
    if mins >= 60 {
        format!("{}h {:02}m", mins / 60, mins % 60)
    } else {
        format!("{mins}m")
    }
}

/// "in 3h", "3h ago", "just now".
pub fn relative(delta_secs: i64) -> String {
    let a = delta_secs.unsigned_abs();
    let unit = if a < 45 {
        return "just now".into();
    } else if a < 3600 {
        format!("{}m", (a + 30) / 60)
    } else if a < 86_400 {
        format!("{}h", (a + 1800) / 3600)
    } else {
        format!("{}d", (a + 43_200) / 86_400)
    };
    if delta_secs >= 0 {
        format!("in {unit}")
    } else {
        format!("{unit} ago")
    }
}

/// ISO-8601 UTC for a Unix timestamp in seconds.
pub fn iso_utc(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, (secs / 60) % 60, secs % 60)
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Stable id from a title: "Deploy staging!" -> "deploy-staging".
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
        if out.len() >= 64 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "job".into()
    } else {
        out
    }
}

/// Parse "{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}" (braces optional) into 16 bytes
/// in the field layout Windows uses (data1 u32, data2 u16, data3 u16, data4 [u8; 8]).
pub fn parse_guid(s: &str) -> Option<(u32, u16, u16, [u8; 8])> {
    let s = s.trim().trim_start_matches('{').trim_end_matches('}');
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 || [8, 4, 4, 4, 12] != [parts[0].len(), parts[1].len(), parts[2].len(), parts[3].len(), parts[4].len()] {
        return None;
    }
    let d1 = u32::from_str_radix(parts[0], 16).ok()?;
    let d2 = u16::from_str_radix(parts[1], 16).ok()?;
    let d3 = u16::from_str_radix(parts[2], 16).ok()?;
    let tail = format!("{}{}", parts[3], parts[4]);
    let mut d4 = [0u8; 8];
    for (i, b) in d4.iter_mut().enumerate() {
        *b = u8::from_str_radix(tail.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some((d1, d2, d3, d4))
}

/// Random (v4) UUID from 16 random bytes supplied by the platform.
pub fn uuid_v4(mut b: [u8; 16]) -> String {
    b[6] = (b[6] & 0x0F) | 0x40;
    b[8] = (b[8] & 0x3F) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// A readable name for a media/app id: "Spotify.exe" -> "Spotify",
/// "Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic" -> "ZuneMusic",
/// "C:#Program Files#App#app.exe" -> "app".
pub fn friendly_app_name(id: &str) -> String {
    let mut s = id;
    if let Some((family, _)) = s.split_once('!') {
        s = family;
    }
    if let Some(pos) = s.rfind(['\\', '/', '#']) {
        s = &s[pos + 1..];
    }
    if let Some(pos) = s.rfind('_') {
        // Package family name suffix ("_8wekyb3d8bbwe").
        if s.len() - pos == 14 {
            s = &s[..pos];
        }
    }
    let s = s.strip_suffix(".exe").or_else(|| s.strip_suffix(".EXE")).unwrap_or(s);
    let s = s.rsplit('.').next().unwrap_or(s);
    if s.is_empty() {
        id.to_string()
    } else {
        s.to_string()
    }
}

/// Lower-case executable base name with extension: "C:\\x\\Game.EXE" -> "game.exe".
pub fn exe_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_ascii_lowercase()
}

/// Truncate to `max` characters, adding an ellipsis.
pub fn ellipsize(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(duration_short(134), "2m14s");
        assert_eq!(duration_short(9), "9s");
        assert_eq!(duration_short(3780), "1h03m");
        assert_eq!(clock(102), "1:42");
        assert_eq!(clock(5528), "1:32:08");
        assert_eq!(hours_minutes(125), "2h 05m");
        assert_eq!(hours_minutes(5), "5m");
    }

    #[test]
    fn relative_times() {
        assert_eq!(relative(-10_800), "3h ago");
        assert_eq!(relative(600), "in 10m");
        assert_eq!(relative(5), "just now");
        assert_eq!(relative(-200_000), "2d ago");
    }

    #[test]
    fn iso() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(iso_utc(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Deploy staging!"), "deploy-staging");
        assert_eq!(slug("  cargo   test --all "), "cargo-test-all");
        assert_eq!(slug("!!!"), "job");
    }

    #[test]
    fn guid() {
        let g = parse_guid("{8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c}").unwrap();
        assert_eq!(g.0, 0x8c5e7fda);
        assert_eq!(g.3[7], 0x5c);
        assert!(parse_guid("8c5e7fda-e8bf-4a96-9a85").is_none());
        assert!(parse_guid("zzzzzzzz-e8bf-4a96-9a85-a6e23a8c635c").is_none());
    }

    #[test]
    fn uuid() {
        let u = uuid_v4([0xFF; 16]);
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        assert!(matches!(&u[19..20], "8" | "9" | "a" | "b"));
    }

    #[test]
    fn app_names() {
        assert_eq!(friendly_app_name("Spotify.exe"), "Spotify");
        assert_eq!(friendly_app_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "ZuneMusic");
        assert_eq!(friendly_app_name("C:#Program Files#Zoom#bin#Zoom.exe"), "Zoom");
        assert_eq!(friendly_app_name("Chrome"), "Chrome");
        assert_eq!(exe_name("C:\\Games\\Foo.EXE"), "foo.exe");
    }

    #[test]
    fn ellipsis() {
        assert_eq!(ellipsize("abcdef", 4), "abc…");
        assert_eq!(ellipsize("abc", 4), "abc");
    }
}
