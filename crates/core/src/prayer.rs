//! Jadwal sholat: prayer times computed offline from coordinates, using the
//! standard solar-position method (as in PrayTimes.org). The default is the
//! Kemenag RI convention: Subuh at 20°, Isya at 18°, plus 2 minutes ihtiyat.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// Kementerian Agama RI (Indonesia): Fajr 20°, Isha 18°.
    #[default]
    Kemenag,
    /// JAKIM (Malaysia): Fajr 20°, Isha 18°.
    Jakim,
    /// Muslim World League: Fajr 18°, Isha 17°.
    Mwl,
    /// ISNA (North America): Fajr 15°, Isha 15°.
    Isna,
    /// Egyptian General Authority: Fajr 19.5°, Isha 17.5°.
    Egypt,
    /// Umm al-Qura (Makkah): Fajr 18.5°, Isha 90 min after Maghrib.
    Makkah,
    /// University of Islamic Sciences, Karachi: Fajr 18°, Isha 18°.
    Karachi,
}

impl Method {
    pub const ALL: [Method; 7] =
        [Method::Kemenag, Method::Jakim, Method::Mwl, Method::Isna, Method::Egypt, Method::Makkah, Method::Karachi];

    pub fn key(self) -> &'static str {
        match self {
            Method::Kemenag => "kemenag",
            Method::Jakim => "jakim",
            Method::Mwl => "mwl",
            Method::Isna => "isna",
            Method::Egypt => "egypt",
            Method::Makkah => "makkah",
            Method::Karachi => "karachi",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Method::Kemenag => "Kemenag RI (Indonesia)",
            Method::Jakim => "JAKIM (Malaysia)",
            Method::Mwl => "Muslim World League",
            Method::Isna => "ISNA (North America)",
            Method::Egypt => "Egyptian Authority",
            Method::Makkah => "Umm al-Qura (Makkah)",
            Method::Karachi => "Karachi",
        }
    }

    fn fajr_angle(self) -> f64 {
        match self {
            Method::Kemenag | Method::Jakim => 20.0,
            Method::Mwl | Method::Karachi => 18.0,
            Method::Isna => 15.0,
            Method::Egypt => 19.5,
            Method::Makkah => 18.5,
        }
    }

    /// Isha as a sun angle, or minutes after Maghrib.
    fn isha(self) -> Isha {
        match self {
            Method::Kemenag | Method::Jakim | Method::Karachi => Isha::Angle(18.0),
            Method::Mwl => Isha::Angle(17.0),
            Method::Isna => Isha::Angle(15.0),
            Method::Egypt => Isha::Angle(17.5),
            Method::Makkah => Isha::Minutes(90.0),
        }
    }

    /// Safety margin added to each time (ihtiyat), in minutes.
    fn ihtiyat(self) -> f64 {
        match self {
            Method::Kemenag | Method::Jakim => 2.0,
            _ => 0.0,
        }
    }
}

enum Isha {
    Angle(f64),
    Minutes(f64),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Asr {
    /// Shafi'i, Maliki, Hanbali: shadow = 1× object length.
    #[default]
    Standard,
    /// Hanafi: shadow = 2× object length.
    Hanafi,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prayer {
    Imsak,
    Subuh,
    Terbit,
    Dzuhur,
    Ashar,
    Maghrib,
    Isya,
}

impl Prayer {
    pub const ALL: [Prayer; 7] =
        [Prayer::Imsak, Prayer::Subuh, Prayer::Terbit, Prayer::Dzuhur, Prayer::Ashar, Prayer::Maghrib, Prayer::Isya];
    /// The five obligatory prayers, which get reminders.
    pub const FIVE: [Prayer; 5] = [Prayer::Subuh, Prayer::Dzuhur, Prayer::Ashar, Prayer::Maghrib, Prayer::Isya];

    pub fn name(self) -> &'static str {
        match self {
            Prayer::Imsak => "Imsak",
            Prayer::Subuh => "Subuh",
            Prayer::Terbit => "Terbit",
            Prayer::Dzuhur => "Dzuhur",
            Prayer::Ashar => "Ashar",
            Prayer::Maghrib => "Maghrib",
            Prayer::Isya => "Isya",
        }
    }
}

/// One day's times in local minutes after midnight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Day {
    pub times: [Option<f64>; 7],
}

