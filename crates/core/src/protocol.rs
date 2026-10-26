//! Named-pipe protocol v1 between the `ngambang` CLI and the running island
//! (SPEC §6.2). One JSON object per line.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: u64 = 1;
pub const MAX_LINE: usize = 4096;
pub const MAX_TITLE: usize = 120;
pub const MAX_BODY: usize = 400;
/// Messages accepted per client per second.
pub const RATE_PER_SEC: u32 = 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    #[default]
    Info,
    Ok,
    Warn,
    Fail,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        match s {
            "info" => Some(Level::Info),
            "ok" | "success" => Some(Level::Ok),
            "warn" | "warning" => Some(Level::Warn),
            "fail" | "error" => Some(Level::Fail),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Request {
    Notify {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
        #[serde(default)]
        level: Level,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        progress: Option<f32>,
        #[serde(default)]
        sticky: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ttl_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hwnd: Option<String>,
    },
    Done {
        id: String,
        /// Shown when the job was never announced with `notify`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_s: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hwnd: Option<String>,
    },
    Clear {
        id: String,
    },
    /// Keep a quick note (`ngambang note "…"`).
    Note {
        text: String,
    },
    Ping,
}

impl Request {
    /// Serialise as one protocol line (without the newline).
    pub fn to_line(&self) -> String {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Value::Object(map) = &mut v {
            map.insert("v".into(), Value::from(VERSION));
        }
        v.to_string()
    }

    /// The job id this request refers to.
    pub fn job_id(&self) -> Option<String> {
        match self {
            Request::Notify { id, title, .. } => Some(id.clone().unwrap_or_else(|| crate::format::slug(title))),
            Request::Done { id, .. } | Request::Clear { id } => Some(crate::format::slug(id)),
            Request::Ping | Request::Note { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok() -> Self {
        Response { ok: true, error: None }
    }
    pub fn error(msg: impl Into<String>) -> Self {
        Response { ok: false, error: Some(msg.into()) }
    }
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{\"ok\":false}".into())
    }
}

/// Parse and sanitise one line from a client.
pub fn parse_line(line: &str) -> Result<Request, String> {
    if line.len() > MAX_LINE {
        return Err(format!("line longer than {MAX_LINE} bytes"));
    }
    let v: Value = serde_json::from_str(line.trim()).map_err(|e| format!("invalid JSON: {e}"))?;
    match v.get("v").and_then(Value::as_u64) {
        Some(VERSION) => {}
        Some(other) => return Err(format!("unsupported protocol version {other}")),
        None => return Err("missing \"v\"".into()),
    }
    let mut req: Request = serde_json::from_value(v).map_err(|e| format!("bad request: {e}"))?;
    sanitise(&mut req)?;
    Ok(req)
}

fn clean_text(s: &str, max: usize) -> String {
    let flat: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    crate::format::ellipsize(flat.trim(), max)
}

fn sanitise(req: &mut Request) -> Result<(), String> {
    match req {
        Request::Notify { id, title, body, progress, ttl_ms, .. } => {
            *title = clean_text(title, MAX_TITLE);
            if title.is_empty() {
                return Err("title is empty".into());
            }
            if let Some(b) = body {
                *b = clean_text(b, MAX_BODY);
            }
            if let Some(i) = id {
                *i = crate::format::slug(i);
            }
            if let Some(p) = progress {
                if !p.is_finite() {
                    return Err("progress must be a number".into());
                }
                *p = p.clamp(0.0, 1.0);
            }
            if let Some(t) = ttl_ms {
                *t = (*t).clamp(1000, 60_000);
            }
        }
        Request::Done { id, title, .. } => {
            if id.trim().is_empty() {
                return Err("id is empty".into());
            }
            if let Some(t) = title {
                *t = clean_text(t, MAX_TITLE);
            }
            *id = crate::format::slug(id);
        }
        Request::Clear { id } => {
            if id.trim().is_empty() {
                return Err("id is empty".into());
            }
            *id = crate::format::slug(id);
        }
        Request::Note { text } => {
            *text = clean_text(text, MAX_BODY);
            if text.is_empty() {
                return Err("note is empty".into());
            }
        }
        Request::Ping => {}
    }
    Ok(())
}

/// Parse a window handle sent as "0x1a2b" or decimal.
pub fn parse_hwnd(s: &str) -> Option<u64> {
    let s = s.trim();
    let v = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok()?,
        None => s.parse().ok()?,
    };
    (v != 0).then_some(v)
}

/// Fixed-window rate limiter, one per client connection.
#[derive(Debug, Default)]
pub struct RateLimiter {
    window_start_ms: u64,
    count: u32,
}

impl RateLimiter {
    pub fn allow(&mut self, now_ms: u64) -> bool {
        if now_ms.saturating_sub(self.window_start_ms) >= 1000 {
            self.window_start_ms = now_ms;
            self.count = 0;
        }
        self.count += 1;
        self.count <= RATE_PER_SEC
    }
}

/// Pipe name for a user SID string.
pub fn pipe_name(user_sid: &str) -> String {
    format!(r"\\.\pipe\ngambang-{user_sid}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let r = Request::Notify {
            id: None,
            title: "deploy staging".into(),
            body: None,
            level: Level::Info,
            progress: Some(0.4),
            sticky: true,
            ttl_ms: None,
            hwnd: None,
        };
        let line = r.to_line();
        assert!(line.contains("\"v\":1"));
        assert!(line.contains("\"op\":\"notify\""));
        assert_eq!(parse_line(&line).unwrap(), r);
        assert_eq!(r.job_id().unwrap(), "deploy-staging");
    }

    #[test]
    fn spec_examples_parse() {
        for l in [
            r#"{"v":1,"op":"notify","id":"deploy","title":"deploy staging","level":"info","progress":0.4,"sticky":true}"#,
            r#"{"v":1,"op":"done","id":"deploy","ok":true,"code":0,"duration_s":134,"hwnd":"0x1a2b3c"}"#,
            r#"{"v":1,"op":"clear","id":"deploy"}"#,
            r#"{"v":1,"op":"ping"}"#,
        ] {
            parse_line(l).unwrap_or_else(|e| panic!("{l}: {e}"));
        }
    }

    #[test]
    fn rejects_and_sanitises() {
        assert!(parse_line(r#"{"op":"ping"}"#).is_err());
        assert!(parse_line(r#"{"v":2,"op":"ping"}"#).is_err());
        assert!(parse_line(r#"{"v":1,"op":"explode"}"#).is_err());
        assert!(parse_line(r#"{"v":1,"op":"notify","title":"   "}"#).is_err());
        assert!(parse_line(&"x".repeat(MAX_LINE + 1)).is_err());
        let r = parse_line(r#"{"v":1,"op":"notify","title":"a\nb","progress":7,"ttl_ms":5,"extra":true}"#).unwrap();
        let Request::Notify { title, progress, ttl_ms, .. } = r else { panic!() };
        assert_eq!(title, "a b");
        assert_eq!(progress, Some(1.0));
        assert_eq!(ttl_ms, Some(1000));
        let long = format!(r#"{{"v":1,"op":"notify","title":"{}"}}"#, "y".repeat(500));
        let Request::Notify { title, .. } = parse_line(&long).unwrap() else { panic!() };
        assert_eq!(title.chars().count(), MAX_TITLE);
    }

    #[test]
    fn hwnd() {
        assert_eq!(parse_hwnd("0x1a2b3c"), Some(0x1a2b3c));
        assert_eq!(parse_hwnd("42"), Some(42));
        assert_eq!(parse_hwnd("0"), None);
        assert_eq!(parse_hwnd("zz"), None);
    }

    #[test]
    fn rate_limit() {
        let mut r = RateLimiter::default();
        let allowed = (0..100).filter(|_| r.allow(5_000)).count();
        assert_eq!(allowed, RATE_PER_SEC as usize);
        assert!(r.allow(6_000));
    }
}
