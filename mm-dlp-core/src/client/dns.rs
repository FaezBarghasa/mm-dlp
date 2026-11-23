use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;

use crate::error::EngineError;

pub const DEFAULT_DNS_PRIMARY: &str = "1.1.1.1:53";
pub const DEFAULT_DNS_FALLBACK: &str = "9.9.9.9:53";
pub const DEFAULT_DNS_SECONDARY: &str = "1.0.0.1:53";
pub const DEFAULT_DNS_FALLBACK_SECONDARY: &str = "149.112.112.112:53";

/// Returns default DNS servers (1.1.1.1 primary, 9.9.9.9 fallback).
pub fn default_dns_servers() -> Vec<SocketAddr> {
    vec![
        DEFAULT_DNS_PRIMARY.parse().expect("valid primary DNS"),
        DEFAULT_DNS_FALLBACK.parse().expect("valid fallback DNS"),
        DEFAULT_DNS_SECONDARY.parse().expect("valid secondary DNS"),
        DEFAULT_DNS_FALLBACK_SECONDARY
            .parse()
            .expect("valid fallback secondary DNS"),
    ]
}

/// Parses comma-separated, space-separated, or semicolon-separated DNS server specifications.
/// Accepts "1.1.1.1", "1.1.1.1:53", "9.9.9.9", "[2606:4700:4700::1111]:53", etc.
pub fn parse_dns_servers(s: &str) -> Result<Vec<SocketAddr>, EngineError> {
    let mut addrs = Vec::new();
    for part in s.split([',', ' ', ';']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let addr = if part.contains(':') && !part.ends_with(']') {
            part.parse::<SocketAddr>().map_err(|e| {
                EngineError::InvalidConfig(format!("Invalid DNS address '{}': {}", part, e))
            })?
        } else if let Ok(ip) = part.parse::<IpAddr>() {
            SocketAddr::new(ip, 53)
        } else {
            let with_port = format!("{}:53", part);
            with_port.parse::<SocketAddr>().map_err(|e| {
                EngineError::InvalidConfig(format!("Invalid DNS address '{}': {}", part, e))
            })?
        };
        addrs.push(addr);
    }
    if addrs.is_empty() {
        return Err(EngineError::InvalidConfig(
            "No valid DNS server addresses provided".to_string(),
        ));
    }
    Ok(addrs)
}

/// Builds an RFC 1035 wire-format DNS query packet for standard A/AAAA record resolution.
pub fn build_dns_query(hostname: &str, qtype: u16, tx_id: u16) -> Result<Vec<u8>, EngineError> {
    let mut packet = Vec::with_capacity(64);
    // Header
    packet.extend_from_slice(&tx_id.to_be_bytes()); // ID
    packet.extend_from_slice(&0x0100u16.to_be_bytes()); // Flags: RD = 1 (Recursion Desired)
    packet.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1
    packet.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT = 0
    packet.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT = 0
    packet.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT = 0

    // QNAME: length-prefixed labels
    for label in hostname.split('.') {
        if label.is_empty() {
            continue;
        }
        if label.len() > 63 {
            return Err(EngineError::InvalidConfig(format!(
                "DNS label '{}' exceeds maximum 63 bytes",
                label
            )));
        }
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0); // Root label null terminator

    packet.extend_from_slice(&qtype.to_be_bytes()); // QTYPE (1 = A, 28 = AAAA)
    packet.extend_from_slice(&1u16.to_be_bytes()); // QCLASS (1 = IN)

    Ok(packet)
}

