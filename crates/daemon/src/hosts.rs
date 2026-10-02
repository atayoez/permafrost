//! The section of `/etc/hosts` Permafrost manages.

use std::collections::BTreeSet;
use std::io::Write;
use std::net::IpAddr;
use std::path::Path;

const BEGIN: &str = "# BEGIN permafrost";
const END: &str = "# END permafrost";

/// Renders the managed block. Empty when there's nothing to block.
///
/// `community` hosts come from large downloaded lists and only get an IPv4
/// line, which halves the file. `redirects` send a hostname to another
/// address (SafeSearch); blocked hostnames win over redirects.
pub fn render(blocked: &BTreeSet<String>, community: &BTreeSet<&str>, redirects: &[(IpAddr, String)]) -> String {
    if blocked.is_empty() && community.is_empty() && redirects.is_empty() {
        return String::new();
    }
    let mut out = format!("{BEGIN} — managed by permafrostd, edits are overwritten\n");
    for (ip, host) in redirects {
        if !blocked.contains(host) && !community.contains(host.as_str()) {
            out.push_str(&format!("{ip} {host}\n"));
        }
    }
    for host in blocked {
        out.push_str(&format!("0.0.0.0 {host}\n:: {host}\n"));
    }
    for host in community {
        out.push_str(&format!("0.0.0.0 {host}\n"));
    }
    out.push_str(END);
    out.push('\n');
    out
}

/// Replaces the managed block in `existing` with `block`, keeping everything else.
pub fn splice(existing: &str, block: &str) -> String {
    let mut out = String::with_capacity(existing.len() + block.len());
    let mut inside = false;
    for line in existing.lines() {
        if line.starts_with(BEGIN) {
            inside = true;
        } else if inside && line.starts_with(END) {
            inside = false;
        } else if !inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !block.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(block);
    }
    out
}

/// Rewrites the file in place, so its inode and SELinux label stay the same.
pub fn write(path: &Path, block: &str) -> std::io::Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let updated = splice(&existing, block);
    if updated != existing {
        let mut file = std::fs::OpenOptions::new().write(true).truncate(true).create(true).open(path)?;
        file.write_all(updated.as_bytes())?;
        file.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "127.0.0.1 localhost\n::1 localhost\n";

    #[test]
    fn adds_and_removes_block() {
        let blocked = BTreeSet::from(["x.com".to_owned()]);
        let block = render(&blocked, &BTreeSet::new(), &[]);
        let with = splice(BASE, &block);
        assert!(with.starts_with(BASE));
        assert!(with.contains("0.0.0.0 x.com\n:: x.com\n"));
        assert_eq!(splice(&with, ""), BASE);
    }

    #[test]
    fn replaces_existing_block() {
        let first = splice(BASE, &render(&BTreeSet::from(["a.com".to_owned()]), &BTreeSet::new(), &[]));
        let second = splice(&first, &render(&BTreeSet::from(["b.com".to_owned()]), &BTreeSet::new(), &[]));
        assert!(!second.contains("a.com"));
        assert_eq!(second.matches(BEGIN).count(), 1);
    }

    #[test]
    fn community_hosts_get_one_line() {
        let block = render(&BTreeSet::new(), &BTreeSet::from(["porn.example"]), &[]);
        assert!(block.contains("0.0.0.0 porn.example\n"));
        assert!(!block.contains(":: porn.example"));
    }

    #[test]
    fn blocking_beats_safe_search() {
        let blocked = BTreeSet::from(["www.youtube.com".to_owned()]);
        let ip: IpAddr = "216.239.38.120".parse().unwrap();
        let block = render(&blocked, &BTreeSet::new(), &[(ip, "www.youtube.com".into()), (ip, "www.google.com".into())]);
        assert!(!block.contains("216.239.38.120 www.youtube.com"));
        assert!(block.contains("216.239.38.120 www.google.com"));
    }
}
