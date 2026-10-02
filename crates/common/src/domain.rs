//! Turning what people type ("https://www.Reddit.com/r/linux") into hostnames.

/// Normalizes user input into a bare, lowercase domain, or `None` if it isn't one.
///
/// Accepts URLs, `*.` wildcards and a leading `www.`; all of them map to the
/// registrable part the user meant, since subdomain variants are added later.
pub fn normalize(input: &str) -> Option<String> {
    let mut s = input.trim().to_ascii_lowercase();
    if let Some((_, rest)) = s.split_once("://") {
        s = rest.to_owned();
    }
    if let Some(end) = s.find(['/', '?', '#']) {
        s.truncate(end);
    }
    if let Some((_, host)) = s.rsplit_once('@') {
        s = host.to_owned();
    }
    if let Some((host, port)) = s.rsplit_once(':')
        && port.chars().all(|c| c.is_ascii_digit())
    {
        s = host.to_owned();
    }
    let s = s.trim_end_matches('.');
    let s = s.strip_prefix("*.").unwrap_or(s);
    let s = s.strip_prefix("www.").unwrap_or(s);

    let valid = s.len() <= 253
        && s.contains('.')
        && s.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        // The last label of a real domain is never all digits; this rejects bare IPs.
        && !s.rsplit('.').next().unwrap_or("").chars().all(|c| c.is_ascii_digit());
    valid.then(|| s.to_owned())
}

/// Common hostnames people actually reach a site through.
///
/// `/etc/hosts` has no wildcards, so these cover the usual entry points.
pub const SUBDOMAIN_PREFIXES: &[&str] = &["www", "m", "mobile", "old", "new", "web", "app"];

/// Every hostname to block for `domain`, including common subdomains.
pub fn expand(domain: &str) -> impl Iterator<Item = String> + '_ {
    std::iter::once(domain.to_owned())
        .chain(SUBDOMAIN_PREFIXES.iter().map(move |p| format!("{p}.{domain}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_urls_and_prefixes() {
        assert_eq!(normalize("https://www.Reddit.com/r/linux").as_deref(), Some("reddit.com"));
        assert_eq!(normalize("  *.tiktok.com ").as_deref(), Some("tiktok.com"));
        assert_eq!(normalize("old.reddit.com:443").as_deref(), Some("old.reddit.com"));
        assert_eq!(normalize("user@example.org/path").as_deref(), Some("example.org"));
        assert_eq!(normalize("news.ycombinator.com.").as_deref(), Some("news.ycombinator.com"));
    }

    #[test]
    fn rejects_non_domains() {
        for bad in ["", "localhost", "192.168.1.1", "-bad.com", "spa ce.com", "a..b"] {
            assert_eq!(normalize(bad), None, "{bad:?} should be rejected");
        }
    }

    #[test]
    fn expands_common_subdomains() {
        let hosts: Vec<_> = expand("x.com").collect();
        assert_eq!(hosts[0], "x.com");
        assert!(hosts.contains(&"www.x.com".to_owned()));
        assert!(hosts.contains(&"m.x.com".to_owned()));
    }
}
