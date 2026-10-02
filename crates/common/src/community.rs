//! Large, community-maintained blocklists the service downloads and keeps fresh.

pub struct Source {
    pub id: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub homepage: &'static str,
}

pub const SOURCES: &[Source] = &[
    Source {
        id: "stevenblack-porn",
        name: "Community Adult Site List",
        url: "https://raw.githubusercontent.com/StevenBlack/hosts/master/alternates/porn-only/hosts",
        homepage: "https://github.com/StevenBlack/hosts",
    },
    Source {
        id: "stevenblack-gambling",
        name: "Community Gambling Site List",
        url: "https://raw.githubusercontent.com/StevenBlack/hosts/master/alternates/gambling-only/hosts",
        homepage: "https://github.com/StevenBlack/hosts",
    },
];

pub fn find(id: &str) -> Option<&'static Source> {
    SOURCES.iter().find(|s| s.id == id)
}

const IGNORED: &[&str] = &["localhost", "localhost.localdomain", "local", "broadcasthost", "0.0.0.0", "ip6-localhost"];

/// Hostnames from a blocklist in hosts-file format (`0.0.0.0 example.com`).
pub fn parse_hosts(text: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        let mut fields = line.split_whitespace();
        if !matches!(fields.next(), Some("0.0.0.0" | "127.0.0.1" | "::" | "::1")) {
            continue;
        }
        for host in fields {
            let host = host.trim_end_matches('.').to_ascii_lowercase();
            let valid = host.contains('.')
                && host.len() <= 253
                && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
            if valid && !IGNORED.contains(&host.as_str()) {
                hosts.push(host);
            }
        }
    }
    hosts.sort_unstable();
    hosts.dedup();
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hosts_files() {
        let text = "# Title: test\n127.0.0.1 localhost\n0.0.0.0 0.0.0.0\n0.0.0.0 Example.COM # comment\n0.0.0.0 a.example.org b.example.org\n\n192.168.1.1 router.lan\nnot a line\n";
        assert_eq!(parse_hosts(text), ["a.example.org", "b.example.org", "example.com"]);
    }
}
