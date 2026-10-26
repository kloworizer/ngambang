//! Network: HTTPS GET over WinHTTP, Windows' own "connected to the internet"
//! status, and the worker that refreshes weather, early warnings and update
//! checks. Without a connection nothing is requested and nothing errors: the
//! island keeps its cached data and catches up when the connection returns.

use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ngambang_core::model::Event;
use ngambang_core::update::{self, Release};
use ngambang_core::weather;
use ngambang_core::{calendar, finance};
use windows::core::{w, HSTRING};
use windows::Win32::Networking::NetworkListManager::{INetworkListManager, NetworkListManager};
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

use super::util::{local_offset_min, unix_now, Bus};

/// The release found by the last update check, for the installer.
pub static PENDING_RELEASE: Mutex<Option<Release>> = Mutex::new(None);

/// Retries after a failed request: quick at first, then every ten minutes.
const RETRY: [Duration; 5] = [
    Duration::from_secs(20),
    Duration::from_secs(60),
    Duration::from_secs(2 * 60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(10 * 60),
];
const OFFLINE_POLL: Duration = Duration::from_secs(60);
/// Large calendars can be several megabytes.
const MAX_BODY: usize = 16 * 1024 * 1024;

struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// GET an https:// URL and return the body as text (status 200 only).
pub fn http_get(url: &str) -> Result<String, String> {
    let rest = url.strip_prefix("https://").ok_or("only https URLs are supported")?;
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let agent = HSTRING::from(format!("Ngambang/{} (+https://github.com/kloworizer/ngambang)", ngambang_core::VERSION));
    unsafe {
        let session = Handle(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, None, None, 0));
        if session.0.is_null() {
            return Err("WinHTTP unavailable".into());
        }
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 10_000, 15_000);
        let connect = Handle(WinHttpConnect(session.0, &HSTRING::from(host), INTERNET_DEFAULT_HTTPS_PORT, 0));
        if connect.0.is_null() {
            return Err(format!("cannot reach {host}"));
        }
        let request =
            Handle(WinHttpOpenRequest(connect.0, w!("GET"), &HSTRING::from(path), None, None, std::ptr::null(), WINHTTP_FLAG_SECURE));
        if request.0.is_null() {
            return Err("cannot open request".into());
        }
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|e| format!("send: {e}"))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|e| format!("receive: {e}"))?;
        let mut status = 0u32;
        let mut size = 4u32;
        let mut index = 0u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            None,
            Some(&mut status as *mut u32 as *mut _),
            &mut size,
            &mut index,
        )
        .map_err(|e| format!("status: {e}"))?;
        if status != 200 {
            return Err(format!("HTTP {status}"));
        }
        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut available).map_err(|e| format!("read: {e}"))?;
            if available == 0 {
                break;
            }
            let start = body.len();
            body.resize(start + available as usize, 0);
            let mut read = 0u32;
            WinHttpReadData(request.0, body[start..].as_mut_ptr().cast(), available, &mut read).map_err(|e| format!("read: {e}"))?;
            body.truncate(start + read as usize);
            if body.len() > MAX_BODY {
                return Err("response too large".into());
            }
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }
}

/// What Windows thinks about internet connectivity. None if it cannot tell.
pub fn online() -> Option<bool> {
    unsafe {
        let nlm: INetworkListManager = CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).ok()?;
        nlm.IsConnectedToInternet().ok().map(|b| b.as_bool())
    }
}

/// Ask GitHub for the latest release; Some if it is newer than this build.
pub fn check_update() -> Result<Option<Release>, String> {
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    let json = http_get(update::LATEST_URL)?;
    let release = update::parse_release(&json, arch).ok_or("no release for this computer yet")?;
    Ok(update::is_newer(&release.version, ngambang_core::VERSION).then_some(release))
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetConfig {
    pub weather: bool,
    pub city: String,
    pub lat: f64,
    pub lon: f64,
    pub auto_update: bool,
    /// iCal addresses of linked calendars (empty: the module is off).
    pub calendars: Vec<String>,
    pub calendar_days: u32,
    pub symbols: Vec<String>,
    /// Watchlist names, for headlines when a symbol has none of its own.
    pub names: Vec<String>,
    /// Keep an eye on prices in the background (move alerts).
    pub price_alerts: bool,
    pub news: bool,
}

/// Search Yahoo Finance for symbols (Settings ▸ Markets).
pub fn search_symbols(query: &str) -> Result<Vec<finance::SearchHit>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(finance::parse_search(&http_get(&finance::search_url(query))?))
}

