//! Weather (Open-Meteo, worldwide) and early warnings: GDACS, the UN/EU
//! Global Disaster Alert and Coordination System (earthquakes, tsunamis,
//! cyclones, floods, volcanoes, droughts, wildfires), plus USGS for fast
//! earthquake reports. Pure parsing and decisions; the platform fetches.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const GDACS_ALERTS: &str =
    "https://www.gdacs.org/gdacsapi/api/events/geteventlist/SEARCH?eventlist=EQ;TC;FL;VO;DR;WF;TS&alertlevel=Orange;Red";
pub const USGS_QUAKES: &str = "https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/2.5_day.geojson";
/// Weather refresh interval while online.
pub const WEATHER_EVERY_MS: u64 = 30 * 60_000;
/// Warning feeds refresh interval while online.
pub const ALERTS_EVERY_MS: u64 = 5 * 60_000;
/// GDACS events older than this are no longer shown.
pub const ALERT_MAX_AGE_SECS: i64 = 3 * 86_400;
/// Data older than this is marked as stale.
pub const STALE_SECS: i64 = 2 * 3600;

pub fn open_meteo_url(lat: f64, lon: f64) -> String {
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat:.4}&longitude={lon:.4}\
         &current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m\
         &daily=temperature_2m_max,temperature_2m_min,precipitation_probability_max\
         &hourly=temperature_2m,weather_code&minutely_15=weather_code&forecast_days=2&timezone=auto"
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Condition {
    Clear,
    PartlyCloudy,
    Cloudy,
    Haze,
    Fog,
    Drizzle,
    Rain,
    HeavyRain,
    Thunder,
}

impl Condition {
    /// Short label (Indonesian, matching the rest of the island's local text).
    pub fn label(self) -> &'static str {
        match self {
            Condition::Clear => "Cerah",
            Condition::PartlyCloudy => "Cerah Berawan",
            Condition::Cloudy => "Berawan",
            Condition::Haze => "Udara Kabur",
            Condition::Fog => "Kabut",
            Condition::Drizzle => "Gerimis",
            Condition::Rain => "Hujan",
            Condition::HeavyRain => "Hujan Lebat",
            Condition::Thunder => "Hujan Petir",
        }
    }

    pub fn is_rain(self) -> bool {
        matches!(self, Condition::Drizzle | Condition::Rain | Condition::HeavyRain | Condition::Thunder)
    }

    /// WMO codes (Open-Meteo).
    pub fn from_wmo(code: i64) -> Condition {
        match code {
            0 => Condition::Clear,
            1 | 2 => Condition::PartlyCloudy,
            3 => Condition::Cloudy,
            45 | 48 => Condition::Fog,
            51..=57 => Condition::Drizzle,
            61 | 63 | 66 | 80 | 81 => Condition::Rain,
            65 | 67 | 82 => Condition::HeavyRain,
            95..=99 => Condition::Thunder,
            _ => Condition::Cloudy,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    /// Local "HH:MM".
    pub time: String,
    pub temp_c: f32,
    pub condition: Condition,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    /// Data provider, shown as attribution.
    pub source: String,
    pub place: String,
    pub temp_c: f32,
    pub feels_c: Option<f32>,
    pub humidity: Option<u8>,
    pub wind_kmh: Option<f32>,
    pub condition: Condition,
    /// The provider's own description where it has one.
    pub description: String,
    pub min_c: Option<f32>,
    pub max_c: Option<f32>,
    pub rain_chance: Option<u8>,
    pub next: Vec<Slot>,
    /// Unix seconds when it was fetched.
    pub fetched: i64,
    /// When rain should start (unix seconds), if it is dry now and the
    /// forecast has rain within `RAIN_LOOKAHEAD_S`.
    #[serde(default)]
    pub rain_at: Option<i64>,
}

/// How far ahead `Weather::rain_at` looks.
pub const RAIN_LOOKAHEAD_S: i64 = 2 * 3600;

/// Seconds since 1970 for "YYYY-MM-DDTHH:MM[:SS][Z]" or "YYYY-MM-DD HH:MM:SS" (taken as UTC).
pub fn parse_utc(s: &str) -> Option<i64> {
    let s = s.trim().trim_end_matches('Z');
    let (date, time) = s.split_once(['T', ' '])?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|x| x.parse::<i64>().ok());
    let (h, mi) = (t.next()??, t.next()??);
    let sec = t.next().flatten().unwrap_or(0);
    // Days from civil (Howard Hinnant).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + da - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + sec)
}