impl Day {
    pub fn get(&self, p: Prayer) -> Option<f64> {
        self.times[Prayer::ALL.iter().position(|x| *x == p).unwrap_or(0)]
    }

    /// The next of the five prayers at or after `minute` (local), if any today.
    pub fn next(&self, minute: f64) -> Option<(Prayer, f64)> {
        Prayer::FIVE.iter().filter_map(|p| self.get(*p).map(|t| (*p, t))).find(|(_, t)| *t >= minute)
    }
}

fn rad(d: f64) -> f64 {
    d.to_radians()
}
fn deg(r: f64) -> f64 {
    r.to_degrees()
}
fn fix(a: f64, b: f64) -> f64 {
    let a = a - b * (a / b).floor();
    if a < 0.0 {
        a + b
    } else {
        a
    }
}

fn julian(y: i32, m: u32, d: u32) -> f64 {
    let (mut y, mut m) = (y as f64, m as f64);
    if m <= 2.0 {
        y -= 1.0;
        m += 12.0;
    }
    let a = (y / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    (365.25 * (y + 4716.0)).floor() + (30.6001 * (m + 1.0)).floor() + d as f64 + b - 1524.5
}

/// (declination in degrees, equation of time in hours)
fn sun(jd: f64) -> (f64, f64) {
    let d = jd - 2_451_545.0;
    let g = fix(357.529 + 0.985_600_28 * d, 360.0);
    let q = fix(280.459 + 0.985_647_36 * d, 360.0);
    let l = fix(q + 1.915 * rad(g).sin() + 0.020 * rad(2.0 * g).sin(), 360.0);
    let e = 23.439 - 0.000_000_36 * d;
    let ra = fix(deg((rad(e).cos() * rad(l).sin()).atan2(rad(l).cos())) / 15.0, 24.0);
    let eqt = q / 15.0 - ra;
    let decl = deg((rad(e).sin() * rad(l).sin()).asin());
    (decl, eqt)
}

/// Compute a day's prayer times.
/// `utc_offset_h` is the local time zone offset for that date (e.g. +7 for WIB).
#[allow(clippy::too_many_arguments)]
pub fn compute(y: i32, m: u32, d: u32, lat: f64, lon: f64, utc_offset_h: f64, method: Method, asr: Asr) -> Day {
    let jd = julian(y, m, d) - lon / (15.0 * 24.0);
    let mid_day = |t: f64| fix(12.0 - sun(jd + t).1, 24.0);
    let angle_time = |angle: f64, t: f64, ccw: bool| -> Option<f64> {
        let (decl, _) = sun(jd + t);
        let noon = mid_day(t);
        let x = (-rad(angle).sin() - rad(decl).sin() * rad(lat).sin()) / (rad(decl).cos() * rad(lat).cos());
        if !(-1.0..=1.0).contains(&x) {
            return None;
        }
        let h = deg(x.acos()) / 15.0;
        Some(if ccw { noon - h } else { noon + h })
    };
    let asr_time = |factor: f64, t: f64| -> Option<f64> {
        let (decl, _) = sun(jd + t);
        let angle = -deg((1.0 / (factor + rad((lat - decl).abs()).tan())).atan());
        angle_time(angle, t, false)
    };

    // Day portions of the usual times seed the solar position.
    let fajr = angle_time(method.fajr_angle(), 5.0 / 24.0, true);
    let sunrise = angle_time(0.833, 6.0 / 24.0, true);
    let dhuhr = Some(mid_day(12.0 / 24.0));
    let asr_t = asr_time(if asr == Asr::Hanafi { 2.0 } else { 1.0 }, 13.0 / 24.0);
    let sunset = angle_time(0.833, 18.0 / 24.0, false);
    let isha = match method.isha() {
        Isha::Angle(a) => angle_time(a, 18.0 / 24.0, false),
        Isha::Minutes(min) => sunset.map(|s| s + min / 60.0),
    };

    let shift = utc_offset_h - lon / 15.0;
    let ih = method.ihtiyat() / 60.0;
    let local = |t: Option<f64>, extra: f64| t.map(|h| fix(h + shift + extra, 24.0) * 60.0);
    let subuh = local(fajr, ih);
    let imsak = subuh.map(|s| s - 10.0);
    Day { times: [imsak, subuh, local(sunrise, -ih), local(dhuhr, ih), local(asr_t, ih), local(sunset, ih), local(isha, ih)] }
}

/// "HH:MM" for minutes after midnight (rounded up, as published schedules do).
pub fn hhmm(minutes: f64) -> String {
    let m = minutes.ceil() as i64;
    format!("{:02}:{:02}", (m / 60).rem_euclid(24), m.rem_euclid(60))
}

/// Indonesian cities (and a few neighbours) with coordinates and UTC offset.
pub const CITIES: &[(&str, f64, f64, f64)] = &[
    ("Banda Aceh", 5.5483, 95.3238, 7.0),
    ("Medan", 3.5952, 98.6722, 7.0),
    ("Padang", -0.9471, 100.4172, 7.0),
    ("Pekanbaru", 0.5071, 101.4478, 7.0),
    ("Batam", 1.0456, 104.0305, 7.0),
    ("Jambi", -1.6101, 103.6131, 7.0),
    ("Palembang", -2.9761, 104.7754, 7.0),
    ("Bengkulu", -3.7928, 102.2608, 7.0),
    ("Bandar Lampung", -5.3971, 105.2668, 7.0),
    ("Pangkal Pinang", -2.1316, 106.1169, 7.0),
    ("Jakarta", -6.2088, 106.8456, 7.0),
    ("Bogor", -6.5971, 106.8060, 7.0),
    ("Depok", -6.4025, 106.7942, 7.0),
    ("Tangerang", -6.1783, 106.6319, 7.0),
    ("Bekasi", -6.2383, 106.9756, 7.0),
    ("Serang", -6.1104, 106.1640, 7.0),
    ("Bandung", -6.9175, 107.6191, 7.0),
    ("Cirebon", -6.7320, 108.5523, 7.0),
    ("Tasikmalaya", -7.3274, 108.2207, 7.0),
    ("Semarang", -6.9667, 110.4167, 7.0),
    ("Yogyakarta", -7.7956, 110.3695, 7.0),
    ("Surakarta (Solo)", -7.5755, 110.8243, 7.0),
    ("Purwokerto", -7.4310, 109.2477, 7.0),
    ("Surabaya", -7.2575, 112.7521, 7.0),
    ("Malang", -7.9666, 112.6326, 7.0),
    ("Kediri", -7.8480, 112.0178, 7.0),
    ("Jember", -8.1724, 113.7000, 7.0),
    ("Pontianak", -0.0263, 109.3425, 7.0),
    ("Palangka Raya", -2.2161, 113.9135, 7.0),
    ("Denpasar", -8.6705, 115.2126, 8.0),
    ("Mataram", -8.5833, 116.1167, 8.0),
    ("Kupang", -10.1772, 123.6070, 8.0),
    ("Banjarmasin", -3.3186, 114.5944, 8.0),
    ("Balikpapan", -1.2379, 116.8529, 8.0),
    ("Samarinda", -0.5022, 117.1536, 8.0),
    ("Makassar", -5.1477, 119.4327, 8.0),
    ("Palu", -0.8917, 119.8707, 8.0),
    ("Kendari", -3.9985, 122.5129, 8.0),
    ("Manado", 1.4748, 124.8421, 8.0),
    ("Gorontalo", 0.5435, 123.0568, 8.0),
    ("Ambon", -3.6954, 128.1814, 9.0),
    ("Ternate", 0.7893, 127.3815, 9.0),
    ("Sorong", -0.8762, 131.2558, 9.0),
    ("Jayapura", -2.5337, 140.7181, 9.0),
    ("Merauke", -8.4991, 140.4045, 9.0),
    ("Kuala Lumpur", 3.1390, 101.6869, 8.0),
    ("Singapore", 1.3521, 103.8198, 8.0),
    ("Bandar Seri Begawan", 4.9031, 114.9398, 8.0),
];

pub fn city(name: &str) -> Option<(f64, f64, f64)> {
    CITIES.iter().find(|c| c.0.eq_ignore_ascii_case(name)).map(|c| (c.1, c.2, c.3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jakarta(y: i32, m: u32, d: u32) -> Day {
        compute(y, m, d, -6.2088, 106.8456, 7.0, Method::Kemenag, Asr::Standard)
    }

    fn within(actual: Option<f64>, expected: &str, tolerance_min: f64) {
        let (h, m) = expected.split_once(':').unwrap();
        let want = h.parse::<f64>().unwrap() * 60.0 + m.parse::<f64>().unwrap();
        let got = actual.expect("time");
        assert!((got - want).abs() <= tolerance_min, "got {} want {expected}", hhmm(got));
    }

    #[test]
    fn jakarta_matches_independent_solar_calculation() {
        // Expected values: NOAA solar calculator for Jakarta, 1 January 2025
        // (noon 11:56, sunrise 05:41, sunset 18:10, sun at -20° 04:16, -18° 19:26),
        // plus Kemenag's 2-minute ihtiyat (minus 2 for sunrise).
        let d = jakarta(2025, 1, 1);
        within(d.get(Prayer::Subuh), "04:18", 2.0);
        within(d.get(Prayer::Imsak), "04:08", 2.0);
        within(d.get(Prayer::Terbit), "05:39", 2.0);
        within(d.get(Prayer::Dzuhur), "11:58", 2.0);
        within(d.get(Prayer::Maghrib), "18:12", 2.0);
        within(d.get(Prayer::Isya), "19:28", 2.0);
        let asr = d.get(Prayer::Ashar).unwrap();
        assert!(asr > 15.0 * 60.0 && asr < 15.75 * 60.0, "Ashar {}", hhmm(asr));
    }

    #[test]
    fn times_are_in_order_everywhere() {
        for (name, lat, lon, tz) in CITIES {
            for (y, m, d) in [(2026, 1, 15), (2026, 4, 1), (2026, 7, 1), (2026, 10, 24)] {
                let day = compute(y, m, d, *lat, *lon, *tz, Method::Kemenag, Asr::Standard);
                let t: Vec<f64> = day.times.iter().map(|x| x.unwrap_or_else(|| panic!("{name}: missing time"))).collect();
                assert!(t.windows(2).all(|w| w[0] < w[1]), "{name} {y}-{m}-{d}: {:?}", t.iter().map(|x| hhmm(*x)).collect::<Vec<_>>());
                assert!(t[1] > 3.0 * 60.0 && t[1] < 6.5 * 60.0, "{name}: Subuh {}", hhmm(t[1]));
                assert!(t[5] > 17.0 * 60.0 && t[5] < 20.0 * 60.0, "{name}: Maghrib {}", hhmm(t[5]));
            }
        }
    }

    #[test]
    fn hanafi_asr_is_later_and_makkah_isha_is_90_min() {
        let s = compute(2026, 3, 1, 21.4225, 39.8262, 3.0, Method::Makkah, Asr::Standard);
        let h = compute(2026, 3, 1, 21.4225, 39.8262, 3.0, Method::Makkah, Asr::Hanafi);
        assert!(h.get(Prayer::Ashar).unwrap() > s.get(Prayer::Ashar).unwrap() + 20.0);
        let gap = s.get(Prayer::Isya).unwrap() - s.get(Prayer::Maghrib).unwrap();
        assert!((gap - 90.0).abs() < 0.01);
    }

    #[test]
    fn next_prayer_and_formatting() {
        let d = jakarta(2025, 1, 1);
        assert_eq!(d.next(12.0 * 60.0).unwrap().0, Prayer::Ashar);
        assert_eq!(d.next(0.0).unwrap().0, Prayer::Subuh);
        assert!(d.next(23.0 * 60.0).is_none());
        assert_eq!(hhmm(4.0 * 60.0 + 13.2), "04:14");
        assert_eq!(city("jakarta").unwrap().2, 7.0);
    }

    #[test]
    fn polar_days_do_not_panic() {
        let d = compute(2026, 6, 21, 78.2, 15.6, 2.0, Method::Mwl, Asr::Standard);
        assert!(d.get(Prayer::Dzuhur).is_some());
    }
}
