//! Whether a newer asterofix is out: GitHub's latest release, asked about at most once a day.
//! Only the AppImage asks; Nix and the AUR package update through their own package managers.
//! Nothing is downloaded or replaced here. The window says a release is out and opens its page.

use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const LATEST_RELEASE: &str = "https://api.github.com/repos/Kalkaro/asterofix/releases/latest";

/// How long an answer is trusted before GitHub is asked again.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// Without the tag's "v": "0.2.0".
    pub version: String,
    /// The release's page.
    pub url: String,
}

/// The last answer, in the cache folder.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Checked {
    /// Seconds past the epoch.
    at: u64,
    release: Option<Release>,
}

fn cache_file(paths: &Paths) -> PathBuf {
    paths.cache_dir().join("latest-release.json")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The latest release if it's newer than `current`. Asks `api` (GitHub's latest release) when
/// the last answer is a day old or missing, on the calling thread; a failed request is tried
/// again on the next start, and the last answer stands meanwhile.
pub fn check(paths: &Paths, api: &str, current: &str) -> Option<Release> {
    let cached: Option<Checked> =
        std::fs::read(cache_file(paths)).ok().and_then(|data| serde_json::from_slice(&data).ok());
    let fresh = cached.as_ref().is_some_and(|c| now().saturating_sub(c.at) < CHECK_EVERY.as_secs());
    let release = match cached {
        Some(c) if fresh => c.release,
        cached => match fetch(api) {
            Some(release) => {
                let checked = Checked { at: now(), release: Some(release.clone()) };
                if let Ok(data) = serde_json::to_vec(&checked) {
                    let _ = std::fs::create_dir_all(paths.cache_dir());
                    let _ = std::fs::write(cache_file(paths), data);
                }
                Some(release)
            }
            None => cached.and_then(|c| c.release),
        },
    };
    release.filter(|r| newer(&r.version, current))
}

fn fetch(api: &str) -> Option<Release> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).build().into();
    let mut response = agent.get(api).header("Accept", "application/vnd.github+json").call().ok()?;
    parse(&response.body_mut().read_to_string().ok()?)
}

/// GitHub's answer for the latest release; drafts and prereleases aren't offered.
fn parse(json: &str) -> Option<Release> {
    #[derive(Deserialize)]
    struct Answer {
        tag_name: String,
        html_url: String,
        #[serde(default)]
        draft: bool,
        #[serde(default)]
        prerelease: bool,
    }
    let a: Answer = serde_json::from_str(json).ok()?;
    let version = a.tag_name.trim_start_matches('v').to_owned();
    (!a.draft && !a.prerelease && !version.is_empty()).then_some(Release { version, url: a.html_url })
}

/// Whether version `a` comes after `b`, number by number: "0.10.0" is after "0.9.1". Anything
/// after a `-` (a prerelease or build tag) is ignored.
pub fn newer(a: &str, b: &str) -> bool {
    let numbers = |v: &str| -> Vec<u64> {
        let v = v.trim_start_matches('v');
        let v = v.split(['-', '+']).next().unwrap_or("");
        v.split('.').map(|n| n.parse().unwrap_or(0)).collect()
    };
    let (mut a, mut b) = (numbers(a), numbers(b));
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    a > b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tempdir;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn compares_versions_number_by_number() {
        assert!(newer("0.1.3", "0.1.2"));
        assert!(newer("0.10.0", "0.9.9"));
        assert!(newer("v1.0", "0.99.99"));
        assert!(newer("0.2", "0.1.9"));
        assert!(!newer("0.1.2", "0.1.2"));
        assert!(!newer("0.1.2", "0.1.10"));
        assert!(!newer("0.1.2-rc1", "0.1.2"), "a tag after the numbers is ignored");
    }

    #[test]
    fn reads_githubs_latest_release() {
        let json = r#"{"tag_name":"v0.2.0","html_url":"https://github.com/Kalkaro/asterofix/releases/tag/v0.2.0","draft":false,"prerelease":false,"assets":[]}"#;
        let want =
            Release { version: "0.2.0".into(), url: "https://github.com/Kalkaro/asterofix/releases/tag/v0.2.0".into() };
        assert_eq!(parse(json), Some(want));
        assert_eq!(parse(r#"{"tag_name":"v0.3.0","html_url":"u","prerelease":true}"#), None);
        assert_eq!(parse("not json"), None);
    }

    /// A stand-in for GitHub answering `tag` every time, counting the requests.
    fn github(tag: &'static str) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                counter.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let body = format!(r#"{{"tag_name":"{tag}","html_url":"https://example.org/{tag}"}}"#);
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(format!("{head}{body}").as_bytes());
            }
        });
        (url, hits)
    }

    /// GitHub is asked once a day; in between, and while it can't be reached, the last answer
    /// stands.
    #[test]
    fn asks_at_most_once_a_day() {
        let home = tempdir();
        let paths = Paths::under(home.path());
        let (url, hits) = github("v0.2.0");
        let found = check(&paths, &url, "0.1.2").expect("a newer release");
        assert_eq!((found.version.as_str(), found.url.as_str()), ("0.2.0", "https://example.org/v0.2.0"));
        assert_eq!(check(&paths, &url, "0.1.2"), Some(found.clone()));
        assert_eq!(hits.load(Ordering::SeqCst), 1, "the cached answer is used within the day");
        assert_eq!(check(&paths, &url, "0.2.0"), None, "not newer than what runs");

        // A day later, with GitHub unreachable.
        let stale = Checked { at: now() - CHECK_EVERY.as_secs() - 1, release: Some(found.clone()) };
        std::fs::write(cache_file(&paths), serde_json::to_vec(&stale).unwrap()).unwrap();
        assert_eq!(check(&paths, "http://127.0.0.1:9/latest", "0.1.2"), Some(found));
        check(&paths, &url, "0.1.2");
        assert_eq!(hits.load(Ordering::SeqCst), 2, "asked again once it was a day old");
    }
}
