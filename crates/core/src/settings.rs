//! The Settings window's form: built from the config, validated, and turned
//! into in-place `config.toml` edits. Pure so it is tested on every OS.

use crate::config::{Config, ModuleId, Modules};
use crate::geometry::Edge;
use crate::hotkey::Hotkey;

#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub autostart: bool,
    pub edge: Edge,
    /// Position along the edge, 0–100 (%).
    pub offset_pct: String,
    pub monitor: String,
    pub hotkey: String,
    pub hide_in_fullscreen: bool,
    pub reduce_motion: bool,
    pub hide_from_capture: bool,
    pub modules: Modules,
    pub track_tags: bool,
    pub volume_step: String,
    pub long_command_s: String,
    pub clipboard_tags: bool,
    pub timer_presets: String,
    pub break_after_min: String,
    pub ping_host: String,
    /// Power plan GUID, or empty for "don't switch".
    pub power_plan: String,
    pub prayer_city: String,
    pub prayer_lat: String,
    pub prayer_lon: String,
    pub prayer_method: crate::prayer::Method,
    pub prayer_asr: crate::prayer::Asr,
    pub prayer_reminder: String,
    pub update_check: bool,
    pub weather_alerts: bool,
    pub quake_radius_km: String,
}

/// One `key = value` change inside a table; `value` is a TOML literal.
pub type Edit = (&'static str, &'static str, String);

fn toml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn number(field: &str, text: &str, min: u64, max: u64) -> Result<u64, String> {
    let v: u64 = text.trim().parse().map_err(|_| format!("{field}: enter a whole number"))?;
    if !(min..=max).contains(&v) {
        return Err(format!("{field}: use a number from {min} to {max}"));
    }
    Ok(v)
}

impl Form {
    pub fn from_config(c: &Config, autostart: bool) -> Form {
        Form {
            autostart,
            edge: c.dock.edge,
            offset_pct: format!("{}", (c.dock.offset * 100.0).round() as i32),
            monitor: c.dock.monitor.clone(),
            hotkey: c.dock.hotkey.clone(),
            hide_in_fullscreen: c.dock.hide_in_fullscreen,
            reduce_motion: c.motion.reduce,
            hide_from_capture: c.privacy.hide_from_capture,
            modules: c.modules.clone(),
            track_tags: c.media.track_tags,
            volume_step: c.media.volume_step.to_string(),
            long_command_s: c.commands.long_command_s.to_string(),
            clipboard_tags: c.clipboard.tags,
            timer_presets: c.timer.presets_min.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(", "),
            break_after_min: c.game.break_after_min.to_string(),
            ping_host: c.game.ping_host.clone(),
            power_plan: c.game.power_plan.clone(),
            prayer_city: c.location.city.clone(),
            prayer_lat: format!("{:.4}", c.location.latitude),
            prayer_lon: format!("{:.4}", c.location.longitude),
            prayer_method: c.prayer.method,
            prayer_asr: c.prayer.asr,
            prayer_reminder: c.prayer.reminder_min.to_string(),
            update_check: c.update.auto_check,
            weather_alerts: c.weather.alerts,
            quake_radius_km: c.weather.quake_radius_km.to_string(),
        }
    }

    /// Validate and produce the config edits. The first problem is returned as
    /// a sentence the user can act on.
    pub fn edits(&self) -> Result<Vec<Edit>, String> {
        let offset = number("Position along the edge", &self.offset_pct, 0, 100)?;
        if Hotkey::parse(&self.hotkey).is_none() {
            return Err("Hotkey: use modifiers plus one key, for example Win+Alt+Space".into());
        }
        let step = number("Volume step", &self.volume_step, 1, 20)?;
        let long = number("Long command", &self.long_command_s, 1, 86_400)?;
        let brk = number("Break reminder", &self.break_after_min, 0, 1440)?;
        if brk != 0 && brk < 10 {
            return Err("Break reminder: use 0 (off) or at least 10 minutes".into());
        }
        let mut presets = Vec::new();
        for p in self.timer_presets.split([',', ' ']).filter(|p| !p.trim().is_empty()) {
            presets.push(number("Timer presets", p, 1, 240)?);
        }
        if presets.is_empty() || presets.len() > 3 {
            return Err("Timer presets: enter one to three lengths in minutes, like 25, 50".into());
        }
        let host = self.ping_host.trim();
        if host.is_empty() || host.contains(char::is_whitespace) {
            return Err("Ping host: enter \"gateway\", an IP address or a host name".into());
        }
        if !self.power_plan.is_empty() && crate::format::parse_guid(&self.power_plan).is_none() {
            return Err("Power plan: pick one from the list".into());
        }
        let coord = |field: &str, text: &str, limit: f64| -> Result<f64, String> {
            let v: f64 = text.trim().replace(',', ".").parse().map_err(|_| format!("{field}: enter a number like -6.2088"))?;
            if !(-limit..=limit).contains(&v) {
                return Err(format!("{field}: use a value from -{limit} to {limit}"));
            }
            Ok(v)
        };
        let lat = coord("Latitude", &self.prayer_lat, 90.0)?;
        let lon = coord("Longitude", &self.prayer_lon, 180.0)?;
        let reminder = number("Prayer reminder", &self.prayer_reminder, 0, 60)?;
        let city = self.prayer_city.trim();
        let radius = number("Earthquake radius", &self.quake_radius_km, 10, 5000)?;
        let b = |v: bool| v.to_string();
        let mut out: Vec<Edit> = vec![
            ("dock", "edge", toml_str(self.edge.name())),
            ("dock", "offset", format!("{}", offset as f32 / 100.0)),
            ("dock", "monitor", toml_str(if self.monitor.trim().is_empty() { "primary" } else { self.monitor.trim() })),
            ("dock", "hotkey", toml_str(self.hotkey.trim())),
            ("dock", "hide_in_fullscreen", b(self.hide_in_fullscreen)),
            ("motion", "reduce", b(self.reduce_motion)),
            ("privacy", "hide_from_capture", b(self.hide_from_capture)),
            ("media", "track_tags", b(self.track_tags)),
            ("media", "volume_step", step.to_string()),
            ("commands", "long_command_s", long.to_string()),
            ("clipboard", "tags", b(self.clipboard_tags)),
            ("timer", "presets_min", format!("[{}]", presets.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", "))),
            ("game", "break_after_min", brk.to_string()),
            ("game", "ping_host", toml_str(host)),
            ("game", "power_plan", toml_str(&self.power_plan)),
            ("location", "city", toml_str(if city.is_empty() { "My location" } else { city })),
            ("location", "latitude", format!("{lat:.4}")),
            ("location", "longitude", format!("{lon:.4}")),
            ("prayer", "method", toml_str(self.prayer_method.key())),
            ("prayer", "asr", toml_str(if self.prayer_asr == crate::prayer::Asr::Hanafi { "hanafi" } else { "standard" })),
            ("prayer", "reminder_min", reminder.to_string()),
            ("update", "auto_check", b(self.update_check)),
            ("weather", "alerts", b(self.weather_alerts)),
            ("weather", "quake_radius_km", radius.to_string()),
        ];
        for m in ModuleId::ALL {
            out.push(("modules", m.key(), b(self.modules.get(m))));
        }
        Ok(out)
    }
}

