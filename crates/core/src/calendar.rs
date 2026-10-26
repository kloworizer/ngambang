//! Calendars from iCal (`.ics`) feeds: the "secret address in iCal format"
//! Google Calendar gives for each calendar (Outlook and iCloud publish the same
//! format). Read-only, no sign-in. Time zones and recurring events are
//! expanded here, so the platform only downloads text.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

/// Refresh linked calendars this often.
pub const REFRESH_MS: u64 = 15 * 60_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalEvent {
    pub title: String,
    pub location: String,
    /// Unix seconds.
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    /// Index into the configured feeds.
    pub feed: usize,
    /// The event's own reminders (VALARM), in seconds before the start.
    #[serde(default)]
    pub alarms: Vec<i64>,
    /// A video meeting to join (Meet, Zoom, Teams, Webex…), if any.
    #[serde(default)]
    pub link: String,
}

/// The first video-meeting link in some text.
pub fn meeting_link(text: &str) -> Option<String> {
    const HOSTS: [&str; 6] =
        ["meet.google.com/", "zoom.us/j/", "zoom.us/my/", "teams.microsoft.com/l/meetup-join", "teams.live.com/meet", "webex.com/"];
    let mut rest = text;
    while let Some(i) = rest.find("https://") {
        let url: String =
            rest[i..].chars().take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '<' | '>' | ')' | ',' | '\\')).collect();
        if HOSTS.iter().any(|h| url.contains(h)) {
            return Some(url);
        }
        rest = &rest[i + 8..];
    }
    None
}

/// `webcal://` links (what "subscribe" buttons hand out) are plain HTTPS.
pub fn feed_url(url: &str) -> String {
    let u = url.trim();
    match u.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => u.to_string(),
    }
}

// ---- dates --------------------------------------------------------------

/// Days since 1970-01-01 for a civil date (proleptic Gregorian).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day number.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// 0 = Monday … 6 = Sunday.
pub fn weekday(days: i64) -> u32 {
    (days + 3).rem_euclid(7) as u32
}

fn days_in_month(y: i64, m: u32) -> u32 {
    let next = if m == 12 { days_from_civil(y + 1, 1, 1) } else { days_from_civil(y, m + 1, 1) };
    (next - days_from_civil(y, m, 1)) as u32
}

/// The `n`th `wd` of a month (n < 0 counts from the end), as a day number.
fn nth_weekday(y: i64, m: u32, n: i32, wd: u32) -> Option<i64> {
    let first = days_from_civil(y, m, 1);
    let len = days_in_month(y, m) as i64;
    let day = if n > 0 {
        first + (wd as i64 - weekday(first) as i64).rem_euclid(7) + (n as i64 - 1) * 7
    } else {
        let last = first + len - 1;
        last - (weekday(last) as i64 - wd as i64).rem_euclid(7) + (n as i64 + 1) * 7
    };
    (day >= first && day < first + len).then_some(day)
}

fn weekday_code(s: &str) -> Option<u32> {
    ["MO", "TU", "WE", "TH", "FR", "SA", "SU"].iter().position(|w| *w == s).map(|i| i as u32)
}

/// "20261025" or "20261025T090000[Z]" → (local seconds since the epoch, is a date, is UTC).
fn parse_stamp(v: &str) -> Option<(i64, bool, bool)> {
    let v = v.trim();
    let num = |a: usize, b: usize| v.get(a..b)?.parse::<i64>().ok();
    let days = days_from_civil(num(0, 4)?, num(4, 6)? as u32, num(6, 8)? as u32);
    if v.len() == 8 {
        return Some((days * 86_400, true, false));
    }
    if v.as_bytes().get(8) != Some(&b'T') {
        return None;
    }
    let secs = num(9, 11)? * 3600 + num(11, 13)? * 60 + num(13, 15).unwrap_or(0);
    Some((days * 86_400 + secs, false, v.ends_with('Z')))
}

