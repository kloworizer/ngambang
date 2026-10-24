//! A tiny size-capped log file. Warnings and errors only, never user content.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

static LOG: Mutex<Option<File>> = Mutex::new(None);
const MAX_BYTES: u64 = 256 * 1024;

pub fn init() {
    let path = crate::paths::log_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, path.with_extension("log.old"));
    }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
        *LOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    }
}

pub fn write(level: &str, msg: &str) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let line = format!("{} {level} {msg}\n", ngambang_core::format::iso_utc(secs));
    if let Some(f) = LOG.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = f.write_all(line.as_bytes());
    }
    if cfg!(debug_assertions) {
        eprint!("{line}");
    }
}

macro_rules! log_warn {
    ($($t:tt)*) => { $crate::win::log::write("WARN", &format!($($t)*)) };
}
macro_rules! log_info {
    ($($t:tt)*) => { $crate::win::log::write("INFO", &format!($($t)*)) };
}
#[allow(unused_imports)]
pub(crate) use {log_info, log_warn};
