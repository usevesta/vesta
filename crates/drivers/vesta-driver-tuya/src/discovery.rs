//! Finding devices on the home network. Addresses come from DHCP and change,
//! but every Tuya device announces itself every few seconds with a UDP
//! broadcast holding its ID, address and protocol version:
//!
//! | Port | Who | Format |
//! |---|---|---|
//! | 6666 | 3.1 | 55AA frame, plain JSON |
//! | 6667 | 3.2–3.5 | 55AA frame, JSON in AES-ECB (3.3); 6699 frame, AES-GCM (3.4/3.5) |
//! | 7000 | 3.5 | 6699 frame, AES-GCM (answers to app requests) |
//!
//! Both encrypted forms use a well-known key (MD5 of a constant string),
//! so they hide nothing: they only identify the device. The listener is
//! passive (it never sends anything) and shared by every Tuya device of a
//! hub. A device is only ever trusted after its session proves it knows
//! the device's own local key, so a fake broadcast can at worst point VESTA
//! at an address that fails the handshake.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, OnceLock, PoisonError, RwLock};
use std::time::Duration;

use serde_json::Value;
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::crypto::{self, Key};
use crate::frame::{self, Check, Framing};
use crate::protocol::Version;

/// Ports devices broadcast on.
pub const BROADCAST_PORTS: [u16; 3] = [6666, 6667, 7000];
/// `MD5("yGAdlopoPVldABfn")`, the key every Tuya device uses for its
/// broadcasts.
pub const UDP_KEY: Key = [
    0x6c, 0x1e, 0xc8, 0xe2, 0xbb, 0x9b, 0xb5, 0x9a, 0xb5, 0x0b, 0x0d, 0xaf, 0x64, 0x9b, 0x41, 0x0a,
];
/// An address heard longer ago than this is only a hint.
pub const FRESH_FOR: Duration = Duration::from_secs(300);

/// What one broadcast says.
#[derive(Clone, PartialEq, Eq)]
pub struct Announcement {
    pub device_id: String,
    pub ip: Ipv4Addr,
    pub version: Option<Version>,
}

impl std::fmt::Debug for Announcement {
    /// Without the device ID: IDs never reach logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Announcement")
            .field("ip", &self.ip)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

/// The JSON inside a broadcast, from any of the three formats.
fn broadcast_json(datagram: &[u8]) -> Option<Vec<u8>> {
    if datagram.starts_with(&frame::PREFIX_6699) {
        let (raw, _) = frame::decode(datagram, Framing::Frame6699(&UDP_KEY)).ok()??;
        let mut body = raw.body;
        // A return code may come first.
        if !body.starts_with(b"{") && body.get(4) == Some(&b'{') {
            body.drain(..4);
        }
        while body.last() == Some(&0) {
            body.pop();
        }
        return Some(body);
    }
    if datagram.starts_with(&frame::PREFIX_55AA) {
        let (raw, _) = frame::decode(datagram, Framing::Frame55aa(Check::Crc)).ok()??;
        // Broadcasts carry a return code before the body.
        let body = raw.body.get(4..)?;
        if body.starts_with(b"{") {
            return Some(body.to_vec());
        }
        return crypto::ecb_decrypt(&UDP_KEY, body, true).ok();
    }
    None
}

/// Reads one broadcast. `source` is the address it came from, which is
/// preferred over the address the device claims (it is what answers).
pub fn parse(datagram: &[u8], source: IpAddr) -> Option<Announcement> {
    let json = broadcast_json(datagram)?;
    let value: Value = serde_json::from_slice(&json).ok()?;
    // Apps answer on the same ports; only devices count.
    if value.get("from").and_then(Value::as_str) == Some("app") {
        return None;
    }
    let device_id = value
        .get("gwId")
        .or_else(|| value.get("devId"))
        .and_then(Value::as_str)?
        .to_owned();
    if device_id.is_empty() || device_id.len() > 64 {
        return None;
    }
    let claimed = value
        .get("ip")
        .and_then(Value::as_str)
        .and_then(|ip| ip.parse::<Ipv4Addr>().ok());
    let ip = match source {
        IpAddr::V4(ip) if !ip.is_unspecified() => ip,
        _ => claimed?,
    };
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .and_then(Version::parse);
    Some(Announcement {
        device_id,
        ip,
        version,
    })
}

/// Where a device was last heard from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub ip: Ipv4Addr,
    pub version: Option<Version>,
    pub at: Instant,
}

/// Devices heard on the network, by ID.
pub struct Discovery {
    seen: RwLock<HashMap<String, Seen>>,
    /// Bumped on every new or moved device.
    changed: watch::Sender<u64>,
    /// Where it listens.
    local: Vec<SocketAddr>,
}

impl std::fmt::Debug for Discovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Discovery")
            .field("listening", &self.local.len())
            .finish_non_exhaustive()
    }
}

fn bind_shared(addr: SocketAddr) -> std::io::Result<std::net::UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    // Other listeners (another hub, a scanner) may want the same ports.
    socket.set_reuse_address(true)?;
    #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    Ok(socket.into())
}