/// "PT1H30M", "P1D", "P2W" → seconds.
fn parse_duration(v: &str) -> Option<i64> {
    let v = v.trim().trim_start_matches('+');
    let neg = v.starts_with('-');
    let v = v.trim_start_matches('-').strip_prefix('P')?;
    let (mut total, mut num, mut in_time) = (0i64, 0i64, false);
    for c in v.chars() {
        match c {
            '0'..='9' => num = num * 10 + c.to_digit(10)? as i64,
            'T' => in_time = true,
            'W' => (total, num) = (total + num * 7 * 86_400, 0),
            'D' => (total, num) = (total + num * 86_400, 0),
            'H' if in_time => (total, num) = (total + num * 3600, 0),
            'M' if in_time => (total, num) = (total + num * 60, 0),
            'S' if in_time => (total, num) = (total + num, 0),
            _ => return None,
        }
    }
    Some(if neg { -total } else { total })
}

// ---- time zones ---------------------------------------------------------

/// One STANDARD or DAYLIGHT part of a VTIMEZONE.
#[derive(Clone, Debug, Default)]
struct ZoneRule {
    offset_to: i64,
    /// Local seconds of the first onset.
    start: i64,
    month: Option<u32>,
    nth: Option<(i32, u32)>,
    month_day: Option<i32>,
}

impl ZoneRule {
    /// The latest onset at or before local time `ls`.
    fn onset_before(&self, ls: i64) -> Option<i64> {
        let tod = self.start.rem_euclid(86_400);
        let Some(month) = self.month else { return (self.start <= ls).then_some(self.start) };
        let (year, _, _) = civil_from_days(ls.div_euclid(86_400));
        for y in [year, year - 1] {
            let day = match (self.nth, self.month_day) {
                (Some((n, wd)), _) => nth_weekday(y, month, n, wd),
                (None, Some(d)) if d > 0 => Some(days_from_civil(y, month, d as u32)),
                _ => Some(days_from_civil(y, month, civil_from_days(self.start.div_euclid(86_400)).2)),
            };
            if let Some(day) = day {
                let at = day * 86_400 + tod;
                if at <= ls && at >= self.start {
                    return Some(at);
                }
            }
        }
        None
    }
}

/// UTC offsets (minutes) for zones a feed names without defining; only zones
/// without daylight saving, so a fixed offset is right all year.
const FIXED_ZONES: &[(&str, i64)] = &[
    ("UTC", 0),
    ("Etc/UTC", 0),
    ("GMT", 0),
    ("Asia/Jakarta", 420),
    ("Asia/Pontianak", 420),
    ("Asia/Makassar", 480),
    ("Asia/Jayapura", 540),
    ("Asia/Singapore", 480),
    ("Asia/Kuala_Lumpur", 480),
    ("Asia/Kuching", 480),
    ("Asia/Brunei", 480),
    ("Asia/Bangkok", 420),
    ("Asia/Ho_Chi_Minh", 420),
    ("Asia/Manila", 480),
    ("Asia/Shanghai", 480),
    ("Asia/Hong_Kong", 480),
    ("Asia/Taipei", 480),
    ("Asia/Tokyo", 540),
    ("Asia/Seoul", 540),
    ("Asia/Kolkata", 330),
    ("Asia/Calcutta", 330),
    ("Asia/Karachi", 300),
    ("Asia/Dhaka", 360),
    ("Asia/Dubai", 240),
    ("Asia/Riyadh", 180),
    ("Asia/Qatar", 180),
    ("Europe/Istanbul", 180),
];

#[derive(Clone, Debug)]
enum Tz {
    Utc,
    /// No zone: the computer's local time.
    Floating,
    Named(String),
}

struct Zones {
    defined: HashMap<String, Vec<ZoneRule>>,
    local_offset: i64,
}

impl Zones {
    /// Local seconds in `tz` → unix seconds.
    fn to_unix(&self, ls: i64, tz: &Tz) -> i64 {
        let offset_min = match tz {
            Tz::Utc => 0,
            Tz::Floating => self.local_offset,
            Tz::Named(id) => match self.defined.get(id) {
                Some(rules) => rules
                    .iter()
                    .filter_map(|r| r.onset_before(ls).map(|at| (at, r.offset_to)))
                    .max_by_key(|(at, _)| *at)
                    .map(|(_, off)| off)
                    .or_else(|| rules.first().map(|r| r.offset_to))
                    .unwrap_or(self.local_offset),
                None => FIXED_ZONES.iter().find(|(n, _)| n.eq_ignore_ascii_case(id)).map(|(_, o)| *o).unwrap_or(self.local_offset),
            },
        };
        ls - offset_min * 60
    }
}