/// Download every linked calendar; None when none could be read.
fn fetch_calendars(urls: &[String], days: u32) -> Option<Vec<calendar::CalEvent>> {
    let now = unix_now();
    let (from, to) = (now - 86_400, now + (days as i64 + 1) * 86_400);
    let offset = local_offset_min();
    let mut events = Vec::new();
    let mut any = false;
    for (i, url) in urls.iter().enumerate() {
        match http_get(&calendar::feed_url(url)) {
            Ok(text) => {
                any = true;
                events.extend(calendar::parse(&text, i, from, to, offset));
            }
            Err(e) => log_warn!("calendar {}: {e}", i + 1),
        }
    }
    events.sort_by_key(|e| e.start);
    any.then_some(events)
}

fn fetch_quotes(symbols: &[String]) -> Vec<finance::Quote> {
    symbols.iter().filter_map(|s| http_get(&finance::chart_url(s)).ok().and_then(|j| finance::parse_chart(&j))).collect()
}

/// Headlines for each symbol from Google News (by name, freshest) and
/// Yahoo Finance (by symbol); None when nothing could be read.
fn fetch_news(symbols: &[String], names: &[String]) -> Option<Vec<finance::NewsItem>> {
    let mut out: Vec<finance::NewsItem> = Vec::new();
    let mut any = false;
    for (i, sym) in symbols.iter().take(10).enumerate() {
        let name = names.get(i).map(|n| n.trim()).filter(|n| !n.is_empty()).unwrap_or(sym);
        if let Ok(xml) = http_get(&finance::google_news_url(name)) {
            any = true;
            let mut items = finance::parse_rss(&xml, sym);
            // The newest few per symbol are plenty.
            items.sort_by_key(|n| std::cmp::Reverse(n.time));
            items.truncate(4);
            out.extend(items);
        }
        if let Ok(json) = http_get(&finance::news_url(sym)) {
            any = true;
            out.extend(finance::parse_news(&json, sym));
        }
    }
    // The same story from both sources once.
    let key = |t: &str| t.to_lowercase().chars().filter(|c| c.is_alphanumeric()).take(48).collect::<String>();
    let mut seen = std::collections::HashSet::new();
    out.sort_by_key(|n| std::cmp::Reverse(n.time));
    out.retain(|n| seen.insert(key(&n.title)));
    any.then_some(out)
}

enum Msg {
    Config(NetConfig),
    /// Quotes on (the watchlist is open) or off.
    Finance(bool),
    Refresh,
    CheckUpdate,
    Stop,
}

pub struct NetWorker {
    tx: Sender<Msg>,
}