/// Apply edits to a config file's text.
pub fn apply(text: &str, edits: &[Edit]) -> String {
    edits.iter().fold(text.to_string(), |t, (table, key, value)| crate::config::set_toml_value(&t, table, key, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DEFAULT_TOML;

    #[test]
    fn unchanged_form_round_trips() {
        let cfg = Config::default();
        let form = Form::from_config(&cfg, false);
        let text = apply(DEFAULT_TOML, &form.edits().unwrap());
        let (back, warnings) = Config::parse(&text);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(back, cfg);
        assert!(text.contains("# Ngambang configuration"), "comments kept");
    }

    #[test]
    fn changes_apply() {
        let mut form = Form::from_config(&Config::default(), true);
        form.edge = Edge::Top;
        form.offset_pct = "20".into();
        form.monitor = r"\\.\DISPLAY2".into();
        form.modules.set(ModuleId::Ports, false);
        form.timer_presets = "15 45 90".into();
        form.ping_host = "1.1.1.1".into();
        form.power_plan = "{8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c}".into();
        form.modules.set(ModuleId::Prayer, true);
        form.prayer_city = "Makassar".into();
        form.prayer_lat = "-5,1477".into();
        form.prayer_lon = "119.4327".into();
        form.prayer_method = crate::prayer::Method::Mwl;
        form.prayer_asr = crate::prayer::Asr::Hanafi;
        form.update_check = false;
        form.weather_alerts = false;
        form.quake_radius_km = "500".into();
        let (c, w) = Config::parse(&apply(DEFAULT_TOML, &form.edits().unwrap()));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.dock.edge, Edge::Top);
        assert_eq!(c.dock.offset, 0.2);
        assert_eq!(c.dock.monitor, r"\\.\DISPLAY2");
        assert!(!c.modules.ports);
        assert_eq!(c.timer.presets_min, vec![15, 45, 90]);
        assert_eq!(c.game.ping_host, "1.1.1.1");
        assert!(!c.game.power_plan.is_empty());
        assert!(c.modules.prayer);
        assert_eq!(c.location.city, "Makassar");
        assert!((c.location.latitude + 5.1477).abs() < 1e-9);
        assert_eq!(c.prayer.method, crate::prayer::Method::Mwl);
        assert_eq!(c.prayer.asr, crate::prayer::Asr::Hanafi);
        assert!(!c.update.auto_check);
        assert!(!c.weather.alerts);
        assert_eq!(c.weather.quake_radius_km, 500);
    }

    #[test]
    fn validation_messages() {
        let base = Form::from_config(&Config::default(), false);
        let check = |f: Form, needle: &str| {
            let e = f.edits().unwrap_err();
            assert!(e.contains(needle), "{e}");
        };
        check(Form { offset_pct: "150".into(), ..base.clone() }, "Position");
        check(Form { hotkey: "Space".into(), ..base.clone() }, "Hotkey");
        check(Form { volume_step: "x".into(), ..base.clone() }, "whole number");
        check(Form { break_after_min: "5".into(), ..base.clone() }, "Break");
        check(Form { timer_presets: "".into(), ..base.clone() }, "Timer");
        check(Form { timer_presets: "1,2,3,4".into(), ..base.clone() }, "Timer");
        check(Form { ping_host: "a b".into(), ..base.clone() }, "Ping");
        check(Form { power_plan: "nope".into(), ..base.clone() }, "Power plan");
        check(Form { prayer_lat: "95".into(), ..base.clone() }, "Latitude");
        check(Form { prayer_lon: "east".into(), ..base.clone() }, "Longitude");
        check(Form { prayer_reminder: "90".into(), ..base }, "Prayer reminder");
    }
}
