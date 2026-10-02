//! A small DNS forwarder that sees every lookup systemd-resolved makes.
//!
//! The firewall (`firewall.rs`) sends resolved's queries for upstream server
//! `n` to local port `BASE_PORT + n`, so each query is forwarded to the server
//! resolved picked, and split DNS (VPNs, Tailscale) keeps working. Lookups
//! let the service enforce allowlists, block whole domains including every
//! subdomain, and tell when a site with a daily limit is in use.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use hickory_proto::op::{Message, ResponseCode};
use hickory_proto::rr::RData;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket, UdpSocket};

pub const BASE_PORT: u16 = 5300;
/// Upstream servers we can follow; more are left alone.
pub const MAX_UPSTREAMS: usize = 8;
const TIMEOUT: Duration = Duration::from_secs(4);
/// A limited site counts as in use this long after it was looked up.
const QUERY_IN_USE: Duration = Duration::from_secs(90);
/// How long an address from a limited site's answer is remembered.
const ADDRESS_MEMORY: Duration = Duration::from_secs(30 * 60);

/// Domains that must keep resolving while an allowlist is active, so the
/// system keeps working: connectivity checks, time sync, and what the service
/// itself needs.
pub const ESSENTIAL_DOMAINS: &[&str] = &[
    "fedoraproject.org",
    "nmcheck.gnome.org",
    "connectivity-check.ubuntu.com",
    "pool.ntp.org",
    "time.cloudflare.com",
    "raw.githubusercontent.com",
    "blocklistproject.github.io",
    "forcesafesearch.google.com",
    "strict.bing.com",
    "safe.duckduckgo.com",
    "restrict.youtube.com",
];

/// What to do with lookups. Domains match themselves and every subdomain.
#[derive(Default, PartialEq)]
pub struct Policy {
    /// Allow only these (plus `ESSENTIAL_DOMAINS`), if set.
    pub allow: Option<HashSet<String>>,
    pub block: HashSet<String>,
    /// Domain → id of the block list with a daily limit it belongs to.
    pub limited: HashMap<String, String>,
}

impl Policy {
    pub fn needs_interception(&self) -> bool {
        self.allow.is_some() || !self.limited.is_empty()
    }
}

#[derive(Default)]
struct Usage {
    queried: HashMap<String, Instant>,
    addresses: HashMap<IpAddr, (String, Instant)>,
}

/// A server systemd-resolved queries, and the network interface it uses for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upstream {
    pub address: IpAddr,
    pub device: Option<String>,
}

#[derive(Default)]
pub struct Shared {
    pub policy: RwLock<Policy>,
    pub upstreams: RwLock<Vec<Upstream>>,
    usage: Mutex<Usage>,
}

/// The first of `name` and its parent domains that `found` accepts.
fn find<T>(name: &str, mut found: impl FnMut(&str) -> Option<T>) -> Option<T> {
    let mut domain = name;
    loop {
        if let Some(value) = found(domain) {
            return Some(value);
        }
        domain = domain.split_once('.')?.1;
    }
}

enum Verdict {
    Block,
    Forward { limited: Option<String> },
}

impl Shared {
    fn verdict(&self, name: &str) -> Verdict {
        let policy = self.policy.read().unwrap_or_else(|p| p.into_inner());
        if let Some(allow) = &policy.allow {
            let allowed = find(name, |d| (allow.contains(d) || ESSENTIAL_DOMAINS.contains(&d)).then_some(())).is_some();
            if !allowed {
                return Verdict::Block;
            }
        }
        if find(name, |d| policy.block.contains(d).then_some(())).is_some() {
            return Verdict::Block;
        }
        Verdict::Forward { limited: find(name, |d| policy.limited.get(d).cloned()) }
    }

    fn record(&self, list: String, answer: &Message) {
        let mut usage = self.usage.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        for record in &answer.answers {
            let address = match &record.data {
                RData::A(a) => IpAddr::V4(a.0),
                RData::AAAA(a) => IpAddr::V6(a.0),
                _ => continue,
            };
            usage.addresses.insert(address, (list.clone(), now));
        }
        usage.queried.insert(list, now);
    }

    /// Limited lists whose sites were looked up recently or have an open connection.
    pub fn lists_in_use(&self) -> BTreeSet<String> {
        let mut usage = self.usage.lock().unwrap_or_else(|p| p.into_inner());
        usage.addresses.retain(|_, (_, seen)| seen.elapsed() < ADDRESS_MEMORY);
        let mut lists: BTreeSet<String> = usage
            .queried
            .iter()
            .filter(|(_, at)| at.elapsed() < QUERY_IN_USE)
            .map(|(list, _)| list.clone())
            .collect();
        if !usage.addresses.is_empty() {
            for address in established_connections() {
                if let Some((list, _)) = usage.addresses.get(&address) {
                    lists.insert(list.clone());
                }
            }
        }
        lists
    }

