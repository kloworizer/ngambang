//! Update checks against GitHub Releases: compare versions and pick the
//! download for this machine. Pure; the platform does the HTTP and install.

use serde_json::Value;

pub const LATEST_URL: &str = "https://api.github.com/repos/kloworizer/ngambang/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/kloworizer/ngambang/releases/latest";
/// How often the island looks for a new release.
pub const CHECK_EVERY_MS: u64 = 24 * 60 * 60 * 1000;

/// (major, minor, patch) from "0.2.1" or "v0.2.1".
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = (it.next()??, it.next()??, it.next()??);
    it.next().is_none().then_some(v)
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub page_url: String,
    pub zip_url: String,
    pub sha256_url: Option<String>,
}

/// Read the GitHub "latest release" JSON and find the zip for `arch`
/// ("x64" or "arm64"). Drafts and prereleases are skipped.
pub fn parse_release(json: &str, arch: &str) -> Option<Release> {
    let v: Value = serde_json::from_str(json).ok()?;
    if v.get("draft").and_then(Value::as_bool) == Some(true) || v.get("prerelease").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let tag = v.get("tag_name")?.as_str()?;
    parse_version(tag)?;
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    let assets = v.get("assets")?.as_array()?;
    let url_of = |name: &str| {
        assets
            .iter()
            .find(|a| a.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|a| a.get("browser_download_url")?.as_str().map(String::from))
    };
    let zip = format!("ngambang-{version}-windows-{arch}.zip");
    let zip_url = url_of(&zip)?;
    // Only accept downloads from this repository's releases.
    if !zip_url.starts_with("https://github.com/kloworizer/ngambang/releases/download/") {
        return None;
    }
    Some(Release {
        page_url: v.get("html_url").and_then(Value::as_str).unwrap_or(RELEASES_PAGE).to_string(),
        sha256_url: url_of(&format!("{zip}.sha256")),
        zip_url,
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "tag_name": "v0.2.0", "draft": false, "prerelease": false,
        "html_url": "https://github.com/kloworizer/ngambang/releases/tag/v0.2.0",
        "assets": [
            {"name": "ngambang-0.2.0-windows-x64.zip", "browser_download_url": "https://github.com/kloworizer/ngambang/releases/download/v0.2.0/ngambang-0.2.0-windows-x64.zip"},
            {"name": "ngambang-0.2.0-windows-x64.zip.sha256", "browser_download_url": "https://github.com/kloworizer/ngambang/releases/download/v0.2.0/ngambang-0.2.0-windows-x64.zip.sha256"},
            {"name": "ngambang-0.2.0-windows-arm64.zip", "browser_download_url": "https://github.com/kloworizer/ngambang/releases/download/v0.2.0/ngambang-0.2.0-windows-arm64.zip"}
        ]
    }"#;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.10.2"), Some((0, 10, 2)));
        assert_eq!(parse_version("1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn picks_the_right_asset() {
        let r = parse_release(SAMPLE, "x64").unwrap();
        assert_eq!(r.version, "0.2.0");
        assert!(r.zip_url.ends_with("windows-x64.zip"));
        assert!(r.sha256_url.is_some());
        let a = parse_release(SAMPLE, "arm64").unwrap();
        assert!(a.sha256_url.is_none());
        assert!(parse_release(SAMPLE, "x86").is_none());
    }

    #[test]
    fn rejects_drafts_and_foreign_urls() {
        assert!(parse_release(&SAMPLE.replace("\"draft\": false", "\"draft\": true"), "x64").is_none());
        assert!(parse_release(
            &SAMPLE.replace(
                "https://github.com/kloworizer/ngambang/releases/download/v0.2.0/ngambang-0.2.0-windows-x64.zip\"",
                "https://evil.example/x.zip\""
            ),
            "x64"
        )
        .is_none());
        assert!(parse_release("not json", "x64").is_none());
    }
}
