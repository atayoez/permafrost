//! Forcing SafeSearch by pointing search engines at their filtered hosts.
//!
//! Google, Bing, DuckDuckGo and YouTube all document this DNS-based switch.

use std::net::{IpAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

const TARGETS: &[(&str, &[&str])] = &[
    ("forcesafesearch.google.com", &["google.com", "www.google.com"]),
    ("strict.bing.com", &["bing.com", "www.bing.com"]),
    ("safe.duckduckgo.com", &["duckduckgo.com", "www.duckduckgo.com"]),
    (
        "restrict.youtube.com",
        &[
            "youtube.com",
            "www.youtube.com",
            "m.youtube.com",
            "youtubei.googleapis.com",
            "youtube.googleapis.com",
            "www.youtube-nocookie.com",
        ],
    ),
];

const REFRESH: Duration = Duration::from_secs(60 * 60);

#[derive(Default)]
pub struct SafeSearch {
    entries: Vec<(IpAddr, String)>,
    resolved_at: Option<Instant>,
}

impl SafeSearch {
    /// Host redirects, resolving the targets at most once an hour.
    pub fn redirects(&mut self) -> &[(IpAddr, String)] {
        let stale = self.resolved_at.is_none_or(|t| t.elapsed() > REFRESH);
        if stale {
            let entries = resolve();
            // Keep the last good answer if we're offline right now.
            if !entries.is_empty() || self.entries.is_empty() {
                self.entries = entries;
            }
            self.resolved_at = Some(Instant::now());
        }
        &self.entries
    }
}

fn resolve() -> Vec<(IpAddr, String)> {
    let mut entries = Vec::new();
    for (target, hosts) in TARGETS {
        let Ok(addrs) = (*target, 443).to_socket_addrs() else {
            tracing::warn!("couldn't resolve {target}; SafeSearch for it is off until the next try");
            continue;
        };
        let addrs: Vec<IpAddr> = addrs.map(|a| a.ip()).collect();
        let picked = [addrs.iter().find(|a| a.is_ipv4()), addrs.iter().find(|a| a.is_ipv6())];
        for ip in picked.into_iter().flatten() {
            entries.extend(hosts.iter().map(|h| (*ip, (*h).to_owned())));
        }
    }
    entries
}