    async fn answer(&self, port: u16, query: &[u8], tcp: bool) -> Option<Vec<u8>> {
        let request = Message::from_vec(query).ok()?;
        let name = request.queries.first()?.name().to_ascii().trim_end_matches('.').to_ascii_lowercase();
        let limited = match self.verdict(&name) {
            Verdict::Block => return refuse(&request, ResponseCode::NXDomain),
            Verdict::Forward { limited } => limited,
        };
        let upstream = self.upstreams.read().unwrap_or_else(|p| p.into_inner()).get(usize::from(port - BASE_PORT)).cloned();
        let Some(upstream) = upstream else {
            return refuse(&request, ResponseCode::ServFail);
        };
        let response = tokio::time::timeout(TIMEOUT, forward(&upstream, query, tcp)).await.ok()?.ok()?;
        if let Some(list) = limited
            && let Ok(answer) = Message::from_vec(&response)
        {
            self.record(list, &answer);
        }
        Some(response)
    }
}

fn refuse(request: &Message, code: ResponseCode) -> Option<Vec<u8>> {
    let mut response = Message::error_msg(request.metadata.id, request.metadata.op_code, code);
    response.metadata.recursion_desired = request.metadata.recursion_desired;
    response.metadata.recursion_available = true;
    response.add_queries(request.queries.clone());
    response.to_vec().ok()
}

/// Sends `query` to `upstream` out of the same interface systemd-resolved would use,
/// which matters when a VPN routes everything else elsewhere.
async fn forward(upstream: &Upstream, query: &[u8], tcp: bool) -> std::io::Result<Vec<u8>> {
    let server = SocketAddr::new(upstream.address, 53);
    let device = upstream.device.as_deref().map(str::as_bytes);
    if tcp {
        let socket = if upstream.address.is_ipv4() { TcpSocket::new_v4()? } else { TcpSocket::new_v6()? };
        if device.is_some() {
            // Needs root; without it the system routes the query.
            let _ = socket.bind_device(device);
        }
        let mut stream = socket.connect(server).await?;
        stream.write_all(&(query.len() as u16).to_be_bytes()).await?;
        stream.write_all(query).await?;
        let length = stream.read_u16().await?;
        let mut response = vec![0; usize::from(length)];
        stream.read_exact(&mut response).await?;
        Ok(response)
    } else {
        let local: SocketAddr = if upstream.address.is_ipv4() {
            (Ipv4Addr::UNSPECIFIED, 0).into()
        } else {
            (Ipv6Addr::UNSPECIFIED, 0).into()
        };
        let socket = UdpSocket::bind(local).await?;
        if device.is_some() {
            let _ = socket.bind_device(device);
        }
        socket.connect(server).await?;
        socket.send(query).await?;
        let mut response = vec![0; 4096];
        let n = socket.recv(&mut response).await?;
        response.truncate(n);
        Ok(response)
    }
}

/// Listens on every forwarding port, on IPv4 and IPv6 loopback.
pub async fn serve(shared: Arc<Shared>) -> std::io::Result<()> {
    for index in 0..MAX_UPSTREAMS {
        let port = BASE_PORT + index as u16;
        for host in [IpAddr::V4(Ipv4Addr::LOCALHOST), IpAddr::V6(Ipv6Addr::LOCALHOST)] {
            let address = SocketAddr::new(host, port);
            let udp = Arc::new(UdpSocket::bind(address).await?);
            tokio::spawn(serve_udp(shared.clone(), udp, port));
            let tcp = TcpListener::bind(address).await?;
            tokio::spawn(serve_tcp(shared.clone(), tcp, port));
        }
    }
    Ok(())
}

async fn serve_udp(shared: Arc<Shared>, socket: Arc<UdpSocket>, port: u16) {
    let mut buffer = vec![0; 4096];
    loop {
        let Ok((n, client)) = socket.recv_from(&mut buffer).await else { continue };
        let query = buffer[..n].to_vec();
        let (shared, socket) = (shared.clone(), socket.clone());
        tokio::spawn(async move {
            if let Some(response) = shared.answer(port, &query, false).await {
                let _ = socket.send_to(&response, client).await;
            }
        });
    }
}

async fn serve_tcp(shared: Arc<Shared>, listener: TcpListener, port: u16) {
    loop {
        let Ok((mut stream, _)) = listener.accept().await else { continue };
        let shared = shared.clone();
        tokio::spawn(async move {
            while let Ok(length) = stream.read_u16().await {
                let mut query = vec![0; usize::from(length)];
                if stream.read_exact(&mut query).await.is_err() {
                    return;
                }
                let Some(response) = shared.answer(port, &query, true).await else { return };
                let sent = stream.write_all(&(response.len() as u16).to_be_bytes()).await.is_ok()
                    && stream.write_all(&response).await.is_ok();
                if !sent {
                    return;
                }
            }
        });
    }
}

