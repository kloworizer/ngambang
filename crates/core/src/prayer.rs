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

/// Places with coordinates and standard UTC offset: Indonesia in depth,
/// then its neighbours, the Middle East and large cities elsewhere.
/// (name, country, latitude, longitude, UTC offset in hours)
pub const CITIES: &[(&str, &str, f64, f64, f64)] = &[
    ("Banda Aceh", "Indonesia", 5.548, 95.32, 7e+00),
    ("Medan", "Indonesia", 3.595, 98.67, 7e+00),
    ("Padang", "Indonesia", -0.9471, 100.4, 7e+00),
    ("Pekanbaru", "Indonesia", 0.5071, 101.4, 7e+00),
    ("Batam", "Indonesia", 1.046, 104.0, 7e+00),
    ("Jambi", "Indonesia", -1.61, 103.6, 7e+00),
    ("Palembang", "Indonesia", -2.976, 104.8, 7e+00),
    ("Bengkulu", "Indonesia", -3.793, 102.3, 7e+00),
    ("Bandar Lampung", "Indonesia", -5.397, 105.3, 7e+00),
    ("Pangkal Pinang", "Indonesia", -2.132, 106.1, 7e+00),
    ("Jakarta", "Indonesia", -6.209, 106.8, 7e+00),
    ("Bogor", "Indonesia", -6.597, 106.8, 7e+00),
    ("Depok", "Indonesia", -6.402, 106.8, 7e+00),
    ("Tangerang", "Indonesia", -6.178, 106.6, 7e+00),
    ("Bekasi", "Indonesia", -6.238, 107.0, 7e+00),
    ("Serang", "Indonesia", -6.11, 106.2, 7e+00),
    ("Bandung", "Indonesia", -6.918, 107.6, 7e+00),
    ("Cirebon", "Indonesia", -6.732, 108.6, 7e+00),
    ("Tasikmalaya", "Indonesia", -7.327, 108.2, 7e+00),
    ("Semarang", "Indonesia", -6.967, 110.4, 7e+00),
    ("Yogyakarta", "Indonesia", -7.796, 110.4, 7e+00),
    ("Surakarta (Solo)", "Indonesia", -7.575, 110.8, 7e+00),
    ("Purwokerto", "Indonesia", -7.431, 109.2, 7e+00),
    ("Surabaya", "Indonesia", -7.258, 112.8, 7e+00),
    ("Malang", "Indonesia", -7.967, 112.6, 7e+00),
    ("Kediri", "Indonesia", -7.848, 112.0, 7e+00),
    ("Jember", "Indonesia", -8.172, 113.7, 7e+00),
    ("Pontianak", "Indonesia", -0.0263, 109.3, 7e+00),
    ("Palangka Raya", "Indonesia", -2.216, 113.9, 7e+00),
    ("Denpasar", "Indonesia", -8.671, 115.2, 8e+00),
    ("Mataram", "Indonesia", -8.583, 116.1, 8e+00),
    ("Kupang", "Indonesia", -10.18, 123.6, 8e+00),
    ("Banjarmasin", "Indonesia", -3.319, 114.6, 8e+00),
    ("Balikpapan", "Indonesia", -1.238, 116.9, 8e+00),
    ("Samarinda", "Indonesia", -0.5022, 117.2, 8e+00),
    ("Makassar", "Indonesia", -5.148, 119.4, 8e+00),
    ("Palu", "Indonesia", -0.8917, 119.9, 8e+00),
    ("Kendari", "Indonesia", -3.998, 122.5, 8e+00),
    ("Manado", "Indonesia", 1.475, 124.8, 8e+00),
    ("Gorontalo", "Indonesia", 0.5435, 123.1, 8e+00),
    ("Ambon", "Indonesia", -3.695, 128.2, 9e+00),
    ("Ternate", "Indonesia", 0.7893, 127.4, 9e+00),
    ("Sorong", "Indonesia", -0.8762, 131.3, 9e+00),
    ("Jayapura", "Indonesia", -2.534, 140.7, 9e+00),
    ("Merauke", "Indonesia", -8.499, 140.4, 9e+00),
    ("Sabang", "Indonesia", 5.893, 95.32, 7e+00),
    ("Lhokseumawe", "Indonesia", 5.18, 97.15, 7e+00),
    ("Langsa", "Indonesia", 4.468, 97.97, 7e+00),
    ("Binjai", "Indonesia", 3.6, 98.49, 7e+00),
    ("Pematangsiantar", "Indonesia", 2.959, 99.07, 7e+00),
    ("Sibolga", "Indonesia", 1.743, 98.78, 7e+00),
    ("Bukittinggi", "Indonesia", -0.3055, 100.4, 7e+00),
    ("Dumai", "Indonesia", 1.667, 101.5, 7e+00),
    ("Tanjung Pinang", "Indonesia", 0.9186, 104.5, 7e+00),
    ("Lubuklinggau", "Indonesia", -3.297, 102.9, 7e+00),
    ("Prabumulih", "Indonesia", -3.432, 104.2, 7e+00),
    ("Metro", "Indonesia", -5.113, 105.3, 7e+00),
    ("Cilegon", "Indonesia", -6.003, 106.0, 7e+00),
    ("Sukabumi", "Indonesia", -6.928, 106.9, 7e+00),
    ("Cimahi", "Indonesia", -6.872, 107.5, 7e+00),
    ("Karawang", "Indonesia", -6.323, 107.3, 7e+00),
    ("Purwakarta", "Indonesia", -6.557, 107.4, 7e+00),
    ("Garut", "Indonesia", -7.228, 107.9, 7e+00),
    ("Kuningan", "Indonesia", -6.976, 108.5, 7e+00),
    ("Indramayu", "Indonesia", -6.326, 108.3, 7e+00),
    ("Tegal", "Indonesia", -6.869, 109.1, 7e+00),
    ("Pekalongan", "Indonesia", -6.889, 109.7, 7e+00),
    ("Magelang", "Indonesia", -7.48, 110.2, 7e+00),
    ("Salatiga", "Indonesia", -7.33, 110.5, 7e+00),
    ("Kudus", "Indonesia", -6.805, 110.8, 7e+00),
    ("Klaten", "Indonesia", -7.706, 110.6, 7e+00),
    ("Cilacap", "Indonesia", -7.727, 109.0, 7e+00),
    ("Madiun", "Indonesia", -7.63, 111.5, 7e+00),
    ("Blitar", "Indonesia", -8.095, 112.2, 7e+00),
    ("Probolinggo", "Indonesia", -7.754, 113.2, 7e+00),
    ("Pasuruan", "Indonesia", -7.645, 112.9, 7e+00),
    ("Mojokerto", "Indonesia", -7.466, 112.4, 7e+00),
    ("Sidoarjo", "Indonesia", -7.448, 112.7, 7e+00),
    ("Gresik", "Indonesia", -7.154, 112.7, 7e+00),
    ("Banyuwangi", "Indonesia", -8.219, 114.4, 7e+00),
    ("Tuban", "Indonesia", -6.898, 112.1, 7e+00),
    ("Pamekasan", "Indonesia", -7.157, 113.5, 7e+00),
    ("Singkawang", "Indonesia", 0.906, 109.0, 7e+00),
    ("Ketapang", "Indonesia", -1.852, 110.0, 7e+00),
    ("Sampit", "Indonesia", -2.539, 112.9, 7e+00),
    ("Singaraja", "Indonesia", -8.112, 115.1, 8e+00),
    ("Bima", "Indonesia", -8.46, 118.7, 8e+00),
    ("Sumbawa Besar", "Indonesia", -8.493, 117.4, 8e+00),
    ("Labuan Bajo", "Indonesia", -8.496, 119.9, 8e+00),
    ("Maumere", "Indonesia", -8.62, 122.2, 8e+00),
    ("Ende", "Indonesia", -8.843, 121.7, 8e+00),
    ("Tarakan", "Indonesia", 3.3, 117.6, 8e+00),
    ("Tanjung Selor", "Indonesia", 2.837, 117.4, 8e+00),
    ("Bontang", "Indonesia", 0.1333, 117.5, 8e+00),
    ("Nunukan", "Indonesia", 4.133, 117.7, 8e+00),
    ("Parepare", "Indonesia", -4.013, 119.6, 8e+00),
    ("Palopo", "Indonesia", -2.993, 120.2, 8e+00),
    ("Bone (Watampone)", "Indonesia", -4.539, 120.3, 8e+00),
    ("Baubau", "Indonesia", -5.47, 122.6, 8e+00),
    ("Bitung", "Indonesia", 1.44, 125.1, 8e+00),
    ("Tomohon", "Indonesia", 1.324, 124.8, 8e+00),
    ("Luwuk", "Indonesia", -0.9516, 122.8, 8e+00),
    ("Mamuju", "Indonesia", -2.675, 118.9, 8e+00),
    ("Tual", "Indonesia", -5.64, 132.8, 9e+00),
    ("Manokwari", "Indonesia", -0.8615, 134.1, 9e+00),
    ("Timika", "Indonesia", -4.547, 136.9, 9e+00),
    ("Nabire", "Indonesia", -3.367, 135.5, 9e+00),
    ("Wamena", "Indonesia", -4.095, 138.9, 9e+00),
    ("Biak", "Indonesia", -1.18, 136.1, 9e+00),
    ("Fakfak", "Indonesia", -2.926, 132.3, 9e+00),
    ("Kuala Lumpur", "Malaysia", 3.139, 101.7, 8e+00),
    ("Shah Alam", "Malaysia", 3.073, 101.5, 8e+00),
    ("Putrajaya", "Malaysia", 2.926, 101.7, 8e+00),
    ("Johor Bahru", "Malaysia", 1.493, 103.7, 8e+00),
    ("Melaka", "Malaysia", 2.19, 102.3, 8e+00),
    ("Ipoh", "Malaysia", 4.598, 101.1, 8e+00),
    ("George Town (Penang)", "Malaysia", 5.414, 100.3, 8e+00),
    ("Kota Bharu", "Malaysia", 6.125, 102.2, 8e+00),
    ("Kuala Terengganu", "Malaysia", 5.33, 103.1, 8e+00),
    ("Kuantan", "Malaysia", 3.808, 103.3, 8e+00),
    ("Kota Kinabalu", "Malaysia", 5.98, 116.1, 8e+00),
    ("Kuching", "Malaysia", 1.553, 110.4, 8e+00),
    ("Singapore", "Singapore", 1.352, 103.8, 8e+00),
    ("Bandar Seri Begawan", "Brunei", 4.903, 114.9, 8e+00),
    ("Dili", "Timor-Leste", -8.557, 125.6, 9e+00),
    ("Bangkok", "Thailand", 13.76, 100.5, 7e+00),
    ("Hat Yai", "Thailand", 7.009, 100.5, 7e+00),
    ("Pattani", "Thailand", 6.87, 101.3, 7e+00),
    ("Manila", "Philippines", 14.6, 121.0, 8e+00),
    ("Davao", "Philippines", 7.191, 125.5, 8e+00),
    ("Cotabato", "Philippines", 7.224, 124.2, 8e+00),
    ("Zamboanga", "Philippines", 6.921, 122.1, 8e+00),
    ("Ho Chi Minh City", "Vietnam", 10.82, 106.6, 7e+00),
    ("Hanoi", "Vietnam", 21.03, 105.8, 7e+00),
    ("Phnom Penh", "Cambodia", 11.56, 104.9, 7e+00),
    ("Yangon", "Myanmar", 16.84, 96.17, 6e+00),
    ("Makkah", "Saudi Arabia", 21.42, 39.83, 3e+00),
    ("Madinah", "Saudi Arabia", 24.47, 39.61, 3e+00),
    ("Jeddah", "Saudi Arabia", 21.49, 39.19, 3e+00),
    ("Riyadh", "Saudi Arabia", 24.71, 46.68, 3e+00),
    ("Dubai", "United Arab Emirates", 25.2, 55.27, 4e+00),
    ("Abu Dhabi", "United Arab Emirates", 24.45, 54.38, 4e+00),
    ("Doha", "Qatar", 25.29, 51.53, 3e+00),
    ("Kuwait City", "Kuwait", 29.38, 47.98, 3e+00),
    ("Manama", "Bahrain", 26.23, 50.59, 3e+00),
    ("Muscat", "Oman", 23.59, 58.38, 4e+00),
    ("Sanaa", "Yemen", 15.37, 44.19, 3e+00),
    ("Amman", "Jordan", 31.95, 35.93, 3e+00),
    ("Jerusalem (Al-Quds)", "Palestine", 31.77, 35.21, 2e+00),
    ("Gaza", "Palestine", 31.5, 34.47, 2e+00),
    ("Beirut", "Lebanon", 33.89, 35.5, 2e+00),
    ("Damascus", "Syria", 33.51, 36.28, 3e+00),
    ("Baghdad", "Iraq", 33.32, 44.37, 3e+00),
    ("Tehran", "Iran", 35.69, 51.39, 4e+00),
    ("Istanbul", "Turkey", 41.01, 28.98, 3e+00),
    ("Ankara", "Turkey", 39.93, 32.86, 3e+00),
    ("Cairo", "Egypt", 30.04, 31.24, 2e+00),
    ("Alexandria", "Egypt", 31.2, 29.92, 2e+00),
    ("Khartoum", "Sudan", 15.5, 32.56, 2e+00),
    ("Tripoli", "Libya", 32.89, 13.19, 2e+00),
    ("Tunis", "Tunisia", 36.81, 10.18, 1e+00),
    ("Algiers", "Algeria", 36.75, 3.059, 1e+00),
    ("Casablanca", "Morocco", 33.57, -7.59, 1e+00),
    ("Rabat", "Morocco", 34.02, -6.842, 1e+00),
    ("Dakar", "Senegal", 14.72, -17.47, 0e+00),
    ("Lagos", "Nigeria", 6.524, 3.379, 1e+00),
    ("Kano", "Nigeria", 12.0, 8.592, 1e+00),
    ("Addis Ababa", "Ethiopia", 9.03, 38.74, 3e+00),
    ("Mogadishu", "Somalia", 2.047, 45.32, 3e+00),
    ("Nairobi", "Kenya", -1.292, 36.82, 3e+00),
    ("Dar es Salaam", "Tanzania", -6.792, 39.21, 3e+00),
    ("Johannesburg", "South Africa", -26.2, 28.05, 2e+00),
    ("Cape Town", "South Africa", -33.92, 18.42, 2e+00),
    ("Karachi", "Pakistan", 24.86, 67.0, 5e+00),
    ("Lahore", "Pakistan", 31.52, 74.36, 5e+00),
    ("Islamabad", "Pakistan", 33.68, 73.05, 5e+00),
    ("Kabul", "Afghanistan", 34.56, 69.21, 4e+00),
    ("Dhaka", "Bangladesh", 23.81, 90.41, 6e+00),
    ("Chittagong", "Bangladesh", 22.36, 91.78, 6e+00),
    ("New Delhi", "India", 28.61, 77.21, 6e+00),
    ("Mumbai", "India", 19.08, 72.88, 6e+00),
    ("Hyderabad", "India", 17.39, 78.49, 6e+00),
    ("Kolkata", "India", 22.57, 88.36, 6e+00),
    ("Chennai", "India", 13.08, 80.27, 6e+00),
    ("Colombo", "Sri Lanka", 6.927, 79.86, 6e+00),
    ("Male", "Maldives", 4.176, 73.51, 5e+00),
    ("Tashkent", "Uzbekistan", 41.3, 69.24, 5e+00),
    ("Almaty", "Kazakhstan", 43.22, 76.85, 5e+00),
    ("Beijing", "China", 39.9, 116.4, 8e+00),
    ("Shanghai", "China", 31.23, 121.5, 8e+00),
    ("Guangzhou", "China", 23.13, 113.3, 8e+00),
    ("Hong Kong", "Hong Kong", 22.32, 114.2, 8e+00),
    ("Taipei", "Taiwan", 25.03, 121.6, 8e+00),
    ("Seoul", "South Korea", 37.57, 127.0, 9e+00),
    ("Tokyo", "Japan", 35.68, 139.7, 9e+00),
    ("Osaka", "Japan", 34.69, 135.5, 9e+00),
    ("Sydney", "Australia", -33.87, 151.2, 1e+01),
    ("Melbourne", "Australia", -37.81, 145.0, 1e+01),
    ("Brisbane", "Australia", -27.47, 153.0, 1e+01),
    ("Perth", "Australia", -31.95, 115.9, 8e+00),
    ("Darwin", "Australia", -12.46, 130.8, 1e+01),
    ("Auckland", "New Zealand", -36.85, 174.8, 1e+01),
    ("London", "United Kingdom", 51.51, -0.1278, 0e+00),
    ("Birmingham", "United Kingdom", 52.49, -1.89, 0e+00),
    ("Manchester", "United Kingdom", 53.48, -2.243, 0e+00),
    ("Dublin", "Ireland", 53.35, -6.26, 0e+00),
    ("Paris", "France", 48.86, 2.352, 1e+00),
    ("Marseille", "France", 43.3, 5.37, 1e+00),
    ("Brussels", "Belgium", 50.85, 4.352, 1e+00),
    ("Amsterdam", "Netherlands", 52.37, 4.904, 1e+00),
    ("Rotterdam", "Netherlands", 51.92, 4.478, 1e+00),
    ("Berlin", "Germany", 52.52, 13.4, 1e+00),
    ("Frankfurt", "Germany", 50.11, 8.682, 1e+00),
    ("Zurich", "Switzerland", 47.38, 8.542, 1e+00),
    ("Vienna", "Austria", 48.21, 16.37, 1e+00),
    ("Madrid", "Spain", 40.42, -3.704, 1e+00),
    ("Barcelona", "Spain", 41.39, 2.169, 1e+00),
    ("Rome", "Italy", 41.9, 12.5, 1e+00),
    ("Sarajevo", "Bosnia and Herzegovina", 43.86, 18.41, 1e+00),
    ("Stockholm", "Sweden", 59.33, 18.07, 1e+00),
    ("Oslo", "Norway", 59.91, 10.75, 1e+00),
    ("Copenhagen", "Denmark", 55.68, 12.57, 1e+00),
    ("Moscow", "Russia", 55.76, 37.62, 3e+00),
    ("Kazan", "Russia", 55.8, 49.11, 3e+00),
    ("New York", "United States", 40.71, -74.01, -5e+00),
    ("Washington, D.C.", "United States", 38.91, -77.04, -5e+00),
    ("Chicago", "United States", 41.88, -87.63, -6e+00),
    ("Houston", "United States", 29.76, -95.37, -6e+00),
    ("Dallas", "United States", 32.78, -96.8, -6e+00),
    ("Detroit", "United States", 42.33, -83.05, -5e+00),
    ("Los Angeles", "United States", 34.05, -118.2, -8e+00),
    ("San Francisco", "United States", 37.77, -122.4, -8e+00),
    ("Seattle", "United States", 47.61, -122.3, -8e+00),
    ("Toronto", "Canada", 43.65, -79.38, -5e+00),
    ("Montreal", "Canada", 45.5, -73.57, -5e+00),
    ("Vancouver", "Canada", 49.28, -123.1, -8e+00),
    ("Mexico City", "Mexico", 19.43, -99.13, -6e+00),
    ("São Paulo", "Brazil", -23.55, -46.63, -3e+00),
    ("Buenos Aires", "Argentina", -34.6, -58.38, -3e+00),
];