fn hhmm_of(local: &str) -> String {
    local.split_once(['T', ' ']).map(|(_, t)| t.chars().take(5).collect()).unwrap_or_default()
}

fn f(v: &Value) -> Option<f32> {
    v.as_f64().map(|x| x as f32)
}

/// Open-Meteo forecast response (with `timezone=auto`).
pub fn parse_open_meteo(json: &str, place: &str, now: i64) -> Option<Weather> {
    let v: Value = serde_json::from_str(json).ok()?;
    if v.get("error").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let cur = v.get("current")?;
    let cur_time = cur.get("time").and_then(Value::as_str).unwrap_or("");
    let condition = Condition::from_wmo(cur.get("weather_code").and_then(Value::as_i64).unwrap_or(3));
    let daily = v.get("daily");
    let day0 = |k: &str| daily.and_then(|d| d.get(k)).and_then(|a| a.get(0)).and_then(f);
    let hourly = v.get("hourly");
    let mut next = Vec::new();
    if let (Some(times), Some(temps), Some(codes)) = (
        hourly.and_then(|h| h.get("time")).and_then(Value::as_array),
        hourly.and_then(|h| h.get("temperature_2m")).and_then(Value::as_array),
        hourly.and_then(|h| h.get("weather_code")).and_then(Value::as_array),
    ) {
        for (i, t) in times.iter().enumerate() {
            let Some(t) = t.as_str() else { continue };
            // Every third hour after now.
            if t > cur_time && next.len() < 4 && i % 3 == 0 {
                if let (Some(temp), Some(code)) = (temps.get(i).and_then(f), codes.get(i).and_then(Value::as_i64)) {
                    next.push(Slot { time: hhmm_of(t), temp_c: temp, condition: Condition::from_wmo(code) });
                }
            }
        }
    }
    // Rain soon: the 15-minute forecast where there is one, else the hourly one.
    let offset = v.get("utc_offset_seconds").and_then(Value::as_i64).unwrap_or(0);
    let cur_unix = parse_utc(cur_time).map(|t| t - offset);
    let soon = |block: Option<&Value>| -> Option<i64> {
        let times = block?.get("time")?.as_array()?;
        let codes = block?.get("weather_code")?.as_array()?;
        let now = cur_unix?;
        times.iter().zip(codes).find_map(|(t, c)| {
            let at = parse_utc(t.as_str()?)? - offset;
            let rain = Condition::from_wmo(c.as_i64()?).is_rain();
            (at > now && at - now <= RAIN_LOOKAHEAD_S && rain).then_some(at)
        })
    };
    let rain_at = if condition.is_rain() { None } else { soon(v.get("minutely_15")).or_else(|| soon(hourly)) };
    Some(Weather {
        rain_at,
        source: "Open-Meteo".into(),
        place: place.to_string(),
        temp_c: cur.get("temperature_2m").and_then(f)?,
        feels_c: cur.get("apparent_temperature").and_then(f),
        humidity: cur.get("relative_humidity_2m").and_then(Value::as_u64).map(|h| h.min(100) as u8),
        wind_kmh: cur.get("wind_speed_10m").and_then(f),
        condition,
        description: condition.label().into(),
        min_c: day0("temperature_2m_min"),
        max_c: day0("temperature_2m_max"),
        rain_chance: day0("precipitation_probability_max").map(|p| p.clamp(0.0, 100.0) as u8),
        next,
        fetched: now,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hazard {
    Earthquake,
    Tsunami,
    Cyclone,
    Flood,
    Volcano,
    Drought,
    Wildfire,
}

impl Hazard {
    pub fn label(self) -> &'static str {
        match self {
            Hazard::Earthquake => "Earthquake",
            Hazard::Tsunami => "Tsunami",
            Hazard::Cyclone => "Tropical cyclone",
            Hazard::Flood => "Flood",
            Hazard::Volcano => "Volcano",
            Hazard::Drought => "Drought",
            Hazard::Wildfire => "Wildfire",
        }
    }

    fn from_gdacs(code: &str) -> Option<Hazard> {
        Some(match code {
            "EQ" => Hazard::Earthquake,
            "TS" => Hazard::Tsunami,
            "TC" => Hazard::Cyclone,
            "FL" => Hazard::Flood,
            "VO" => Hazard::Volcano,
            "DR" => Hazard::Drought,
            "WF" => Hazard::Wildfire,
            _ => return None,
        })
    }

    /// How far away an event of this kind can still matter.
    pub fn reach_km(self, quake_radius_km: f64) -> f64 {
        match self {
            Hazard::Earthquake => quake_radius_km,
            Hazard::Tsunami => 3000.0,
            Hazard::Cyclone => 1000.0,
            Hazard::Flood | Hazard::Wildfire => 300.0,
            Hazard::Volcano => 200.0,
            Hazard::Drought => 500.0,
        }
    }
}

/// GDACS colour levels; USGS reports are mapped onto them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Level {
    Green,
    Orange,
    Red,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    /// Stable id, e.g. "gdacs:EQ:1570339:1738684" or "usgs:us6000u4m9".
    pub id: String,
    pub hazard: Hazard,
    pub level: Level,
    pub title: String,
    pub lat: f64,
    pub lon: f64,
    /// Unix seconds when it happened (or started).
    pub time: i64,
    pub magnitude: Option<f32>,
    /// "GDACS" or "USGS".
    pub source: String,
    pub url: String,
    /// The provider flagged a possible tsunami.
    pub tsunami: bool,
}

fn features(json: &str) -> Option<Vec<Value>> {
    let v: Value = serde_json::from_str(json).ok()?;
    v.get("features")?.as_array().cloned()
}

fn point(feature: &Value) -> Option<(f64, f64)> {
    let c = feature.get("geometry")?.get("coordinates")?.as_array()?;
    Some((c.get(1)?.as_f64()?, c.first()?.as_f64()?))
}

/// GDACS event list. Events that ended more than `ALERT_MAX_AGE_SECS` ago are dropped.
pub fn parse_gdacs(json: &str, now: i64) -> Option<Vec<Alert>> {
    let mut out = Vec::new();
    for f in features(json)? {
        let p = match f.get("properties") {
            Some(p) => p,
            None => continue,
        };
        let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let Some(hazard) = Hazard::from_gdacs(&s("eventtype")) else { continue };
        let level = match s("alertlevel").as_str() {
            "Red" => Level::Red,
            "Orange" => Level::Orange,
            _ => Level::Green,
        };
        let Some((lat, lon)) = point(&f) else { continue };
        let end = parse_utc(&s("todate")).or_else(|| parse_utc(&s("fromdate"))).unwrap_or(0);
        if s("iscurrent") != "true" && now - end > ALERT_MAX_AGE_SECS {
            continue;
        }
        let magnitude =
            (hazard == Hazard::Earthquake).then(|| p.get("severitydata")?.get("severity")?.as_f64().map(|m| m as f32)).flatten();
        let id_num = |k: &str| p.get(k).map(|v| v.to_string()).unwrap_or_default();
        let name = s("name");
        let country = s("country");
        let title = if name.is_empty() { format!("{} in {country}", hazard.label()) } else { name };
        out.push(Alert {
            id: format!("gdacs:{}:{}:{}", s("eventtype"), id_num("eventid"), id_num("episodeid")),
            hazard,
            level,
            title,
            lat,
            lon,
            time: parse_utc(&s("fromdate")).unwrap_or(end),
            magnitude,
            source: "GDACS".into(),
            url: p.get("url").and_then(|u| u.get("report")).and_then(Value::as_str).unwrap_or("https://www.gdacs.org").to_string(),
            tsunami: hazard == Hazard::Tsunami,
        });
    }
    Some(out)
}

/// USGS earthquake GeoJSON (M2.5+, past day).
pub fn parse_usgs(json: &str) -> Option<Vec<Alert>> {
    let mut out = Vec::new();
    for f in features(json)? {
        let Some(p) = f.get("properties") else { continue };
        let Some(mag) = p.get("mag").and_then(Value::as_f64) else { continue };
        let Some((lat, lon)) = point(&f) else { continue };
        let tsunami = p.get("tsunami").and_then(Value::as_i64) == Some(1);
        // USGS PAGER colours, else a rough magnitude scale.
        let level = match p.get("alert").and_then(Value::as_str) {
            Some("red") => Level::Red,
            Some("orange") => Level::Orange,
            _ if mag >= 7.0 => Level::Orange,
            _ => Level::Green,
        };
        out.push(Alert {
            id: format!("usgs:{}", f.get("id").and_then(Value::as_str).unwrap_or("")),
            hazard: Hazard::Earthquake,
            level: if tsunami { level.max(Level::Orange) } else { level },
            title: format!("M{mag:.1} · {}", p.get("place").and_then(Value::as_str).unwrap_or("earthquake")),
            lat,
            lon,
            time: p.get("time").and_then(Value::as_i64).unwrap_or(0) / 1000,
            magnitude: Some(mag as f32),
            source: "USGS".into(),
            url: p.get("url").and_then(Value::as_str).unwrap_or("https://earthquake.usgs.gov").to_string(),
            tsunami,
        });
    }
    Some(out)
}

/// Great-circle distance in kilometres.
pub fn distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    6371.0 * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
}