/// The servers systemd-resolved sends queries to, with their interfaces.
pub fn system_upstreams(resolved: Option<&zbus::blocking::Connection>) -> Vec<Upstream> {
    let from_resolved = resolved.and_then(|connection| {
        let proxy = zbus::blocking::Proxy::new(
            connection,
            "org.freedesktop.resolve1",
            "/org/freedesktop/resolve1",
            "org.freedesktop.resolve1.Manager",
        )
        .ok()?;
        let servers: Vec<(i32, i32, Vec<u8>)> = proxy.get_property("DNS").ok()?;
        Some(
            servers
                .into_iter()
                .filter_map(|(ifindex, _, bytes)| {
                    let address = match bytes.len() {
                        4 => IpAddr::from(<[u8; 4]>::try_from(bytes).ok()?),
                        16 => IpAddr::from(<[u8; 16]>::try_from(bytes).ok()?),
                        _ => return None,
                    };
                    Some(Upstream { address, device: interface_name(ifindex) })
                })
                .collect::<Vec<_>>(),
        )
    });
    let mut upstreams = from_resolved.unwrap_or_else(|| {
        let text = std::fs::read_to_string("/run/systemd/resolve/resolv.conf")
            .or_else(|_| std::fs::read_to_string("/etc/resolv.conf"))
            .unwrap_or_default();
        text.lines()
            .filter_map(|line| line.strip_prefix("nameserver"))
            .filter_map(|ip| ip.trim().split('%').next()?.parse::<IpAddr>().ok())
            .map(|address| Upstream { address, device: None })
            .collect()
    });
    upstreams.retain(|u| !u.address.is_loopback());
    upstreams.dedup_by(|a, b| a.address == b.address);
    upstreams.truncate(MAX_UPSTREAMS);
    upstreams
}

fn interface_name(ifindex: i32) -> Option<String> {
    if ifindex <= 0 {
        return None;
    }
    std::fs::read_dir("/sys/class/net").ok()?.flatten().find_map(|entry| {
        let index = std::fs::read_to_string(entry.path().join("ifindex")).ok()?;
        (index.trim().parse::<i32>().ok()? == ifindex).then(|| entry.file_name().to_string_lossy().into_owned())
    })
}

/// Remote addresses of established TCP connections.
fn established_connections() -> HashSet<IpAddr> {
    let mut addresses = HashSet::new();
    for (path, v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
        let Ok(table) = std::fs::read_to_string(path) else { continue };
        for line in table.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // 01 is TCP_ESTABLISHED.
            if fields.get(3) != Some(&"01") {
                continue;
            }
            let Some(remote) = fields.get(2).and_then(|f| f.split(':').next()) else { continue };
            if let Some(address) = parse_proc_address(remote, v6) {
                addresses.insert(address);
            }
        }
    }
    addresses
}

/// `/proc/net/tcp*` addresses are hex, in 32-bit words of host byte order.
fn parse_proc_address(hex: &str, v6: bool) -> Option<IpAddr> {
    let words: Vec<u32> = (0..hex.len() / 8)
        .map(|i| u32::from_str_radix(&hex[i * 8..i * 8 + 8], 16))
        .collect::<Result<_, _>>()
        .ok()?;
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    if v6 {
        let bytes: [u8; 16] = bytes.try_into().ok()?;
        let address = Ipv6Addr::from(bytes);
        Some(address.to_ipv4_mapped().map_or(IpAddr::V6(address), IpAddr::V4))
    } else {
        let bytes: [u8; 4] = bytes.try_into().ok()?;
        Some(IpAddr::V4(Ipv4Addr::from(bytes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_subdomains() {
        let shared = Shared::default();
        shared.policy.write().unwrap().block.insert("reddit.com".into());
        assert!(matches!(shared.verdict("old.reddit.com"), Verdict::Block));
        assert!(matches!(shared.verdict("notreddit.com"), Verdict::Forward { .. }));
    }

    #[test]
    fn allowlist_blocks_everything_else() {
        let shared = Shared::default();
        shared.policy.write().unwrap().allow = Some(HashSet::from(["wikipedia.org".to_owned()]));
        assert!(matches!(shared.verdict("en.wikipedia.org"), Verdict::Forward { .. }));
        assert!(matches!(shared.verdict("nmcheck.gnome.org"), Verdict::Forward { .. }), "essential");
        assert!(matches!(shared.verdict("youtube.com"), Verdict::Block));
    }

    #[test]
    fn parses_proc_addresses() {
        // 127.0.0.1 as it appears in /proc/net/tcp.
        assert_eq!(parse_proc_address("0100007F", false), Some(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert_eq!(
            parse_proc_address("0000000000000000FFFF00000100007F", true),
            Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            "IPv4-mapped"
        );
    }
}