/// "+0700" / "-0430" → minutes.
fn parse_offset(v: &str) -> Option<i64> {
    let v = v.trim();
    let sign = if v.starts_with('-') { -1 } else { 1 };
    let d = v.trim_start_matches(['+', '-']);
    let h: i64 = d.get(0..2)?.parse().ok()?;
    let m: i64 = d.get(2..4)?.parse().ok()?;
    Some(sign * (h * 60 + m))
}

// ---- parsing ------------------------------------------------------------

struct Prop {
    name: String,
    params: HashMap<String, String>,
    value: String,
}

fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&line[1..]);
        } else if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

fn parse_prop(line: &str) -> Option<Prop> {
    // The value starts at the first ':' outside a quoted parameter.
    let mut quoted = false;
    let split = line.char_indices().find(|&(_, c)| {
        if c == '"' {
            quoted = !quoted;
        }
        c == ':' && !quoted
    })?;
    let (head, value) = (&line[..split.0], &line[split.0 + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_uppercase(), v.trim().trim_matches('"').to_string()))
        .collect();
    Some(Prop { name, params, value: value.to_string() })
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push(' '),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

#[derive(Default)]
struct RawEvent {
    uid: String,
    title: String,
    location: String,
    description: String,
    conference: String,
    start: Option<(i64, bool, Tz)>,
    end: Option<(i64, bool, Tz)>,
    duration: Option<i64>,
    rrule: Option<String>,
    exdates: Vec<(i64, Tz)>,
    recurrence_id: Option<(i64, Tz)>,
    cancelled: bool,
    alarms: Vec<Trigger>,
}

/// When a VALARM fires.
#[derive(Clone, Debug)]
enum Trigger {
    /// Seconds from the start (negative: before it).
    Start(i64),
    /// Seconds from the end.
    End(i64),
    /// A fixed moment, unix seconds.
    At(i64),
}

fn parse_trigger(p: &Prop) -> Option<Trigger> {
    if p.params.get("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE-TIME")) {
        let (ls, _, utc) = parse_stamp(&p.value)?;
        return utc.then_some(Trigger::At(ls));
    }
    let secs = parse_duration(&p.value)?;
    Some(if p.params.get("RELATED").is_some_and(|v| v.eq_ignore_ascii_case("END")) { Trigger::End(secs) } else { Trigger::Start(secs) })
}

fn stamp_with_tz(p: &Prop) -> Option<(i64, bool, Tz)> {
    let (ls, is_date, utc) = parse_stamp(&p.value)?;
    let tz = if utc {
        Tz::Utc
    } else if is_date {
        Tz::Floating
    } else {
        p.params.get("TZID").map(|z| Tz::Named(z.clone())).unwrap_or(Tz::Floating)
    };
    Some((ls, is_date, tz))
}

