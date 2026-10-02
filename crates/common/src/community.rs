//! Large, community-maintained blocklists the service downloads and keeps fresh.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Adult,
    Gambling,
    Distractions,
    Harmful,
    Protection,
}

impl Category {
    pub const ALL: [Category; 5] =
        [Category::Adult, Category::Gambling, Category::Distractions, Category::Harmful, Category::Protection];

    pub fn title(self) -> &'static str {
        match self {
            Category::Adult => "Adult Content",
            Category::Gambling => "Gambling",
            Category::Distractions => "Distractions",
            Category::Harmful => "Harmful Sites",
            Category::Protection => "Protection",
        }
    }
}

pub struct Source {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: Category,
    /// Who maintains the list.
    pub project: &'static str,
    pub url: &'static str,
}

pub const SOURCES: &[Source] = &[
    Source {
        id: "stevenblack-porn",
        name: "Adult Sites",
        description: "Pornography and adult cam sites",
        category: Category::Adult,
        project: "StevenBlack/hosts",
        url: "https://raw.githubusercontent.com/StevenBlack/hosts/master/alternates/porn-only/hosts",
    },
    Source {
        id: "hagezi-nsfw",
        name: "Adult Sites, Extended",
        description: "A second, larger adult list for wider coverage",
        category: Category::Adult,
        project: "HaGeZi",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/nsfw-onlydomains.txt",
    },
    Source {
        id: "stevenblack-gambling",
        name: "Gambling Sites",
        description: "Betting, casino and poker sites",
        category: Category::Gambling,
        project: "StevenBlack/hosts",
        url: "https://raw.githubusercontent.com/StevenBlack/hosts/master/alternates/gambling-only/hosts",
    },
    Source {
        id: "hagezi-gambling",
        name: "Gambling Sites, Extended",
        description: "A much larger gambling list, including small and new sites",
        category: Category::Gambling,
        project: "HaGeZi",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/gambling.mini-onlydomains.txt",
    },
    Source {
        id: "stevenblack-social",
        name: "Social Media",
        description: "Social networks and their content servers",
        category: Category::Distractions,
        project: "StevenBlack/hosts",
        url: "https://raw.githubusercontent.com/StevenBlack/hosts/master/alternates/social-only/hosts",
    },
    Source {
        id: "stevenblack-fakenews",
        name: "Fake News",
        description: "Sites known for publishing misinformation",
        category: Category::Distractions,
        project: "StevenBlack/hosts",
        url: "https://raw.githubusercontent.com/StevenBlack/hosts/master/alternates/fakenews-only/hosts",
    },
    Source {
        id: "hagezi-fake",
        name: "Scams and Fake Shops",
        description: "Fake stores, fake streaming sites and subscription traps",
        category: Category::Harmful,
        project: "HaGeZi",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/fake-onlydomains.txt",
    },
    Source {
        id: "blocklistproject-drugs",
        name: "Drugs",
        description: "Sites selling or promoting illegal drugs",
        category: Category::Harmful,
        project: "The Block List Project",
        url: "https://blocklistproject.github.io/Lists/drugs.txt",
    },
    Source {
        id: "hagezi-piracy",
        name: "Piracy",
        description: "Illegal streaming, download and warez sites",
        category: Category::Harmful,
        project: "HaGeZi",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/anti.piracy-onlydomains.txt",
    },
    Source {
        id: "blocklistproject-torrent",
        name: "Torrent Sites",
        description: "Torrent trackers and indexes",
        category: Category::Harmful,
        project: "The Block List Project",
        url: "https://blocklistproject.github.io/Lists/torrent.txt",
    },
    Source {
        id: "hagezi-bypass",
        name: "Ways Around Blocks",
        description: "Encrypted DNS, VPN and proxy services that could get around Permafrost",
        category: Category::Protection,
        project: "HaGeZi",
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/doh-vpn-proxy-bypass-onlydomains.txt",
    },
    Source {
        id: "stevenblack-unified",
        name: "Ads and Malware",
        description: "Advertising, tracking and malware servers",
        category: Category::Protection,
        project: "StevenBlack/hosts",
        url: "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts",
    },
];

pub fn find(id: &str) -> Option<&'static Source> {
    SOURCES.iter().find(|s| s.id == id)
}

const IGNORED: &[&str] = &["localhost", "localhost.localdomain", "local", "broadcasthost", "0.0.0.0", "ip6-localhost"];

fn valid_host(host: &str) -> bool {
    host.contains('.')
        && host.len() <= 253
        && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        && !IGNORED.contains(&host)
}

/// Hostnames from a blocklist, either in hosts format (`0.0.0.0 example.com`)
/// or as one domain per line.
pub fn parse(text: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        let fields: Vec<&str> = line.split_whitespace().collect();
        let names: &[&str] = match fields.as_slice() {
            [] => continue,
            [ip, names @ ..] if matches!(*ip, "0.0.0.0" | "127.0.0.1" | "::" | "::1") => names,
            [_, ..] if fields[0].parse::<std::net::IpAddr>().is_ok() => continue,
            [single] => std::slice::from_ref(single),
            _ => continue,
        };
        for name in names {
            let host = name.trim_start_matches("*.").trim_end_matches('.').to_ascii_lowercase();
            if valid_host(&host) {
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
        assert_eq!(parse(text), ["a.example.org", "b.example.org", "example.com"]);
    }

    #[test]
    fn parses_domain_lists() {
        let text = "# Title: test\nexample.com\n*.wild.example\nlocalhost\n";
        assert_eq!(parse(text), ["example.com", "wild.example"]);
    }

    #[test]
    fn source_ids_are_unique() {
        for (i, a) in SOURCES.iter().enumerate() {
            assert!(SOURCES[i + 1..].iter().all(|b| b.id != a.id), "{}", a.id);
        }
    }
}