/// Should this alert raise a tag for someone at (lat, lon)? Some(priority) or None.
/// Red or tsunami nearby: 1 (shown even during games). Orange nearby, or a
/// felt-range earthquake: 2. Everything else stays in the slip only.
pub fn alert_priority(a: &Alert, lat: f64, lon: f64, quake_radius_km: f64, min_magnitude: f32) -> Option<u8> {
    let d = distance_km(a.lat, a.lon, lat, lon);
    let reach = if a.tsunami { Hazard::Tsunami.reach_km(quake_radius_km) } else { a.hazard.reach_km(quake_radius_km) };
    if d > reach {
        return None;
    }
    if a.tsunami || a.level == Level::Red {
        return Some(1);
    }
    if a.level == Level::Orange {
        return Some(2);
    }
    (a.hazard == Hazard::Earthquake && a.magnitude.is_some_and(|m| m >= min_magnitude)).then_some(2)
}

/// Alerts worth listing for this location, most relevant first: nearby ones,
/// then worldwide Red events. At most `limit`.
pub fn relevant(alerts: &[Alert], lat: f64, lon: f64, quake_radius_km: f64, limit: usize) -> Vec<Alert> {
    let mut v: Vec<(bool, f64, &Alert)> = alerts
        .iter()
        .filter_map(|a| {
            let d = distance_km(a.lat, a.lon, lat, lon);
            let near = d <= a.hazard.reach_km(quake_radius_km) || (a.tsunami && d <= Hazard::Tsunami.reach_km(quake_radius_km));
            (near || a.level == Level::Red).then_some((near, d, a))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(b.2.level.cmp(&a.2.level)).then(a.1.total_cmp(&b.1)));
    let mut seen = Vec::new();
    v.into_iter()
        .filter(|(_, _, a)| {
            // The same quake often appears in both feeds.
            let dup =
                seen.iter().any(|(t, la, lo): &(i64, f64, f64)| (t - a.time).abs() < 300 && distance_km(*la, *lo, a.lat, a.lon) < 100.0);
            if !dup {
                seen.push((a.time, a.lat, a.lon));
            }
            !dup
        })
        .take(limit)
        .map(|(_, _, a)| a.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GDACS: &str = include_str!("../testdata/gdacs.json");
    const USGS: &str = include_str!("../testdata/usgs.json");
    const OPEN_METEO: &str = include_str!("../testdata/openmeteo.json");
    const OPEN_METEO_ERROR: &str = include_str!("../testdata/openmeteo_error.json");

    #[test]
    fn utc_parsing() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc("2023-11-14 22:13:20"), Some(1_700_000_000));
        assert_eq!(parse_utc("2023-11-14T22:13"), Some(1_700_000_000 - 20));
        assert_eq!(parse_utc("nope"), None);
    }

    #[test]
    fn open_meteo_forecast_and_errors() {
        let w = parse_open_meteo(OPEN_METEO, "Jakarta", 0).unwrap();
        assert_eq!(w.source, "Open-Meteo");
        assert!(w.rain_chance.is_some());
        assert!(w.next.iter().all(|s| s.time.len() == 5));
        assert_eq!(w.rain_at, None, "dry for the next two hours");
        assert!(parse_open_meteo(OPEN_METEO_ERROR, "Jakarta", 0).is_none());
        assert!(parse_open_meteo("<html>offline</html>", "Jakarta", 0).is_none());
        assert!(parse_open_meteo("", "x", 0).is_none());
    }

    #[test]
    fn rain_soon_from_the_15_minute_forecast() {
        let json = r#"{"utc_offset_seconds":25200,"current":{"time":"2026-10-24T13:00","temperature_2m":31.0,"weather_code":3},
            "minutely_15":{"time":["2026-10-24T13:00","2026-10-24T13:15","2026-10-24T13:30"],"weather_code":[3,3,61]}}"#;
        let w = parse_open_meteo(json, "Jakarta", 0).unwrap();
        assert_eq!(w.rain_at, Some(parse_utc("2026-10-24T06:30").unwrap()), "13:30 local is 06:30 UTC");
        // Already raining: nothing to announce.
        let wet = json.replace(r#""weather_code":3}"#, r#""weather_code":63}"#);
        assert_eq!(parse_open_meteo(&wet, "Jakarta", 0).unwrap().rain_at, None);
    }

    #[test]
    fn gdacs_feed() {
        let now = parse_utc("2026-10-10T00:00:00Z").unwrap();
        let a = parse_gdacs(GDACS, now).unwrap();
        assert!(!a.is_empty());
        assert!(a.iter().all(|x| x.id.starts_with("gdacs:") && x.source == "GDACS"));
        assert!(a.iter().any(|x| x.level >= Level::Orange));
        assert!(a.iter().all(|x| x.url.starts_with("https://")));
        // A month later the same past events are dropped.
        let later = parse_utc("2026-11-30T00:00:00Z").unwrap();
        assert!(parse_gdacs(GDACS, later).unwrap().is_empty());
        assert!(parse_gdacs("{\"message\":\"Eventtype is required.\"}", now).is_none());
    }

    #[test]
    fn usgs_feed() {
        let a = parse_usgs(USGS).unwrap();
        assert!(!a.is_empty());
        assert!(a.iter().all(|x| x.hazard == Hazard::Earthquake && x.magnitude.is_some() && x.title.starts_with('M')));
        assert!(parse_usgs("not json").is_none());
    }

    fn quake(lat: f64, lon: f64, mag: f32, level: Level) -> Alert {
        Alert {
            id: "t".into(),
            hazard: Hazard::Earthquake,
            level,
            title: "t".into(),
            lat,
            lon,
            time: 0,
            magnitude: Some(mag),
            source: "USGS".into(),
            url: String::new(),
            tsunami: false,
        }
    }

    #[test]
    fn alert_rules() {
        let (lat, lon) = (-6.2088, 106.8456);
        assert_eq!(alert_priority(&quake(-6.6, 106.9, 4.6, Level::Green), lat, lon, 300.0, 4.5), Some(2), "felt nearby");
        assert_eq!(alert_priority(&quake(-6.6, 106.9, 3.0, Level::Green), lat, lon, 300.0, 4.5), None, "too small");
        assert_eq!(alert_priority(&quake(35.0, 139.0, 7.5, Level::Red), lat, lon, 300.0, 4.5), None, "far away: slip only");
        let mut t = quake(-9.0, 108.0, 7.1, Level::Orange);
        t.tsunami = true;
        assert_eq!(alert_priority(&t, lat, lon, 300.0, 4.5), Some(1), "tsunami");
        let cyclone = Alert { hazard: Hazard::Cyclone, magnitude: None, ..quake(-10.0, 110.0, 0.0, Level::Orange) };
        assert_eq!(alert_priority(&cyclone, lat, lon, 300.0, 4.5), Some(2));
        let list = relevant(
            &[quake(35.0, 139.0, 7.5, Level::Red), quake(-6.6, 106.9, 4.6, Level::Green), quake(-6.61, 106.91, 4.6, Level::Green)],
            lat,
            lon,
            300.0,
            5,
        );
        assert_eq!(list.len(), 2, "duplicate removed, far Red kept");
        assert!(list[0].lat < 0.0, "nearby first");
        assert!((distance_km(-6.2088, 106.8456, -7.2575, 112.7521) - 663.0).abs() < 15.0);
    }

    #[test]
    fn conditions() {
        assert_eq!(Condition::from_wmo(65), Condition::HeavyRain);
        assert!(Condition::Drizzle.is_rain());
        assert_eq!(Condition::Cloudy.label(), "Berawan");
    }
}
