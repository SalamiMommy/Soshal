//! mDNS/DNS-SD LAN peer discovery.
//!
//! Complements the HMAC beacon broadcast in [`crate::lan`]: advertisers
//! register `_soshal._tcp` service records carrying the instance name
//! `soshal-<pubkey>`, browsers resolve them into candidate peer addresses.
//! mDNS is *discovery only* — trust is established later at the direct-socket
//! HMAC handshake, so spoofed records are harmless.
//!
//! Android requires the app to hold the Wi-Fi multicast lock (Dart side) for
//! mDNS frames to arrive.

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};

/// Service type advertised/browsed by Soshal peers.
pub const SERVICE_TYPE: &str = "_soshal._tcp.local.";

/// Instance name for a peer, encoding its pubkey: `soshal-<64 hex>`.
pub fn instance_name(pubkey: &str) -> String {
    format!("soshal-{pubkey}")
}

/// Extracts the pubkey from an instance name. `None` for foreign instances.
pub fn parse_instance_pubkey(instance: &str) -> Option<String> {
    let pk = instance.strip_prefix("soshal-")?;
    if pk.len() == 64 && pk.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(pk.to_string())
    } else {
        None
    }
}

/// A resolved peer advertised over mDNS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdnsPeer {
    pub pubkey: String,
    pub addr: SocketAddr,
    pub quic_port: Option<u16>,
}

/// Advertises this device as a `_soshal._tcp` service so LAN peers can find
/// it. Holds the daemon alive for the struct's lifetime; dropping the struct
/// stops the mDNS responder.
pub struct MdnsAdvertiser {
    #[allow(dead_code)] // kept alive for the responder's lifetime only
    daemon: ServiceDaemon,
}

impl MdnsAdvertiser {
    /// Registers the service. `ip` is the LAN address to advertise; when
    /// empty the first private, non-loopback IPv4 interface address is used.
    /// `quic_port` is advertised in TXT records if provided.
    pub fn start(
        pubkey: &str,
        ip: &str,
        port: u16,
        quic_port: Option<u16>,
    ) -> Result<Self, String> {
        let daemon = ServiceDaemon::new().map_err(|e| format!("mdns daemon: {e}"))?;
        let advertised = if ip.is_empty() {
            first_lan_ipv4().unwrap_or_else(|| "127.0.0.1".to_string())
        } else {
            ip.to_string()
        };
        let host = format!("{}.local.", instance_name(pubkey));
        let mut txt = HashMap::new();
        if let Some(qp) = quic_port {
            txt.insert("quic_port".to_string(), qp.to_string());
        }
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name(pubkey),
            &host,
            &advertised,
            port,
            Some(txt),
        )
        .map_err(|e| format!("mdns service info: {e}"))?;
        daemon
            .register(info)
            .map_err(|e| format!("mdns register: {e}"))?;
        Ok(Self { daemon })
    }
}

/// Browses the LAN for `_soshal._tcp` services.
pub struct MdnsBrowser {
    #[allow(dead_code)] // kept alive for the browser's lifetime only
    daemon: ServiceDaemon,
    events: mdns_sd::Receiver<ServiceEvent>,
    seen: HashSet<SocketAddr>,
}

impl MdnsBrowser {
    pub fn start() -> Result<Self, String> {
        let daemon = ServiceDaemon::new().map_err(|e| format!("mdns daemon: {e}"))?;
        let events = daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| format!("mdns browse: {e}"))?;
        Ok(Self {
            daemon,
            events,
            seen: HashSet::new(),
        })
    }

    /// Drains pending discovery events into a list of new peers (deduped,
    /// private-IP-only, foreign instances skipped).
    pub fn drain_peers(&mut self) -> Vec<MdnsPeer> {
        let mut peers = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            if let ServiceEvent::ServiceResolved(info) = event {
                let instance = info
                    .get_fullname()
                    .split('.')
                    .next()
                    .unwrap_or_default()
                    .to_string();
                let Some(pubkey) = parse_instance_pubkey(&instance) else {
                    continue;
                };
                let quic_port = info
                    .get_property_val_str("quic_port")
                    .and_then(|s| s.parse::<u16>().ok());
                for ip in info.get_addresses_v4() {
                    let ip = IpAddr::V4(*ip);
                    if !crate::lan::is_private_ip(ip) {
                        continue;
                    }
                    let addr = SocketAddr::new(ip, info.get_port());
                    if self.seen.insert(addr) {
                        peers.push(MdnsPeer {
                            pubkey: pubkey.clone(),
                            addr,
                            quic_port,
                        });
                    }
                }
            }
        }
        peers
    }
}

/// First private, non-loopback IPv4 interface address, if any.
fn first_lan_ipv4() -> Option<String> {
    let addrs = if_addrs::get_if_addrs().ok()?;
    addrs.into_iter().map(|a| a.ip()).find_map(|ip| match ip {
        IpAddr::V4(v4) if !v4.is_loopback() && crate::lan::is_private_ip(IpAddr::V4(v4)) => {
            Some(v4.to_string())
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn instance_name_roundtrip() {
        let pk = "ab".repeat(32);
        let name = instance_name(&pk);
        assert_eq!(parse_instance_pubkey(&name).as_deref(), Some(pk.as_str()));
        assert_eq!(parse_instance_pubkey("soshal-tooshort"), None);
        assert_eq!(parse_instance_pubkey("other-xyz"), None);
        assert_eq!(
            parse_instance_pubkey(&format!("soshal-{}", "zz".repeat(32))),
            None
        );
    }

    #[test]
    fn private_ip_filter_blocks_external_v4() {
        let ip = Ipv4Addr::new(8, 8, 8, 8);
        assert!(!crate::lan::is_private_ip(IpAddr::V4(ip)));
    }
}
