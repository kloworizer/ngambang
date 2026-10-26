//! Markets watchlist: symbol search and quotes from Yahoo Finance's public
//! endpoints (no key). One symbol format covers everything: shares (BBCA.JK,
//! AAPL), indices (^JKSE), crypto (BTC-USD), currencies (USDIDR=X), futures.

use serde::{Deserialize, Serialize};

/// Quotes refresh this often while the slip is open ...
pub const REFRESH_MS: u64 = 60_000;
/// ... and this often in the background, to notice big moves.
pub const WATCH_MS: u64 = 5 * 60_000;
/// Headlines for the watchlist refresh this often.
pub const NEWS_MS: u64 = 15 * 60_000;
/// A headline pops up only while it is this fresh (seconds).
pub const NEWS_FRESH_S: i64 = 3 * 3600;

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn search_url(query: &str) -> String {
    format!("https://query1.finance.yahoo.com/v1/finance/search?q={}&quotesCount=8&newsCount=0&listsCount=0", encode(query.trim()))
}

/// Headlines about a symbol (or a name, for symbols Yahoo has none for).
pub fn news_url(query: &str) -> String {
    format!("https://query1.finance.yahoo.com/v1/finance/search?q={}&quotesCount=0&newsCount=4&listsCount=0", encode(query.trim()))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewsItem {
    pub id: String,
    pub title: String,
    pub publisher: String,
    pub link: String,
    /// Unix seconds.
    pub time: i64,
    /// The watchlist symbol it was found for.
    pub symbol: String,
}

pub fn parse_news(json: &str, symbol: &str) -> Vec<NewsItem> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let text = |n: &serde_json::Value, k: &str| n.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    v.get("news")
        .and_then(|n| n.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|n| {
                    let (id, title, link) = (text(n, "uuid"), text(n, "title"), text(n, "link"));
                    if id.is_empty() || title.is_empty() || !link.starts_with("https://") {
                        return None;
                    }
                    let time = n.get("providerPublishTime").and_then(|t| t.as_i64()).unwrap_or(0);
                    Some(NewsItem { id, title, publisher: text(n, "publisher"), link, time, symbol: symbol.to_string() })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// How many whole steps of `step` percent a change is (toward zero), to
/// announce a move once per step: +3.2 % with 3 % steps is 1, −6.5 % is −2.
pub fn move_steps(change_pct: f64, step: f64) -> i32 {
    if step <= 0.0 || !change_pct.is_finite() {
        return 0;
    }
    (change_pct / step).trunc() as i32
}

pub fn chart_url(symbol: &str) -> String {
    format!("https://query1.finance.yahoo.com/v8/finance/chart/{}?range=1d&interval=15m", encode(symbol.trim()))
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    pub symbol: String,
    pub name: String,
    /// "Equity", "Index", "Cryptocurrency", "Currency", "ETF"…
    pub kind: String,
    pub exchange: String,
}

impl SearchHit {
    /// One line for a list: "BBCA.JK — Bank Central Asia Tbk · Equity, Jakarta".
    pub fn label(&self) -> String {
        let place = [self.kind.as_str(), self.exchange.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ");
        if place.is_empty() {
            format!("{} — {}", self.symbol, self.name)
        } else {
            format!("{} — {} · {place}", self.symbol, self.name)
        }
    }
}

pub fn parse_search(json: &str) -> Vec<SearchHit> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let text = |q: &serde_json::Value, k: &str| q.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    v.get("quotes")
        .and_then(|q| q.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|q| {
                    let symbol = text(q, "symbol");
                    if symbol.is_empty() {
                        return None;
                    }
                    let name =
                        [text(q, "longname"), text(q, "shortname")].into_iter().find(|n| !n.is_empty()).unwrap_or_else(|| symbol.clone());
                    Some(SearchHit { symbol, name, kind: text(q, "typeDisp"), exchange: text(q, "exchDisp") })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub symbol: String,
    pub name: String,
    pub price: f64,
    /// Against the previous close.
    pub change_pct: f64,
    pub currency: String,
    /// "EQUITY", "INDEX", "CRYPTOCURRENCY", "CURRENCY"…
    pub kind: String,
    /// Today's closes, oldest first, for the sparkline.
    pub points: Vec<f32>,
}

pub fn parse_chart(json: &str) -> Option<Quote> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let r = v.get("chart")?.get("result")?.get(0)?;
    let meta = r.get("meta")?;
    let num = |k: &str| meta.get(k).and_then(|x| x.as_f64());
    let text = |k: &str| meta.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let price = num("regularMarketPrice")?;
    let prev = num("chartPreviousClose").or_else(|| num("previousClose")).unwrap_or(0.0);
    let mut points: Vec<f32> = r
        .get("indicators")
        .and_then(|i| i.get("quote"))
        .and_then(|q| q.get(0))
        .and_then(|q| q.get("close"))
        .and_then(|c| c.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect())
        .unwrap_or_default();
    // At most 48 points: plenty for a sparkline.
    if points.len() > 48 {
        let step = points.len() as f32 / 48.0;
        points = (0..48).map(|i| points[(i as f32 * step) as usize]).collect();
    }
    let symbol = text("symbol");
    let name = [text("shortName"), text("longName")].into_iter().find(|n| !n.is_empty()).unwrap_or_else(|| symbol.clone());
    Some(Quote {
        symbol,
        name,
        price,
        change_pct: if prev > 0.0 { (price - prev) / prev * 100.0 } else { 0.0 },
        currency: text("currency"),
        kind: text("instrumentType"),
        points,
    })
}

fn group(int: u64) -> String {
    let s = int.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A price as people read it: "Rp 5,025", "$80,785", "7,123.45", "0.6213".
pub fn format_price(q: &Quote) -> String {
    let p = q.price.abs();
    let whole = matches!(q.currency.as_str(), "IDR" | "JPY" | "KRW" | "VND") || p >= 10_000.0;
    let decimals = if whole {
        0
    } else if p >= 1.0 {
        2
    } else {
        4
    };
    let scaled = (p * 10f64.powi(decimals)).round() as u64;
    let unit = 10u64.pow(decimals as u32);
    let mut text = group(scaled / unit);
    if decimals > 0 {
        text += &format!(".{:0width$}", scaled % unit, width = decimals as usize);
    }
    if q.price < 0.0 {
        text.insert(0, '-');
    }
    // Indices are points and exchange rates are ratios: no currency sign.
    if matches!(q.kind.as_str(), "INDEX" | "CURRENCY") {
        return text;
    }
    match q.currency.as_str() {
        "USD" => format!("${text}"),
        "IDR" => format!("Rp {text}"),
        "EUR" => format!("€{text}"),
        "GBP" => format!("£{text}"),
        "JPY" => format!("¥{text}"),
        "" => text,
        other => format!("{text} {other}"),
    }
}

/// "+1.25%" / "−0.40%".
pub fn format_change(pct: f64) -> String {
    if pct < 0.0 {
        format!("−{:.2}%", -pct)
    } else {
        format!("+{pct:.2}%")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_results() {
        let hits = parse_search(include_str!("../testdata/yahoo_search.json"));
        assert!(hits.len() >= 5);
        let bbca = hits.iter().find(|h| h.symbol == "BBCA.JK").unwrap();
        assert_eq!(bbca.name, "PT Bank Central Asia Tbk");
        assert_eq!(bbca.label(), "BBCA.JK — PT Bank Central Asia Tbk · Equity, Jakarta");
        assert!(parse_search("not json").is_empty());
        assert_eq!(
            search_url(" btc usd "),
            "https://query1.finance.yahoo.com/v1/finance/search?q=btc%20usd&quotesCount=8&newsCount=0&listsCount=0"
        );
        assert!(chart_url("^JKSE").contains("/chart/%5EJKSE?"));
    }

    #[test]
    fn news_items() {
        let news = parse_news(include_str!("../testdata/yahoo_news.json"), "AAPL");
        assert_eq!(news.len(), 3);
        assert!(news[0].title.starts_with("Apple seeks more iPhone Duo"));
        assert_eq!(news[0].publisher, "Investing.com");
        assert_eq!(news[0].time, 1_792_973_405);
        assert!(news.iter().all(|n| n.symbol == "AAPL" && n.link.starts_with("https://")));
        assert!(parse_news("{}", "X").is_empty());
        assert!(news_url("^JKSE").contains("q=%5EJKSE&quotesCount=0&newsCount=4"));
    }

    #[test]
    fn moves_count_whole_steps() {
        assert_eq!(move_steps(3.2, 3.0), 1);
        assert_eq!(move_steps(2.9, 3.0), 0);
        assert_eq!(move_steps(-6.5, 3.0), -2);
        assert_eq!(move_steps(f64::NAN, 3.0), 0);
        assert_eq!(move_steps(5.0, 0.0), 0);
    }

    #[test]
    fn chart_quote() {
        let q = parse_chart(include_str!("../testdata/yahoo_chart.json")).unwrap();
        assert_eq!(q.symbol, "BBCA.JK");
        assert_eq!(q.price, 5025.0);
        assert!((q.change_pct - (5025.0 - 5175.0) / 5175.0 * 100.0).abs() < 1e-9);
        assert_eq!(q.currency, "IDR");
        assert!(!q.points.is_empty() && q.points.len() <= 48);
        assert_eq!(format_price(&q), "Rp 5,025");
        assert_eq!(format_change(q.change_pct), "−2.90%");
        assert!(parse_chart("{\"chart\":{\"result\":null}}").is_none());
    }

    #[test]
    fn prices_read_naturally() {
        let q = |price: f64, currency: &str, kind: &str| Quote {
            symbol: String::new(),
            name: String::new(),
            price,
            change_pct: 0.0,
            currency: currency.into(),
            kind: kind.into(),
            points: vec![],
        };
        assert_eq!(format_price(&q(80785.26, "USD", "CRYPTOCURRENCY")), "$80,785");
        assert_eq!(format_price(&q(231.5, "USD", "EQUITY")), "$231.50");
        assert_eq!(format_price(&q(7123.456, "IDR", "INDEX")), "7,123");
        assert_eq!(format_price(&q(16250.0, "IDR", "CURRENCY")), "16,250");
        assert_eq!(format_price(&q(0.62134, "USD", "CRYPTOCURRENCY")), "$0.6213");
        assert_eq!(format_price(&q(12.5, "SGD", "EQUITY")), "12.50 SGD");
        assert_eq!(format_change(1.249), "+1.25%");
    }
}
