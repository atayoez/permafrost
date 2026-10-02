//! The nftables rules that send systemd-resolved's lookups to `dns.rs`.
//!
//! Each upstream server gets its own local port, so the forwarder knows which
//! server resolved meant. Programs running as root, the service included,
//! are left alone, which keeps the forwarder's own queries from looping.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::dns::{BASE_PORT, Upstream};

const TABLE: &str = "permafrost_dns";

fn rules(upstreams: &[Upstream]) -> String {
    let mut chain = String::from("    meta skuid 0 return\n");
    for (index, Upstream { address: upstream, .. }) in upstreams.iter().enumerate() {
        let family = if upstream.is_ipv4() { "ip" } else { "ip6" };
        let port = BASE_PORT + index as u16;
        for protocol in ["udp", "tcp"] {
            chain.push_str(&format!("    {family} daddr {upstream} {protocol} dport 53 redirect to :{port}\n"));
        }
    }
    // Creating the table first makes the delete succeed when it doesn't exist yet.
    format!(
        "table inet {TABLE}\ndelete table inet {TABLE}\ntable inet {TABLE} {{\n  chain output {{\n    type nat hook output priority -100; policy accept;\n{chain}  }}\n}}\n"
    )
}

fn nft(script: &str) -> std::io::Result<()> {
    let mut child = Command::new("nft").arg("-f").arg("-").stdin(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    child.stdin.take().expect("piped stdin").write_all(script.as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(String::from_utf8_lossy(&output.stderr).trim().to_owned()));
    }
    Ok(())
}

pub fn install(upstreams: &[Upstream]) -> std::io::Result<()> {
    nft(&rules(upstreams))
}

pub fn remove() -> std::io::Result<()> {
    nft(&format!("table inet {TABLE}\ndelete table inet {TABLE}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_port_per_upstream() {
        let upstreams: Vec<Upstream> = ["193.196.64.1", "fd7a:115c:a1e0::53"]
            .iter()
            .map(|a| Upstream { address: a.parse().unwrap(), device: None })
            .collect();
        let script = rules(&upstreams);
        assert!(script.contains("meta skuid 0 return"));
        assert!(script.contains("ip daddr 193.196.64.1 udp dport 53 redirect to :5300"));
        assert!(script.contains("ip6 daddr fd7a:115c:a1e0::53 tcp dport 53 redirect to :5301"));
    }
}