pub fn city(name: &str) -> Option<(f64, f64, f64)> {
    CITIES.iter().find(|c| c.0.eq_ignore_ascii_case(name)).map(|c| (c.2, c.3, c.4))
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
        for (name, country, lat, lon, tz) in CITIES {
            for (y, m, d) in [(2026, 1, 15), (2026, 4, 1), (2026, 7, 1), (2026, 10, 24)] {
                let day = compute(y, m, d, *lat, *lon, *tz, Method::Kemenag, Asr::Standard);
                // Far from the equator summer twilight lasts all night: Subuh and Isya can be
                // missing or fall after midnight there.
                if lat.abs() > 45.0 {
                    let (dz, mg) = (day.get(Prayer::Dzuhur).unwrap(), day.get(Prayer::Maghrib).unwrap());
                    assert!(dz < mg, "{name} {y}-{m}-{d}");
                    continue;
                }
                let t: Vec<f64> = day.times.iter().map(|x| x.unwrap_or_else(|| panic!("{name}: missing time"))).collect();
                assert!(t.windows(2).all(|w| w[0] < w[1]), "{name} {y}-{m}-{d}: {:?}", t.iter().map(|x| hhmm(*x)).collect::<Vec<_>>());
                if *country == "Indonesia" {
                    assert!(t[1] > 3.0 * 60.0 && t[1] < 6.5 * 60.0, "{name}: Subuh {}", hhmm(t[1]));
                    assert!(t[5] > 17.0 * 60.0 && t[5] < 20.0 * 60.0, "{name}: Maghrib {}", hhmm(t[5]));
                } else {
                    // Solar noon within two hours of 12:00 in standard time.
                    assert!((t[3] - 12.0 * 60.0).abs() < 120.0 + 60.0, "{name}: Dzuhur {}", hhmm(t[3]));
                }
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