impl NetWorker {
    pub fn start(bus: Bus, cfg: NetConfig) -> NetWorker {
        let (tx, rx) = channel();
        let _ = std::thread::Builder::new().name("ngambang-net".into()).spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let mut cfg = cfg;
            let now = Instant::now();
            // First weather right away; the update check waits a bit after start.
            let (mut next_weather, mut next_alerts, mut next_update) = (now, now, now + Duration::from_secs(30));
            let mut last_online: Option<bool> = None;
            let mut gdacs: Vec<weather::Alert> = Vec::new();
            let mut usgs: Vec<weather::Alert> = Vec::new();
            let mut manual_update = false;
            let (mut weather_fails, mut alert_fails) = (0usize, 0usize);
            let (mut next_calendar, mut calendar_fails) = (now, 0usize);
            let (mut quotes_on, mut next_quotes) = (false, now);
            let mut next_news = now + Duration::from_secs(20);
            let mut last_news: Option<Instant> = None;
            let retry = |fails: usize| RETRY[fails.min(RETRY.len() - 1)];
            loop {
                let now = Instant::now();
                let mut due = next_update;
                if cfg.weather {
                    due = due.min(next_weather).min(next_alerts);
                }
                if !cfg.calendars.is_empty() {
                    due = due.min(next_calendar);
                }
                if (quotes_on || cfg.price_alerts) && !cfg.symbols.is_empty() {
                    due = due.min(next_quotes);
                }
                if cfg.news && !cfg.symbols.is_empty() {
                    due = due.min(next_news);
                }
                // Offline, due work waits for the next connectivity check
                // instead of spinning on a deadline that has already passed.
                let wait = if last_online == Some(false) {
                    OFFLINE_POLL
                } else {
                    due.saturating_duration_since(now).min(Duration::from_secs(3600))
                };
                match rx.recv_timeout(wait) {
                    Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                    Ok(Msg::Config(c)) => {
                        let moved = c.lat != cfg.lat || c.lon != cfg.lon || (c.weather && !cfg.weather);
                        if c.calendars != cfg.calendars || c.calendar_days != cfg.calendar_days {
                            next_calendar = Instant::now();
                        }
                        if c.symbols != cfg.symbols {
                            next_quotes = Instant::now();
                            next_news = Instant::now();
                        }
                        cfg = c;
                        if moved {
                            next_weather = Instant::now();
                        }
                    }
                    Ok(Msg::Finance(on)) => {
                        // Fresh prices as soon as the watchlist opens, unless they are seconds old,
                        // and headlines unless they are a couple of minutes old.
                        if on && !quotes_on {
                            next_quotes = next_quotes.min(Instant::now() + Duration::from_millis(300));
                            let stale = last_news.is_none_or(|t| t.elapsed() >= Duration::from_millis(finance::NEWS_OPEN_MS));
                            if stale {
                                next_news = next_news.min(Instant::now() + Duration::from_millis(600));
                            }
                        }
                        quotes_on = on;
                    }
                    Ok(Msg::Refresh) => {
                        next_weather = Instant::now();
                        next_alerts = Instant::now();
                        next_calendar = Instant::now();
                        // A manual refresh also re-checks a connection Windows called offline.
                        last_online = None;
                    }
                    Ok(Msg::CheckUpdate) => {
                        manual_update = true;
                        next_update = Instant::now();
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                }

                // Windows' answer is a hint: with a VPN or a captive proxy it can say
                // "offline" while requests work, so try anyway once in a while.
                let mut is_online = online().unwrap_or(true);
                if !is_online && cfg.weather && Instant::now() >= next_weather {
                    is_online = http_get(&weather::open_meteo_url(cfg.lat, cfg.lon))
                        .ok()
                        .and_then(|j| weather::parse_open_meteo(&j, &cfg.city, unix_now()))
                        .map(|w| bus.send(Event::Weather(w)))
                        .is_some();
                    if is_online {
                        next_weather = Instant::now() + Duration::from_millis(weather::WEATHER_EVERY_MS);
                    }
                }
                if last_online != Some(is_online) {
                    last_online = Some(is_online);
                    bus.send(Event::Online(is_online));
                }
                let now = Instant::now();
                if !is_online {
                    if manual_update {
                        manual_update = false;
                        bus.send(Event::Notice("No internet connection — try again when you're online".into()));
                    }
                    // Everything due waits until the connection is back.
                    continue;
                }

                if cfg.weather && now >= next_weather {
                    match http_get(&weather::open_meteo_url(cfg.lat, cfg.lon))
                        .ok()
                        .and_then(|j| weather::parse_open_meteo(&j, &cfg.city, unix_now()))
                    {
                        Some(w) => {
                            bus.send(Event::Weather(w));
                            weather_fails = 0;
                            next_weather = now + Duration::from_millis(weather::WEATHER_EVERY_MS);
                        }
                        None => {
                            next_weather = now + retry(weather_fails);
                            weather_fails += 1;
                        }
                    }
                }
                if cfg.weather && now >= next_alerts {
                    let mut ok = false;
                    if let Some(list) = http_get(weather::GDACS_ALERTS).ok().and_then(|j| weather::parse_gdacs(&j, unix_now())) {
                        gdacs = list;
                        ok = true;
                    }
                    if let Some(list) = http_get(weather::USGS_QUAKES).ok().and_then(|j| weather::parse_usgs(&j)) {
                        usgs = list;
                        ok = true;
                    }
                    if ok {
                        bus.send(Event::Alerts(gdacs.iter().chain(usgs.iter()).cloned().collect()));
                    }
                    next_alerts = if ok {
                        alert_fails = 0;
                        now + Duration::from_millis(weather::ALERTS_EVERY_MS)
                    } else {
                        alert_fails += 1;
                        now + retry(alert_fails - 1)
                    };
                }
                if !cfg.calendars.is_empty() && now >= next_calendar {
                    match fetch_calendars(&cfg.calendars, cfg.calendar_days) {
                        Some(events) => {
                            bus.send(Event::Calendar(events));
                            calendar_fails = 0;
                            next_calendar = now + Duration::from_millis(calendar::REFRESH_MS);
                        }
                        None => {
                            next_calendar = now + retry(calendar_fails);
                            calendar_fails += 1;
                        }
                    }
                }
                if (quotes_on || cfg.price_alerts) && !cfg.symbols.is_empty() && now >= next_quotes {
                    let quotes = fetch_quotes(&cfg.symbols);
                    if !quotes.is_empty() {
                        bus.send(Event::Quotes(quotes));
                    }
                    // Every minute while the watchlist is open, else now and then.
                    let every = if quotes_on { finance::REFRESH_MS } else { finance::WATCH_MS };
                    next_quotes = Instant::now() + Duration::from_millis(every);
                }
                if cfg.news && !cfg.symbols.is_empty() && now >= next_news {
                    if let Some(news) = fetch_news(&cfg.symbols, &cfg.names) {
                        bus.send(Event::News(news));
                        last_news = Some(Instant::now());
                    }
                    let every = if quotes_on { finance::NEWS_OPEN_MS } else { finance::NEWS_MS };
                    next_news = Instant::now() + Duration::from_millis(every);
                }
                if now >= next_update && (cfg.auto_update || manual_update) {
                    match check_update() {
                        Ok(Some(r)) => {
                            let version = r.version.clone();
                            *PENDING_RELEASE.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
                            bus.send(Event::UpdateAvailable { version });
                        }
                        Ok(None) if manual_update => {
                            bus.send(Event::Notice(format!("Ngambang {} is the latest version", ngambang_core::VERSION)))
                        }
                        Err(e) if manual_update => bus.send(Event::Notice(format!("Update check failed: {e}"))),
                        _ => {}
                    }
                    manual_update = false;
                    next_update = now + Duration::from_millis(update::CHECK_EVERY_MS);
                } else if now >= next_update {
                    next_update = now + Duration::from_millis(update::CHECK_EVERY_MS);
                }
            }
        });
        NetWorker { tx }
    }

    pub fn configure(&self, cfg: NetConfig) {
        let _ = self.tx.send(Msg::Config(cfg));
    }
    pub fn refresh(&self) {
        let _ = self.tx.send(Msg::Refresh);
    }
    pub fn finance(&self, on: bool) {
        let _ = self.tx.send(Msg::Finance(on));
    }
    pub fn check_update(&self) {
        let _ = self.tx.send(Msg::CheckUpdate);
    }
}