static SHARED: OnceLock<Arc<Discovery>> = OnceLock::new();

impl Discovery {
    /// A registry that listens nowhere (tests, or when no port could be
    /// opened): only configured addresses are used.
    pub fn idle() -> Arc<Self> {
        Arc::new(Self {
            seen: RwLock::new(HashMap::new()),
            changed: watch::Sender::new(0),
            local: Vec::new(),
        })
    }

    /// The hub-wide listener on the broadcast ports, started on first use.
    /// Must be called from within a Tokio runtime.
    pub fn shared() -> Arc<Self> {
        Arc::clone(SHARED.get_or_init(|| {
            let addrs: Vec<SocketAddr> = BROADCAST_PORTS
                .iter()
                .map(|port| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), *port))
                .collect();
            Self::listen(&addrs)
        }))
    }

    /// Listens on `addrs` (ports that cannot be opened are skipped and
    /// logged). Must be called from within a Tokio runtime.
    pub fn listen(addrs: &[SocketAddr]) -> Arc<Self> {
        let mut sockets = Vec::new();
        for addr in addrs {
            match bind_shared(*addr).and_then(UdpSocket::from_std) {
                Ok(socket) => sockets.push(socket),
                Err(error) => {
                    tracing::warn!(port = addr.port(), %error, "cannot listen for Tuya device broadcasts on this port");
                }
            }
        }
        let discovery = Arc::new(Self {
            seen: RwLock::new(HashMap::new()),
            changed: watch::Sender::new(0),
            local: sockets
                .iter()
                .filter_map(|socket| socket.local_addr().ok())
                .collect(),
        });
        if sockets.is_empty() {
            tracing::warn!(
                "not listening for Tuya device broadcasts: devices are only reached at their configured address"
            );
        }
        for socket in sockets {
            let weak = Arc::downgrade(&discovery);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 2048];
                loop {
                    let Ok((len, from)) = socket.recv_from(&mut buf).await else {
                        // Transient (e.g. ICMP errors on some systems).
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        continue;
                    };
                    let Some(discovery) = weak.upgrade() else {
                        return;
                    };
                    if let Some(announcement) = parse(&buf[..len], from.ip()) {
                        discovery.record(&announcement);
                    }
                }
            });
        }
        discovery
    }

    /// The addresses it listens on (none: only configured addresses are
    /// used).
    pub fn local_addrs(&self) -> &[SocketAddr] {
        &self.local
    }

    /// Notes a device's address.
    pub fn record(&self, announcement: &Announcement) {
        let now = Instant::now();
        let moved = {
            let mut seen = self.seen.write().unwrap_or_else(PoisonError::into_inner);
            let previous = seen.insert(
                announcement.device_id.clone(),
                Seen {
                    ip: announcement.ip,
                    version: announcement.version,
                    at: now,
                },
            );
            previous.is_none_or(|before| before.ip != announcement.ip)
        };
        if moved {
            self.changed
                .send_modify(|count| *count = count.wrapping_add(1));
        }
    }

    /// Where `device_id` was last heard from, however long ago.
    pub fn lookup(&self, device_id: &str) -> Option<Seen> {
        self.seen
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(device_id)
            .copied()
    }

    /// Where `device_id` was heard from recently.
    pub fn fresh(&self, device_id: &str) -> Option<Seen> {
        self.lookup(device_id)
            .filter(|seen| seen.at.elapsed() <= FRESH_FOR)
    }

    /// Every device heard so far.
    pub fn all(&self) -> HashMap<String, Seen> {
        self.seen
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Waits up to `limit` for `device_id` to be heard (returns at once if
    /// it already was).
    pub async fn wait_for(&self, device_id: &str, limit: Duration) -> Option<Seen> {
        let mut changed = self.changed.subscribe();
        let deadline = Instant::now() + limit;
        loop {
            if let Some(seen) = self.fresh(device_id) {
                return Some(seen);
            }
            if tokio::time::timeout_at(deadline, changed.changed())
                .await
                .is_err()
            {
                return None;
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use serde_json::json;

    use super::*;
    use crate::protocol::cmd;

    const ID: &str = "bf00000000000000000000";

    /// A 3.5 broadcast as devices send it on port 6667.
    pub(crate) fn broadcast_v35(id: &str, ip: &str) -> Vec<u8> {
        let json = json!({ "ip": ip, "gwId": id, "active": 2, "ability": 0, "mode": 0,
                           "encrypt": true, "productKey": "keyxxxxxxxxxxxxx", "version": "3.5",
                           "token": true, "wf_cfg": true });
        let mut plain = 0u32.to_be_bytes().to_vec();
        plain.extend_from_slice(json.to_string().as_bytes());
        frame::encode_6699(0, cmd::UDP_NEW, &plain, &UDP_KEY, b"0123456789ab").unwrap()
    }

    /// A 3.3 broadcast (port 6667): JSON in AES-ECB.
    fn broadcast_v33(id: &str, ip: &str) -> Vec<u8> {
        let json = json!({ "ip": ip, "gwId": id, "active": 2, "encrypt": true, "version": "3.3" });
        let mut body = 0u32.to_be_bytes().to_vec();
        body.extend(crypto::ecb_encrypt(&UDP_KEY, json.to_string().as_bytes(), true).unwrap());
        frame::encode_55aa(0, cmd::UDP_NEW, &body, Check::Crc)
    }

    /// A 3.1 broadcast (port 6666): plain JSON.
    fn broadcast_v31(id: &str, ip: &str) -> Vec<u8> {
        let json = json!({ "ip": ip, "gwId": id, "version": "3.1" });
        let mut body = 0u32.to_be_bytes().to_vec();
        body.extend_from_slice(json.to_string().as_bytes());
        frame::encode_55aa(0, 0, &body, Check::Crc)
    }

    #[test]
    fn the_udp_key_is_the_md5_of_the_well_known_string() {
        use md5::{Digest, Md5};
        let digest: [u8; 16] = Md5::digest(b"yGAdlopoPVldABfn").into();
        assert_eq!(digest, UDP_KEY);
    }

    #[test]
    fn parses_every_broadcast_format() {
        let source = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 31));
        let cases = [
            (broadcast_v35(ID, "192.0.2.31"), Some(Version::V35)),
            (broadcast_v33(ID, "192.0.2.31"), Some(Version::V33)),
            (broadcast_v31(ID, "192.0.2.31"), None),
        ];
        for (datagram, version) in cases {
            let announcement = parse(&datagram, source).unwrap();
            assert_eq!(announcement.device_id, ID);
            assert_eq!(announcement.ip, Ipv4Addr::new(192, 0, 2, 31));
            assert_eq!(announcement.version, version);
        }
    }

    #[test]
    fn the_sender_address_wins_over_the_claimed_one() {
        let datagram = broadcast_v35(ID, "192.0.2.99");
        let from = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 31));
        assert_eq!(
            parse(&datagram, from).unwrap().ip,
            Ipv4Addr::new(192, 0, 2, 31)
        );
        let unknown = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        assert_eq!(
            parse(&datagram, unknown).unwrap().ip,
            Ipv4Addr::new(192, 0, 2, 99)
        );
    }

    #[test]
    fn ignores_noise_app_requests_and_tampering() {
        let source = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 31));
        assert_eq!(parse(b"hello", source), None);
        assert_eq!(parse(&[], source), None);
        let app = json!({ "from": "app", "ip": "192.0.2.5" });
        let request = frame::encode_6699(
            0,
            0x25,
            app.to_string().as_bytes(),
            &UDP_KEY,
            b"0123456789ab",
        )
        .unwrap();
        assert_eq!(parse(&request, source), None);
        let mut tampered = broadcast_v35(ID, "192.0.2.31");
        tampered[40] ^= 1;
        assert_eq!(parse(&tampered, source), None);
        let no_id = frame::encode_6699(
            0,
            cmd::UDP_NEW,
            br#"{"ip":"192.0.2.31"}"#,
            &UDP_KEY,
            b"0123456789ab",
        )
        .unwrap();
        assert_eq!(parse(&no_id, source), None);
    }

    #[test]
    fn announcements_never_print_the_device_id() {
        let announcement = parse(
            &broadcast_v35(ID, "192.0.2.31"),
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 31)),
        )
        .unwrap();
        assert!(!format!("{announcement:?}").contains(ID));
    }

    #[tokio::test]
    async fn listens_and_finds_a_device_by_id() {
        let discovery = Discovery::listen(&["127.0.0.1:0".parse().unwrap()]);
        let [port] = discovery.local_addrs() else {
            panic!("not listening");
        };
        let port = port.port();
        assert_eq!(discovery.fresh(ID), None);
        let waiting = {
            let discovery = Arc::clone(&discovery);
            tokio::spawn(async move { discovery.wait_for(ID, Duration::from_secs(5)).await })
        };
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        socket.send_to(b"noise", ("127.0.0.1", port)).await.unwrap();
        socket
            .send_to(&broadcast_v35(ID, "192.0.2.31"), ("127.0.0.1", port))
            .await
            .unwrap();
        let seen = waiting.await.unwrap().unwrap();
        // The sender's address, not the one in the broadcast.
        assert_eq!(seen.ip, Ipv4Addr::LOCALHOST);
        assert_eq!(seen.version, Some(Version::V35));
        assert_eq!(discovery.all().len(), 1);
    }

    #[tokio::test]
    async fn waiting_gives_up() {
        let discovery = Discovery::idle();
        assert!(discovery.local_addrs().is_empty());
        assert_eq!(
            discovery.wait_for(ID, Duration::from_millis(50)).await,
            None
        );
        discovery.record(&Announcement {
            device_id: ID.to_owned(),
            ip: Ipv4Addr::new(192, 0, 2, 40),
            version: None,
        });
        assert_eq!(
            discovery
                .wait_for(ID, Duration::from_millis(50))
                .await
                .map(|seen| seen.ip),
            Some(Ipv4Addr::new(192, 0, 2, 40))
        );
    }
}