/// Parse a feed and return the events overlapping `[from, to)` (unix
/// seconds), recurring ones expanded. `local_offset_min` is the computer's UTC
/// offset, for all-day events and times without a zone.
pub fn parse(text: &str, feed: usize, from: i64, to: i64, local_offset_min: i32) -> Vec<CalEvent> {
    let mut zones = Zones { defined: HashMap::new(), local_offset: local_offset_min as i64 };
    let mut events: Vec<RawEvent> = Vec::new();
    let mut cur: Option<RawEvent> = None;
    let mut zone: Option<(String, Vec<ZoneRule>)> = None;
    let mut rule: Option<ZoneRule> = None;
    // Inside a VALARM: its trigger, once read. Its other properties
    // (SUMMARY, DESCRIPTION…) are not the event's.
    let mut alarm: Option<Option<Trigger>> = None;
    for line in unfold(text) {
        let Some(p) = parse_prop(&line) else { continue };
        match (p.name.as_str(), p.value.trim()) {
            ("BEGIN", "VEVENT") => cur = Some(RawEvent::default()),
            ("BEGIN", "VALARM") => alarm = Some(None),
            ("END", "VALARM") => {
                if let (Some(Some(t)), Some(e)) = (alarm.take(), cur.as_mut()) {
                    e.alarms.push(t);
                }
                alarm = None;
            }
            (_, _) if alarm.is_some() => {
                if p.name == "TRIGGER" {
                    alarm = Some(parse_trigger(&p));
                }
            }
            ("END", "VEVENT") => events.extend(cur.take()),
            ("BEGIN", "VTIMEZONE") => zone = Some((String::new(), Vec::new())),
            ("END", "VTIMEZONE") => {
                if let Some((id, rules)) = zone.take() {
                    zones.defined.insert(id, rules);
                }
            }
            ("BEGIN", "STANDARD") | ("BEGIN", "DAYLIGHT") if zone.is_some() => rule = Some(ZoneRule::default()),
            ("END", "STANDARD") | ("END", "DAYLIGHT") => {
                if let (Some(r), Some((_, rules))) = (rule.take(), zone.as_mut()) {
                    rules.push(r);
                }
            }
            _ => {
                if let Some(r) = rule.as_mut() {
                    match p.name.as_str() {
                        "TZOFFSETTO" => r.offset_to = parse_offset(&p.value).unwrap_or(0),
                        "DTSTART" => r.start = parse_stamp(&p.value).map(|s| s.0).unwrap_or(0),
                        "RRULE" => {
                            for (k, v) in p.value.split(';').filter_map(|kv| kv.split_once('=')) {
                                match k {
                                    "BYMONTH" => r.month = v.parse().ok(),
                                    "BYMONTHDAY" => r.month_day = v.parse().ok(),
                                    "BYDAY" => {
                                        let (n, wd) = v.split_at(v.len().saturating_sub(2));
                                        r.nth = weekday_code(wd).map(|wd| (n.parse().unwrap_or(1), wd));
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                } else if let Some((id, _)) = zone.as_mut() {
                    if p.name == "TZID" {
                        *id = p.value.trim().to_string();
                    }
                } else if let Some(e) = cur.as_mut() {
                    match p.name.as_str() {
                        "UID" => e.uid = p.value.trim().to_string(),
                        "SUMMARY" => e.title = unescape(&p.value),
                        "LOCATION" => e.location = unescape(&p.value),
                        "DESCRIPTION" => e.description = unescape(&p.value),
                        "X-GOOGLE-CONFERENCE" | "X-MICROSOFT-SKYPETEAMSMEETINGURL" | "URL" => e.conference = p.value.trim().to_string(),
                        "DTSTART" => e.start = stamp_with_tz(&p),
                        "DTEND" => e.end = stamp_with_tz(&p),
                        "DURATION" => e.duration = parse_duration(&p.value),
                        "RRULE" => e.rrule = Some(p.value.trim().to_string()),
                        "STATUS" => e.cancelled = p.value.trim().eq_ignore_ascii_case("CANCELLED"),
                        "RECURRENCE-ID" => e.recurrence_id = stamp_with_tz(&p).map(|(ls, _, tz)| (ls, tz)),
                        "EXDATE" => {
                            let tz = p.params.get("TZID").cloned();
                            for v in p.value.split(',') {
                                if let Some((ls, is_date, utc)) = parse_stamp(v) {
                                    let tz = if utc {
                                        Tz::Utc
                                    } else if is_date {
                                        Tz::Floating
                                    } else {
                                        tz.clone().map(Tz::Named).unwrap_or(Tz::Floating)
                                    };
                                    e.exdates.push((ls, tz));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // Instances replaced or cancelled by RECURRENCE-ID entries.
    let overridden: BTreeSet<(String, i64)> =
        events.iter().filter_map(|e| e.recurrence_id.as_ref().map(|(ls, tz)| (e.uid.clone(), zones.to_unix(*ls, tz)))).collect();

    let mut out = Vec::new();
    for e in &events {
        let Some((start_ls, all_day, tz)) = e.start.clone() else { continue };
        if e.cancelled {
            continue;
        }
        let start_unix = zones.to_unix(start_ls, &tz);
        let length = match (&e.end, e.duration) {
            (Some((ls, _, end_tz)), _) => zones.to_unix(*ls, end_tz) - start_unix,
            (None, Some(d)) => d,
            (None, None) if all_day => 86_400,
            _ => 0,
        }
        .max(0);
        let excluded: BTreeSet<i64> = e.exdates.iter().map(|(ls, tz)| zones.to_unix(*ls, tz)).collect();
        // Reminders as a lead before the start: up to a week ahead, none after it starts.
        let mut alarms: Vec<i64> = e
            .alarms
            .iter()
            .map(|t| match t {
                Trigger::Start(d) => -d,
                Trigger::End(d) => -(length + d),
                Trigger::At(at) => start_unix - at,
            })
            .filter(|lead| (0..=7 * 86_400).contains(lead))
            .collect();
        alarms.sort_unstable();
        alarms.dedup();
        let link = [e.conference.as_str(), e.location.as_str(), e.description.as_str()]
            .iter()
            .find_map(|t| meeting_link(t))
            .or_else(|| e.location.starts_with("https://").then(|| e.location.clone()))
            .unwrap_or_default();
        let mut push = |s: i64| {
            if s < to && s + length.max(1) > from && !excluded.contains(&s) {
                out.push(CalEvent {
                    title: e.title.clone(),
                    location: e.location.clone(),
                    start: s,
                    end: s + length,
                    all_day,
                    feed,
                    alarms: alarms.clone(),
                    link: link.clone(),
                });
            }
        };
        match e.rrule.as_deref().filter(|_| e.recurrence_id.is_none()) {
            None => push(start_unix),
            Some(rule) => {
                for ls in expand(rule, start_ls, &tz, &zones, to) {
                    let s = zones.to_unix(ls, &tz);
                    if !overridden.contains(&(e.uid.clone(), s)) {
                        push(s);
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| (a.start, &a.title).cmp(&(b.start, &b.title)));
    out
}

/// Start times (local seconds in the event's zone) of a recurring event, up to `to`.
fn expand(rule: &str, start: i64, tz: &Tz, zones: &Zones, to: i64) -> Vec<i64> {
    let mut freq = "";
    let (mut interval, mut count, mut until) = (1i64, None::<usize>, None::<i64>);
    let (mut by_day, mut by_month_day, mut by_month) = (Vec::<(i32, u32)>::new(), Vec::<i32>::new(), Vec::<u32>::new());
    for (k, v) in rule.split(';').filter_map(|kv| kv.split_once('=')) {
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => freq = v,
            "INTERVAL" => interval = v.parse().unwrap_or(1).max(1),
            "COUNT" => count = v.parse().ok(),
            "UNTIL" => {
                until = parse_stamp(v).map(|(ls, is_date, utc)| {
                    if utc {
                        ls
                    } else if is_date {
                        zones.to_unix(ls + 86_399, tz)
                    } else {
                        zones.to_unix(ls, tz)
                    }
                })
            }
            "BYDAY" => {
                by_day = v
                    .split(',')
                    .filter_map(|d| {
                        let (n, wd) = d.split_at(d.len().saturating_sub(2));
                        Some((n.parse().unwrap_or(0), weekday_code(wd)?))
                    })
                    .collect()
            }
            "BYMONTHDAY" => by_month_day = v.split(',').filter_map(|d| d.parse().ok()).collect(),
            "BYMONTH" => by_month = v.split(',').filter_map(|d| d.parse().ok()).collect(),
            _ => {}
        }
    }
    let tod = start.rem_euclid(86_400);
    let start_day = start.div_euclid(86_400);
    let (sy, sm, sd) = civil_from_days(start_day);
    let mut out = Vec::new();
    let mut n = 0usize;
    // Candidate days for period `k`, in order.
    let days_for = |k: i64| -> Vec<i64> {
        match freq {
            "DAILY" => {
                let d = start_day + k * interval;
                if by_day.is_empty() || by_day.iter().any(|(_, wd)| *wd == weekday(d)) {
                    vec![d]
                } else {
                    vec![]
                }
            }
            "WEEKLY" => {
                let week = start_day - weekday(start_day) as i64 + k * 7 * interval;
                let mut wds: Vec<u32> = if by_day.is_empty() { vec![weekday(start_day)] } else { by_day.iter().map(|d| d.1).collect() };
                wds.sort();
                wds.dedup();
                wds.into_iter().map(|wd| week + wd as i64).collect()
            }
            "MONTHLY" | "YEARLY" => {
                let months: Vec<(i64, u32)> = if freq == "MONTHLY" {
                    let idx = sy * 12 + (sm as i64 - 1) + k * interval;
                    vec![(idx.div_euclid(12), (idx.rem_euclid(12) + 1) as u32)]
                } else {
                    let y = sy + k * interval;
                    let ms = if by_month.is_empty() { vec![sm] } else { by_month.clone() };
                    ms.into_iter().map(|m| (y, m)).collect()
                };
                let mut days = Vec::new();
                for (y, m) in months {
                    let len = days_in_month(y, m) as i32;
                    if !by_month_day.is_empty() {
                        for d in &by_month_day {
                            let d = if *d < 0 { len + 1 + d } else { *d };
                            if (1..=len).contains(&d) {
                                days.push(days_from_civil(y, m, d as u32));
                            }
                        }
                    } else if !by_day.is_empty() {
                        for (nth, wd) in &by_day {
                            if *nth != 0 {
                                days.extend(nth_weekday(y, m, *nth, *wd));
                            } else {
                                let first = days_from_civil(y, m, 1);
                                days.extend((0..len as i64).map(|i| first + i).filter(|d| weekday(*d) == *wd));
                            }
                        }
                    } else if sd as i32 <= len {
                        days.push(days_from_civil(y, m, sd));
                    }
                }
                days.sort();
                days
            }
            _ => vec![],
        }
    };
    if freq.is_empty() {
        return vec![start];
    }
    for k in 0..5000 {
        for d in days_for(k) {
            let ls = d * 86_400 + tod;
            if ls < start {
                continue;
            }
            let unix = zones.to_unix(ls, tz);
            if until.is_some_and(|u| unix > u) || count.is_some_and(|c| n >= c) || unix >= to {
                return out;
            }
            n += 1;
            out.push(ls);
        }
    }
    out
}

/// "Today", "Tomorrow" or "Mon 27 Oct" for a day, relative to `today`.
pub fn day_label(day: i64, today: i64) -> String {
    match day - today {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        -1 => "Yesterday".into(),
        _ => {
            let (_, m, d) = civil_from_days(day);
            let wd = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][weekday(day) as usize];
            let mon = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][m as usize - 1];
            format!("{wd} {d} {mon}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ICS: &str = include_str!("../testdata/calendar.ics");
    // 2026-10-25 00:00 UTC
    const FROM: i64 = 1_792_886_400;

    fn unix(y: i64, m: u32, d: u32, h: i64, min: i64, offset_min: i64) -> i64 {
        days_from_civil(y, m, d) * 86_400 + h * 3600 + min * 60 - offset_min * 60
    }

    #[test]
    fn dates_round_trip() {
        for days in [-1000, 0, 19_000, 20_751, 30_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(days_from_civil(2026, 10, 25) * 86_400, FROM);
        assert_eq!(weekday(days_from_civil(2026, 10, 25)), 6, "a Sunday");
        assert_eq!(nth_weekday(2026, 3, -1, 6), Some(days_from_civil(2026, 3, 29)));
        assert_eq!(nth_weekday(2026, 11, 1, 6), Some(days_from_civil(2026, 11, 1)));
        assert_eq!(parse_duration("PT1H30M"), Some(5400));
        assert_eq!(parse_duration("P1W"), Some(604_800));
    }

    #[test]
    fn single_all_day_and_zoned_events() {
        let ev = parse(ICS, 2, FROM, FROM + 14 * 86_400, 420);
        let find = |t: &str| ev.iter().find(|e| e.title == t).unwrap_or_else(|| panic!("{t} missing: {ev:#?}"));
        let launch = find("Launch review, part 1");
        assert_eq!(launch.start, unix(2026, 10, 27, 3, 0, 0));
        assert_eq!(launch.end - launch.start, 3600);
        assert_eq!(launch.location, "Room 4, Jakarta");
        assert_eq!(launch.feed, 2);
        let holiday = find("Libur");
        assert!(holiday.all_day);
        assert_eq!(holiday.start, unix(2026, 10, 28, 0, 0, 420), "local midnight");
        assert_eq!(holiday.end - holiday.start, 86_400);
        // Defined VTIMEZONE with daylight saving: New York is UTC-4 until 1 Nov 2026, then UTC-5.
        let ny = ev.iter().filter(|e| e.title == "NY sync").map(|e| e.start).collect::<Vec<_>>();
        assert_eq!(ny, vec![unix(2026, 10, 26, 9, 0, -240), unix(2026, 11, 2, 9, 0, -300)]);
        assert!(!ev.iter().any(|e| e.title == "Cancelled"));
        assert!(ev.windows(2).all(|w| w[0].start <= w[1].start));
    }

    #[test]
    fn recurring_events_expand_with_exceptions() {
        let ev = parse(ICS, 0, FROM, FROM + 14 * 86_400, 420);
        let standups: Vec<i64> = ev.iter().filter(|e| e.title == "Standup").map(|e| e.start).collect();
        // Weekdays at 09:30 Jakarta from Mon 26 Oct, minus 28 Oct (EXDATE) and 29 Oct (moved).
        let day = |d: u32| unix(2026, 10, d, 9, 30, 420);
        assert_eq!(&standups[..4], &[day(26), day(27), day(30), unix(2026, 11, 2, 9, 30, 420)]);
        assert!(ev.iter().any(|e| e.title == "Standup (moved)" && e.start == unix(2026, 10, 29, 11, 0, 420)));
        // Monthly on the last Friday, three times from September.
        let last_friday: Vec<i64> = ev.iter().filter(|e| e.title == "Payday").map(|e| e.start).collect();
        assert_eq!(last_friday, vec![unix(2026, 10, 30, 0, 0, 420)]);
        let all = parse(ICS, 0, 0, i64::MAX / 4, 420);
        assert_eq!(all.iter().filter(|e| e.title == "Payday").count(), 3, "COUNT=3");
    }

    #[test]
    fn event_reminders_come_from_valarm() {
        let ev = parse(ICS, 0, FROM, FROM + 14 * 86_400, 420);
        let dentist = ev.iter().find(|e| e.start == unix(2026, 10, 26, 10, 0, 420)).expect("dentist");
        // An alarm's SUMMARY is not the event's title.
        assert_eq!(dentist.title, "Dentist");
        // 30 min and a day before; one after the end is dropped.
        assert_eq!(dentist.alarms, vec![1800, 86_400]);
        assert!(ev.iter().filter(|e| e.title != "Dentist").all(|e| e.alarms.is_empty()));
    }

    #[test]
    fn times_do_not_depend_on_the_computers_zone() {
        // The same feed read on computers in Jakarta, London and Tokyo.
        for offset in [420, 0, 540, -300] {
            let ev = parse(ICS, 0, FROM, FROM + 14 * 86_400, offset);
            let at = |t: &str| ev.iter().find(|e| e.title == t).map(|e| e.start).unwrap_or_else(|| panic!("{t} missing"));
            // UTC and zoned times are the same instant everywhere ...
            assert_eq!(at("Launch review, part 1"), unix(2026, 10, 27, 3, 0, 0), "offset {offset}");
            assert_eq!(at("Dentist"), unix(2026, 10, 26, 10, 0, 420), "offset {offset}");
            assert_eq!(at("Standup"), unix(2026, 10, 26, 9, 30, 420), "offset {offset}");
            // ... while times without a zone and all-day events follow the computer's clock.
            assert_eq!(at("Lunch (no zone)"), unix(2026, 10, 29, 12, 0, offset as i64), "offset {offset}");
            assert_eq!(at("Libur"), unix(2026, 10, 28, 0, 0, offset as i64), "offset {offset}");
        }
    }

    #[test]
    fn meeting_links() {
        assert_eq!(
            meeting_link("Join: https://meet.google.com/abc-defg-hij\nor call"),
            Some("https://meet.google.com/abc-defg-hij".into())
        );
        assert_eq!(meeting_link("<https://us02web.zoom.us/j/123?pwd=x>"), Some("https://us02web.zoom.us/j/123?pwd=x".into()));
        assert_eq!(meeting_link("see https://example.com"), None);
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20261026T030000Z\nDTEND:20261026T040000Z\nSUMMARY:Sync\nDESCRIPTION:Agenda\\nhttps://example.com\nX-GOOGLE-CONFERENCE:https://meet.google.com/xyz-abcd-efg\nEND:VEVENT\nEND:VCALENDAR\n";
        let ev = parse(ics, 0, FROM, FROM + 7 * 86_400, 420);
        assert_eq!(ev[0].link, "https://meet.google.com/xyz-abcd-efg");
    }

    #[test]
    fn webcal_and_labels() {
        assert_eq!(feed_url(" webcal://calendar.google.com/x.ics "), "https://calendar.google.com/x.ics");
        let today = days_from_civil(2026, 10, 25);
        assert_eq!(day_label(today, today), "Today");
        assert_eq!(day_label(today + 1, today), "Tomorrow");
        assert_eq!(day_label(today + 2, today), "Tue 27 Oct");
    }
}