impl Drop for NetWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
    }
}

const INSTALLER: &str = r#"
param([string]$Zip, [string]$Sha, [string]$Dir, [int]$ParentPid)
$ErrorActionPreference = 'Stop'
$island = Join-Path $Dir 'ngambangw.exe'
try {
    Wait-Process -Id $ParentPid -Timeout 20 -ErrorAction SilentlyContinue
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $tmp = Join-Path $env:TEMP 'ngambang-update'
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Path $tmp | Out-Null
    $zipPath = Join-Path $tmp 'update.zip'
    Invoke-WebRequest -UseBasicParsing -Uri $Zip -OutFile $zipPath
    if ($Sha) {
        $shaPath = Join-Path $tmp 'update.sha256'
        Invoke-WebRequest -UseBasicParsing -Uri $Sha -OutFile $shaPath
        $want = ((Get-Content -Raw $shaPath).Trim() -split '\s+')[0].ToLower()
        $have = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLower()
        if ($want -ne $have) { throw 'The download did not match its checksum.' }
    }
    Expand-Archive -Path $zipPath -DestinationPath $tmp -Force
    $src = Get-ChildItem -Path $tmp -Directory | Select-Object -First 1
    Copy-Item -Path (Join-Path $src.FullName '*') -Destination $Dir -Force
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
} catch {
    Add-Type -AssemblyName PresentationFramework
    [System.Windows.MessageBox]::Show("Ngambang could not update:`n$($_.Exception.Message)`n`nThe current version will start again.", 'Ngambang') | Out-Null
}
Start-Process -FilePath $island
"#;

/// Start the installer for the pending release. Returns false (with a reason)
/// when it cannot run; the caller then quits so files can be replaced.
pub fn install_pending() -> Result<(), String> {
    let release = PENDING_RELEASE.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or("no update is waiting")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("unknown install folder")?.to_path_buf();
    // The folder must be writable (not Program Files without admin rights).
    let probe = dir.join(".ngambang-write-test");
    std::fs::write(&probe, b"ok").map_err(|_| format!("{} is not writable; download the update from the releases page", dir.display()))?;
    let _ = std::fs::remove_file(&probe);
    let script = std::env::temp_dir().join("ngambang-update.ps1");
    std::fs::write(&script, INSTALLER).map_err(|e| e.to_string())?;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-File"])
        .arg(&script)
        .arg("-Zip")
        .arg(&release.zip_url)
        .arg("-Sha")
        .arg(release.sha256_url.clone().unwrap_or_default())
        .arg("-Dir")
        .arg(&dir)
        .arg("-ParentPid")
        .arg(std::process::id().to_string())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("cannot start the installer: {e}"))?;
    Ok(())
}
