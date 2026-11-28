//! Finding the charger again after its address changed (e.g. a new router
//! handed out new addresses). Gentle on purpose: only `GET /RealTimeData`,
//! a few addresses at a time, and only the charger with the same ID counts.

use std::net::{Ipv4Addr, UdpSocket};
use std::time::Duration;

use tokio::task::JoinSet;

use crate::config::Address;
use crate::http::Http;

/// Addresses asked at the same time.
const PARALLEL: usize = 16;

/// The other addresses of the `/24` networks to look in: the charger's old
/// one and the one this machine is on now (they differ after a network
/// change). The old address itself is left out.
pub(crate) fn candidates(current: &Address, local: Option<Ipv4Addr>) -> Vec<Address> {
    let mut networks: Vec<[u8; 3]> = Vec::new();
    for ip in [current.ipv4(), local].into_iter().flatten() {
        if ip.is_loopback() || ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast() {
            continue;
        }
        let [a, b, c, _] = ip.octets();
        if !networks.contains(&[a, b, c]) {
            networks.push([a, b, c]);
        }
    }
    let skip = current.ipv4();
    networks
        .into_iter()
        .flat_map(|[a, b, c]| (1..=254).map(move |d| Ipv4Addr::new(a, b, c, d)))
        .filter(|ip| Some(*ip) != skip)
        .map(|ip| current.with_host(ip))
        .collect()
}

/// This machine's address on the network it would use to reach `towards`.
/// Connecting a UDP socket sends nothing; it only picks the route.
pub(crate) fn local_ipv4(towards: Option<Ipv4Addr>) -> Option<Ipv4Addr> {
    let target = towards.unwrap_or(Ipv4Addr::new(192, 0, 2, 1));
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((target, 80)).ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// Asks every candidate for its readings and returns the first one that
/// reports `id`.
pub(crate) async fn find(candidates: Vec<Address>, id: &str, timeout: Duration) -> Option<Address> {
    let http = Http::new(timeout).ok()?;
    for chunk in candidates.chunks(PARALLEL) {
        let mut probes = JoinSet::new();
        for address in chunk.iter().cloned() {
            let http = http.clone();
            let id = id.to_owned();
            probes.spawn(async move {
                let reading = http.read(&address).await.ok()?;
                (reading.id.as_deref() == Some(id.as_str())).then_some(address)
            });
        }
        while let Some(found) = probes.join_next().await {
            if let Ok(Some(address)) = found {
                probes.abort_all();
                return Some(address);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_in_the_old_and_the_current_network() {
        let current = Address::parse("192.0.2.10").unwrap();
        let same = candidates(&current, Some(Ipv4Addr::new(192, 0, 2, 77)));
        assert_eq!(same.len(), 253);
        assert!(!same.contains(&current));
        assert_eq!(same[0].to_string(), "192.0.2.1");
        assert_eq!(same[252].to_string(), "192.0.2.254");

        let moved = candidates(&current, Some(Ipv4Addr::new(198, 51, 100, 4)));
        assert_eq!(moved.len(), 253 + 254);
        assert!(moved.iter().any(|a| a.to_string() == "198.51.100.10"));

        let with_port = candidates(&Address::parse("192.0.2.10:8080").unwrap(), None);
        assert_eq!(with_port[0].to_string(), "192.0.2.1:8080");

        // A host name has no network to look in, and loopback is no network.
        let named = Address::parse("charger.local").unwrap();
        assert!(candidates(&named, None).is_empty());
        assert!(candidates(&named, Some(Ipv4Addr::LOCALHOST)).is_empty());
    }
}