/// Parses an RFC 1035 wire-format DNS response packet and extracts resolved IP addresses.
pub fn parse_dns_response(buf: &[u8], expected_tx_id: u16) -> Result<Vec<IpAddr>, EngineError> {
    if buf.len() < 12 {
        return Err(EngineError::Network(
            "DNS response packet truncated (less than 12 bytes)".into(),
        ));
    }
    let tx_id = u16::from_be_bytes([buf[0], buf[1]]);
    if tx_id != expected_tx_id {
        return Err(EngineError::Network(
            "DNS response transaction ID mismatch".into(),
        ));
    }
    let flags = u16::from_be_bytes([buf[2], buf[3]]);
    let rcode = flags & 0x000F;
    if rcode != 0 {
        return Err(EngineError::Network(format!(
            "DNS server returned error code RCODE {}",
            rcode
        )));
    }
    let qdcount = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let ancount = u16::from_be_bytes([buf[6], buf[7]]) as usize;

    let mut offset = 12;

    // Skip question section
    for _ in 0..qdcount {
        while offset < buf.len() {
            let len = buf[offset] as usize;
            offset += 1;
            if len == 0 {
                break;
            }
            if len >= 192 {
                offset += 1; // 2-byte compression pointer
                break;
            }
            offset += len;
        }
        offset += 4; // QTYPE (2) + QCLASS (2)
    }

    let mut ips = Vec::new();

    fn skip_name(buf: &[u8], mut offset: usize) -> Result<usize, EngineError> {
        while offset < buf.len() {
            let len = buf[offset] as usize;
            if len == 0 {
                return Ok(offset + 1);
            }
            if len >= 192 {
                return Ok(offset + 2);
            }
            offset += 1 + len;
        }
        Err(EngineError::Network(
            "Malformed domain name in DNS response".into(),
        ))
    }

    for _ in 0..ancount {
        if offset >= buf.len() {
            break;
        }
        offset = skip_name(buf, offset)?;
        if offset + 10 > buf.len() {
            break;
        }
        let rtype = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
        offset += 8; // skip TYPE (2), CLASS (2), TTL (4)
        let rdlength = u16::from_be_bytes([buf[offset], buf[offset + 1]]) as usize;
        offset += 2;

        if offset + rdlength > buf.len() {
            break;
        }

        if rtype == 1 && rdlength == 4 {
            // A record (IPv4)
            let ip = Ipv4Addr::new(
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            );
            ips.push(IpAddr::V4(ip));
        } else if rtype == 28 && rdlength == 16 {
            // AAAA record (IPv6)
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&buf[offset..offset + 16]);
            let ip = Ipv6Addr::from(octets);
            ips.push(IpAddr::V6(ip));
        }

        offset += rdlength;
    }

    Ok(ips)
}

/// Sends a DNS query to a specific DNS server over UDP with an explicit timeout.
pub async fn query_dns_server(
    server: SocketAddr,
    hostname: &str,
    timeout_dur: Duration,
) -> Result<Vec<IpAddr>, EngineError> {
    let socket = if server.is_ipv6() {
        UdpSocket::bind("[::]:0").await
    } else {
        UdpSocket::bind("0.0.0.0:0").await
    }
    .map_err(|e| EngineError::Network(format!("Failed to bind local UDP socket for DNS: {}", e)))?;

    socket.connect(server).await.map_err(|e| {
        EngineError::Network(format!(
            "Failed to connect UDP socket to DNS {}: {}",
            server, e
        ))
    })?;

    let tx_id = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        & 0xFFFF) as u16;

    let query_packet = build_dns_query(hostname, 1, tx_id)?;

    socket.send(&query_packet).await.map_err(|e| {
        EngineError::Network(format!("Failed to send DNS query to {}: {}", server, e))
    })?;

    let mut buf = [0u8; 1024];
    let len = match tokio::time::timeout(timeout_dur, socket.recv(&mut buf)).await {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => {
            return Err(EngineError::Network(format!(
                "DNS recv error from {}: {}",
                server, e
            )));
        }
        Err(_) => {
            return Err(EngineError::Network(format!(
                "DNS query timed out waiting for server {}",
                server
            )));
        }
    };

    parse_dns_response(&buf[..len], tx_id)
}

type DnsCache = Arc<RwLock<HashMap<String, (Vec<SocketAddr>, Instant)>>>;

/// Robust DNS resolver querying primary DNS (1.1.1.1) and failing over to 9.9.9.9,
/// with final fallback to the operating system's getaddrinfo.
#[derive(Clone, Debug)]
pub struct CustomDnsResolver {
    servers: Arc<Vec<SocketAddr>>,
    timeout: Duration,
    cache: DnsCache,
}

impl CustomDnsResolver {
    pub fn new(servers: Vec<SocketAddr>) -> Self {
        Self {
            servers: Arc::new(if servers.is_empty() {
                default_dns_servers()
            } else {
                servers
            }),
            timeout: Duration::from_millis(2000),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_default_servers() -> Self {
        Self::new(default_dns_servers())
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn servers(&self) -> &[SocketAddr] {
        &self.servers
    }

    /// Resolves a hostname to a list of SocketAddrs with port.
    pub async fn resolve_host(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Vec<SocketAddr>, EngineError> {
        let host_clean = host.trim_start_matches('[').trim_end_matches(']');

        // Direct IP parse
        if let Ok(ip) = host_clean.parse::<IpAddr>() {
            return Ok(vec![SocketAddr::new(ip, port)]);
        }

        // Cache lookup (60-second TTL)
        {
            let cache = self.cache.read().await;
            if let Some((cached_addrs, cached_time)) = cache.get(host_clean)
                && cached_time.elapsed() < Duration::from_secs(60)
            {
                return Ok(cached_addrs
                    .iter()
                    .map(|a| SocketAddr::new(a.ip(), port))
                    .collect());
            }
        }

        // Query configured servers sequentially (1.1.1.1 -> 9.9.9.9 -> secondary)
        for &server in self.servers.iter() {
            if let Ok(ips) = query_dns_server(server, host_clean, self.timeout).await
                && !ips.is_empty()
            {
                let addrs: Vec<SocketAddr> = ips
                    .into_iter()
                    .map(|ip| SocketAddr::new(ip, port))
                    .collect();

                let mut cache = self.cache.write().await;
                cache.insert(host_clean.to_string(), (addrs.clone(), Instant::now()));
                return Ok(addrs);
            }
        }

        // Safe OS system fallback if UDP queries are intercepted or firewalled
        let target = format!("{}:{}", host_clean, port);
        if let Ok(system_addrs) = std::net::ToSocketAddrs::to_socket_addrs(&target) {
            let collected: Vec<SocketAddr> = system_addrs.collect();
            if !collected.is_empty() {
                let mut cache = self.cache.write().await;
                cache.insert(host_clean.to_string(), (collected.clone(), Instant::now()));
                return Ok(collected);
            }
        }

        Err(EngineError::Network(format!(
            "Failed to resolve hostname '{}' through DNS servers {:?} and system fallback",
            host_clean, self.servers
        )))
    }
}

impl Default for CustomDnsResolver {
    fn default() -> Self {
        Self::with_default_servers()
    }
}

impl reqwest::dns::Resolve for CustomDnsResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let this = self.clone();
        Box::pin(async move {
            let host = name.as_str();
            let addrs = this
                .resolve_host(host, 0)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            let iter: Box<dyn Iterator<Item = SocketAddr> + Send> = Box::new(addrs.into_iter());
            Ok(iter)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_dns_servers_list() {
        let defaults = default_dns_servers();
        assert!(!defaults.is_empty());
        assert_eq!(defaults[0], "1.1.1.1:53".parse().unwrap());
        assert_eq!(defaults[1], "9.9.9.9:53".parse().unwrap());
    }

    #[test]
    fn test_parse_custom_dns_servers() {
        let custom = parse_dns_servers("8.8.8.8, 8.8.4.4:53; 1.1.1.1").unwrap();
        assert_eq!(custom.len(), 3);
        assert_eq!(custom[0], "8.8.8.8:53".parse().unwrap());
        assert_eq!(custom[1], "8.8.4.4:53".parse().unwrap());
        assert_eq!(custom[2], "1.1.1.1:53".parse().unwrap());
    }

    #[test]
    fn test_build_and_parse_dns_query() {
        let hostname = "www.youtube.com";
        let query = build_dns_query(hostname, 1, 0x1234).unwrap();
        assert!(query.len() > 12);
        assert_eq!(&query[0..2], &0x1234u16.to_be_bytes());
        // Verify label encoding: 3www7youtube3com0
        assert_eq!(query[12], 3);
        assert_eq!(&query[13..16], b"www");
    }

    #[tokio::test]
    async fn test_custom_dns_resolver_ip_direct() {
        let resolver = CustomDnsResolver::default();
        let addrs = resolver.resolve_host("127.0.0.1", 80).await.unwrap();
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0], "127.0.0.1:80".parse().unwrap());
    }
}
