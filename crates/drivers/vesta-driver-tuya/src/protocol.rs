//! The Tuya local protocol, versions 3.3, 3.4 and 3.5: what goes inside a
//! frame, the session-key negotiation and the JSON requests. Pure code (no
//! I/O), so the client and the mocked device in the tests share it.
//!
//! | | 3.3 | 3.4 | 3.5 |
//! |---|---|---|---|
//! | Frame | 55AA + CRC-32 | 55AA + HMAC-SHA256 | 6699 (AES-GCM) |
//! | Body | AES-128-ECB, local key | AES-128-ECB, session key | AES-128-GCM, session key |
//! | Version header (`3.x` + 12 bytes) | before the ciphertext, clear | inside the ciphertext | inside the ciphertext |
//! | Session key | — (the local key) | negotiated | negotiated |
//! | Status query | `0x0a` `{gwId, devId, uid, t}` | `0x10` `{}` | `0x10` `{}` |
//! | Set data points | `0x07` `{devId, uid, t, dps}` | `0x0d` `{protocol: 5, t, data: {dps}}` | same as 3.4 |
//!
//! The version header goes on every command except queries, heartbeats and
//! the negotiation (`NO_HEADER`). Frames from a device carry a 4-byte return
//! code before the body (3.3, 3.4) or at the start of the plaintext (3.5).
//!
//! Negotiation (3.4, 3.5), all with the device's local key:
//!
//! 1. client → `0x03` with a random 16-byte nonce `a`;
//! 2. device → `0x04` with its nonce `b` and `HMAC(local_key, a)`, which the
//!    client checks (proves the device knows the key);
//! 3. client → `0x05` with `HMAC(local_key, b)`.
//!
//! The session key is `a XOR b` encrypted with the local key: AES-ECB (3.4)
//! or AES-GCM with `a[..12]` as nonce, ciphertext only (3.5).

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::crypto::{self, CryptoError, GCM_NONCE_LEN, HMAC_LEN, KEY_LEN, Key};
use crate::frame::{self, Check, FrameError, Framing, RawFrame, Skip};

/// Command codes (the `cmd` field of a frame). The whole set the driver
/// knows of, including some only devices send.
#[allow(dead_code)]
pub mod cmd {
    pub const SESS_KEY_NEG_START: u32 = 0x03;
    pub const SESS_KEY_NEG_RESP: u32 = 0x04;
    pub const SESS_KEY_NEG_FINISH: u32 = 0x05;
    /// Set data points (3.3).
    pub const CONTROL: u32 = 0x07;
    /// Data points the device reports by itself.
    pub const STATUS: u32 = 0x08;
    pub const HEART_BEAT: u32 = 0x09;
    /// Status query (3.3).
    pub const DP_QUERY: u32 = 0x0a;
    /// Set data points (3.4, 3.5; also the query of some 3.3 devices).
    pub const CONTROL_NEW: u32 = 0x0d;
    /// Status query (3.4, 3.5).
    pub const DP_QUERY_NEW: u32 = 0x10;
    pub const UPDATEDPS: u32 = 0x12;
    /// UDP broadcast of 3.3 / 3.5 devices.
    pub const UDP_NEW: u32 = 0x13;
    /// UDP broadcast of 3.4 devices.
    pub const BOARDCAST_LPV34: u32 = 0x23;
    pub const LAN_EXT_STREAM: u32 = 0x40;
}

/// Commands sent without the version header.
const NO_HEADER: [u32; 8] = [
    cmd::DP_QUERY,
    cmd::DP_QUERY_NEW,
    cmd::UPDATEDPS,
    cmd::HEART_BEAT,
    cmd::SESS_KEY_NEG_START,
    cmd::SESS_KEY_NEG_RESP,
    cmd::SESS_KEY_NEG_FINISH,
    cmd::LAN_EXT_STREAM,
];
const HEADER_LEN: usize = 15;
const RETCODE_LEN: usize = 4;
const NONCE_LEN: usize = 16;

/// Protocol version of a device (`protocol_version` in its config).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Version {
    #[serde(rename = "3.3")]
    V33,
    #[serde(rename = "3.4")]
    V34,
    #[serde(rename = "3.5")]
    V35,
}

impl Version {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "3.3" => Some(Self::V33),
            "3.4" => Some(Self::V34),
            "3.5" => Some(Self::V35),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V33 => "3.3",
            Self::V34 => "3.4",
            Self::V35 => "3.5",
        }
    }

    /// Whether a session starts with the key negotiation.
    pub const fn negotiates(self) -> bool {
        matches!(self, Self::V34 | Self::V35)
    }

    /// `3.x` followed by 12 zero bytes.
    fn header(self) -> [u8; HEADER_LEN] {
        let mut header = [0; HEADER_LEN];
        header[..3].copy_from_slice(self.as_str().as_bytes());
        header
    }

    fn strip_header(self, data: &[u8]) -> &[u8] {
        if data.len() >= HEADER_LEN && data.starts_with(self.as_str().as_bytes()) {
            &data[HEADER_LEN..]
        } else {
            data
        }
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who encodes: the client (VESTA) or a device (the mock in the tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Client,
    Device,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error("the device did not prove it knows the local key")]
    HandshakeRejected,
    #[error("unexpected answer {0:#04x} during the key negotiation")]
    HandshakeUnexpected(u32),
}

/// A message, with the version header and encryption removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub seq: u32,
    pub cmd: u32,
    /// Return code (messages from a device only). Non-zero means an error.
    pub retcode: Option<u32>,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn is_error(&self) -> bool {
        self.retcode.is_some_and(|code| code != 0)
    }
}

/// Encodes and decodes the messages of one session.
pub struct Codec {
    version: Version,
    side: Side,
    local_key: Key,
    /// The local key, or the session key once negotiated.
    key: Key,
    seq: u32,
}

impl std::fmt::Debug for Codec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Codec")
            .field("version", &self.version)
            .field("side", &self.side)
            .finish_non_exhaustive()
    }
}

/// Splits off a 4-byte return code.
fn split_retcode(data: &[u8]) -> (Option<u32>, &[u8]) {
    match data.split_first_chunk::<RETCODE_LEN>() {
        Some((code, rest)) => (Some(u32::from_be_bytes(*code)), rest),
        None => (None, data),
    }
}

impl Codec {
    pub fn new(version: Version, local_key: Key, side: Side) -> Self {
        Self {
            version,
            side,
            local_key,
            key: local_key,
            seq: 0,
        }
    }

    /// Uses the negotiated session key from now on.
    pub fn set_session_key(&mut self, key: Key) {
        self.key = key;
    }

    fn framing(&self) -> Framing<'_> {
        match self.version {
            Version::V33 => Framing::Frame55aa(Check::Crc),
            Version::V34 => Framing::Frame55aa(Check::Hmac(&self.key)),
            Version::V35 => Framing::Frame6699(&self.key),
        }
    }

    fn next_seq(&mut self) -> u32 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    /// Encodes `payload` as command `cmd` with the next sequence number.
    pub fn encode(&mut self, cmd: u32, payload: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        let seq = self.next_seq();
        self.encode_frame(seq, cmd, 0, payload)
    }

    /// Encodes with an explicit sequence number and (device side) return
    /// code.
    pub fn encode_frame(
        &mut self,
        seq: u32,
        cmd: u32,
        retcode: u32,
        payload: &[u8],
    ) -> Result<Vec<u8>, ProtocolError> {
        let with_header = !NO_HEADER.contains(&cmd);
        let header = self.version.header();
        let retcode = (self.side == Side::Device).then(|| retcode.to_be_bytes());
        match self.version {
            Version::V33 => {
                let mut body = Vec::new();
                body.extend(retcode.iter().flatten());
                if with_header {
                    body.extend_from_slice(&header);
                }
                body.extend(crypto::ecb_encrypt(&self.local_key, payload, true)?);
                Ok(frame::encode_55aa(seq, cmd, &body, Check::Crc))
            }
            Version::V34 => {
                let mut plain = Vec::new();
                if with_header {
                    plain.extend_from_slice(&header);
                }
                plain.extend_from_slice(payload);
                let mut body = Vec::new();
                body.extend(retcode.iter().flatten());
                body.extend(crypto::ecb_encrypt(&self.key, &plain, true)?);
                Ok(frame::encode_55aa(seq, cmd, &body, Check::Hmac(&self.key)))
            }
            Version::V35 => {
                let mut plain = Vec::new();
                plain.extend(retcode.iter().flatten());
                if with_header {
                    plain.extend_from_slice(&header);
                }
                plain.extend_from_slice(payload);
                let nonce: [u8; GCM_NONCE_LEN] = crypto::random()?;
                Ok(frame::encode_6699(seq, cmd, &plain, &self.key, &nonce)?)
            }
        }
    }

    /// Reads the first message in `buf`; see [`frame::decode`] for the
    /// meaning of the results.
    pub fn decode(&self, buf: &[u8]) -> Result<Option<(Message, usize)>, Skip> {
        let Some((raw, consumed)) = frame::decode(buf, self.framing())? else {
            return Ok(None);
        };
        match self.open(raw) {
            Ok(message) => Ok(Some((message, consumed))),
            Err(error) => Err(Skip {
                error: match error {
                    ProtocolError::Frame(error) => error,
                    ProtocolError::Crypto(error) => FrameError::Crypto(error),
                    _ => FrameError::BadCheck,
                },
                consumed,
            }),
        }
    }

    /// The plaintext of a checked frame.
    fn open(&self, raw: RawFrame) -> Result<Message, ProtocolError> {
        let from_device = self.side == Side::Client;
        let RawFrame { seq, cmd, body } = raw;
        let (retcode, payload) = match self.version {
            Version::V33 => {
                let (retcode, rest) = if from_device {
                    split_retcode(&body)
                } else {
                    (None, body.as_slice())
                };
                let rest = self.version.strip_header(rest);
                let payload = if rest.is_empty()
                    || rest.starts_with(b"{")
                    || !rest.len().is_multiple_of(16)
                {
                    // Empty acknowledgement, or plain text (some devices
                    // answer errors such as "data unvalid" unencrypted).
                    rest.to_vec()
                } else {
                    crypto::ecb_decrypt(&self.local_key, rest, true)?
                };
                (retcode, payload)
            }
            Version::V34 => {
                let (retcode, rest) = if from_device {
                    split_retcode(&body)
                } else {
                    (None, body.as_slice())
                };
                let payload = if rest.is_empty() || !rest.len().is_multiple_of(16) {
                    rest.to_vec()
                } else {
                    let plain = crypto::ecb_decrypt(&self.key, rest, true)?;
                    self.version.strip_header(&plain).to_vec()
                };
                (retcode, payload)
            }
            Version::V35 => {
                let header = self.version.as_str().as_bytes();
                let carries_code =
                    from_device && !body.starts_with(b"{") && !body.starts_with(header);
                let (retcode, rest) = if carries_code {
                    split_retcode(&body)
                } else {
                    (None, body.as_slice())
                };
                (retcode, self.version.strip_header(rest).to_vec())
            }
        };
        Ok(Message {
            seq,
            cmd,
            retcode,
            payload,
        })
    }

    /// Step 1 of the negotiation: a fresh nonce and the frame carrying it.
    pub fn begin_handshake(&mut self) -> Result<(Handshake, Vec<u8>), ProtocolError> {
        let nonce: [u8; NONCE_LEN] = crypto::random()?;
        self.key = self.local_key;
        let frame = self.encode(cmd::SESS_KEY_NEG_START, &nonce)?;
        Ok((Handshake { nonce }, frame))
    }

    /// Steps 2 and 3: checks the device's answer, returns the frame that
    /// finishes the negotiation and switches to the session key.
    pub fn complete_handshake(
        &mut self,
        handshake: Handshake,
        answer: &Message,
    ) -> Result<Vec<u8>, ProtocolError> {
        if answer.cmd != cmd::SESS_KEY_NEG_RESP {
            return Err(ProtocolError::HandshakeUnexpected(answer.cmd));
        }
        let payload = &answer.payload;
        if answer.is_error() || payload.len() < NONCE_LEN + HMAC_LEN {
            return Err(ProtocolError::HandshakeRejected);
        }
        let (remote, proof) = payload.split_at(NONCE_LEN);
        if !crypto::hmac_verify(&self.local_key, &handshake.nonce, &proof[..HMAC_LEN]) {
            return Err(ProtocolError::HandshakeRejected);
        }
        let mut remote_nonce = [0; NONCE_LEN];
        remote_nonce.copy_from_slice(remote);
        let ours = crypto::hmac_sha256(&self.local_key, &[&remote_nonce]);
        let finish = self.encode(cmd::SESS_KEY_NEG_FINISH, &ours)?;
        let key = session_key(
            self.version,
            &self.local_key,
            &handshake.nonce,
            &remote_nonce,
        )?;
        self.set_session_key(key);
        Ok(finish)
    }

    /// The local key (the mocked device needs it to answer a negotiation).
    #[cfg(test)]
    pub(crate) fn local_key(&self) -> &Key {
        &self.local_key
    }
}

/// A negotiation in progress: the nonce this side sent.
pub struct Handshake {
    nonce: [u8; NONCE_LEN],
}

impl Handshake {
    #[cfg(test)]
    pub(crate) fn nonce(&self) -> &[u8; NONCE_LEN] {
        &self.nonce
    }
}

/// The session key from both nonces (3.4: AES-ECB, 3.5: AES-GCM with the
/// first 12 bytes of the client's nonce, ciphertext only).
pub fn session_key(
    version: Version,
    local_key: &Key,
    client_nonce: &[u8; NONCE_LEN],
    device_nonce: &[u8; NONCE_LEN],
) -> Result<Key, ProtocolError> {
    let mut mixed = [0; KEY_LEN];
    for (out, (a, b)) in mixed
        .iter_mut()
        .zip(client_nonce.iter().zip(device_nonce.iter()))
    {
        *out = a ^ b;
    }
    let sealed = match version {
        Version::V33 => return Ok(*local_key),
        Version::V34 => crypto::ecb_encrypt(local_key, &mixed, false)?,
        Version::V35 => {
            let mut nonce = [0; GCM_NONCE_LEN];
            nonce.copy_from_slice(&client_nonce[..GCM_NONCE_LEN]);
            crypto::gcm_encrypt(local_key, &nonce, &[], &mixed)?
        }
    };
    let mut key = [0; KEY_LEN];
    key.copy_from_slice(&sealed[..KEY_LEN]);
    Ok(key)
}

/// A request: command code and JSON payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub cmd: u32,
    pub payload: Vec<u8>,
}

fn request(cmd: u32, value: &Value) -> Request {
    Request {
        cmd,
        payload: value.to_string().into_bytes(),
    }
}

/// Asks for every data point.
pub fn status_query(version: Version, device_id: &str, now: u64) -> Request {
    match version {
        Version::V33 => request(
            cmd::DP_QUERY,
            &json!({ "gwId": device_id, "devId": device_id, "uid": device_id, "t": now.to_string() }),
        ),
        Version::V34 | Version::V35 => request(cmd::DP_QUERY_NEW, &json!({})),
    }
}

/// Status query of 3.3 devices that refuse the usual one ("data unvalid"):
/// the data points to report, each `null`. Nothing is set.
pub fn status_query_listing(device_id: &str, dps: &[String], now: u64) -> Request {
    let listed: Map<String, Value> = dps.iter().map(|dp| (dp.clone(), Value::Null)).collect();
    request(
        cmd::CONTROL_NEW,
        &json!({ "devId": device_id, "uid": device_id, "t": now.to_string(), "dps": listed }),
    )
}

/// Sets data points.
pub fn control(version: Version, device_id: &str, dps: &Map<String, Value>, now: u64) -> Request {
    match version {
        Version::V33 => request(
            cmd::CONTROL,
            &json!({ "devId": device_id, "uid": device_id, "t": now.to_string(), "dps": dps }),
        ),
        Version::V34 | Version::V35 => request(
            cmd::CONTROL_NEW,
            &json!({ "protocol": 5, "t": now, "data": { "dps": dps } }),
        ),
    }
}

pub fn heartbeat(device_id: &str) -> Request {
    request(
        cmd::HEART_BEAT,
        &json!({ "gwId": device_id, "devId": device_id }),
    )
}

fn json_object(payload: &[u8]) -> Option<Map<String, Value>> {
    match serde_json::from_slice(payload).ok()? {
        Value::Object(object) => Some(object),
        _ => None,
    }
}

/// The data points in a status message: `{"dps": …}` (3.3) or
/// `{"data": {"dps": …}}` (3.4, 3.5).
pub fn data_points(payload: &[u8]) -> Option<Map<String, Value>> {
    let object = json_object(payload)?;
    let dps = object
        .get("dps")
        .or_else(|| object.get("data").and_then(|data| data.get("dps")))?;
    dps.as_object().cloned()
}

/// The device ID a message names, if any (`devId` or `gwId`).
pub fn device_id(payload: &[u8]) -> Option<String> {
    let object = json_object(payload)?;
    ["devId", "gwId"]
        .iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str).map(str::to_owned))
}

/// Some 3.3 devices answer the usual status query with this error.
pub fn refuses_plain_query(payload: &[u8]) -> bool {
    payload
        .windows(b"data unvalid".len())
        .any(|window| window == b"data unvalid")
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &Key = b"0123456789abcdef";

    fn pair(version: Version) -> (Codec, Codec) {
        (
            Codec::new(version, *KEY, Side::Client),
            Codec::new(version, *KEY, Side::Device),
        )
    }

    #[test]
    fn versions() {
        assert_eq!(Version::parse("3.5"), Some(Version::V35));
        assert_eq!(Version::parse(" 3.3 "), Some(Version::V33));
        assert_eq!(Version::parse("3.1"), None);
        assert_eq!(Version::V34.header()[..3], *b"3.4");
        assert_eq!(Version::V34.header()[3..], [0; 12]);
        assert!(!Version::V33.negotiates());
        assert!(Version::V35.negotiates());
    }

    #[test]
    fn v33_query_is_ecb_without_header_and_control_has_a_clear_header() {
        let (mut client, device) = pair(Version::V33);
        let query = status_query(Version::V33, "bf00000000000000000000", 1_700_000_000);
        assert_eq!(query.cmd, cmd::DP_QUERY);
        let frame = client.encode(query.cmd, &query.payload).unwrap();
        // 55AA, seq 1, no header: the body is the ECB ciphertext.
        let body = &frame[16..frame.len() - 8];
        assert_eq!(
            body,
            crypto::ecb_encrypt(KEY, &query.payload, true).unwrap()
        );
        let (message, _) = device.decode(&frame).unwrap().unwrap();
        assert_eq!(message.seq, 1);
        assert_eq!(message.retcode, None);
        assert_eq!(
            serde_json::from_slice::<Value>(&message.payload).unwrap(),
            json!({ "gwId": "bf00000000000000000000", "devId": "bf00000000000000000000",
                    "uid": "bf00000000000000000000", "t": "1700000000" })
        );

        let mut dps = Map::new();
        dps.insert("1".to_owned(), json!(true));
        let set = control(Version::V33, "bf00000000000000000000", &dps, 1_700_000_000);
        assert_eq!(set.cmd, cmd::CONTROL);
        let frame = client.encode(set.cmd, &set.payload).unwrap();
        assert_eq!(&frame[4..8], &2u32.to_be_bytes());
        assert_eq!(&frame[16..19], b"3.3");
        assert_eq!(&frame[19..31], &[0; 12]);
        let (message, _) = device.decode(&frame).unwrap().unwrap();
        assert_eq!(
            data_points(&message.payload),
            Some(dps),
            "the device reads the dps back"
        );
    }

    #[test]
    fn device_messages_carry_a_return_code() {
        for version in [Version::V33, Version::V34, Version::V35] {
            let (client, mut device) = pair(version);
            let push = device
                .encode_frame(0, cmd::STATUS, 0, br#"{"dps":{"1":false}}"#)
                .unwrap();
            let (message, _) = client.decode(&push).unwrap().unwrap();
            assert_eq!(message.retcode, Some(0), "{version}");
            assert_eq!(message.payload, br#"{"dps":{"1":false}}"#, "{version}");

            let refused = device.encode_frame(5, cmd::CONTROL_NEW, 1, b"").unwrap();
            let (message, _) = client.decode(&refused).unwrap().unwrap();
            assert!(message.is_error(), "{version}");
            assert!(message.payload.is_empty(), "{version}");
        }
    }

    #[test]
    fn v33_plain_text_errors_are_read_as_is() {
        let (client, _) = pair(Version::V33);
        let mut body = 1u32.to_be_bytes().to_vec();
        body.extend_from_slice(b"json obj data unvalid");
        let frame = frame::encode_55aa(1, cmd::DP_QUERY, &body, Check::Crc);
        let (message, _) = client.decode(&frame).unwrap().unwrap();
        assert!(refuses_plain_query(&message.payload));
        assert_eq!(message.retcode, Some(1));
    }

    #[test]
    fn v34_encrypts_the_header_and_signs_with_hmac() {
        let (mut client, device) = pair(Version::V34);
        let set = control(
            Version::V34,
            "bf00000000000000000000",
            &Map::new(),
            1_700_000_000,
        );
        assert_eq!(set.cmd, cmd::CONTROL_NEW);
        let frame = client.encode(set.cmd, &set.payload).unwrap();
        let body = &frame[16..frame.len() - 36];
        let plain = crypto::ecb_decrypt(KEY, body, true).unwrap();
        assert_eq!(&plain[..3], b"3.4");
        assert_eq!(
            serde_json::from_slice::<Value>(&plain[15..]).unwrap(),
            json!({ "protocol": 5, "t": 1_700_000_000, "data": { "dps": {} } })
        );
        assert!(crypto::hmac_verify(
            KEY,
            &frame[..frame.len() - 36],
            &frame[frame.len() - 36..frame.len() - 4]
        ));
        assert!(device.decode(&frame).unwrap().is_some());
    }

    #[test]
    fn v35_puts_everything_in_the_gcm_plaintext() {
        let (mut client, device) = pair(Version::V35);
        let query = status_query(Version::V35, "bf00000000000000000000", 0);
        assert_eq!(query, request(cmd::DP_QUERY_NEW, &json!({})));
        let frame = client.encode(query.cmd, &query.payload).unwrap();
        assert_eq!(&frame[..4], &frame::PREFIX_6699);
        let (message, _) = device.decode(&frame).unwrap().unwrap();
        assert_eq!(message.payload, b"{}");

        let set = control(Version::V35, "bf00000000000000000000", &Map::new(), 0);
        let frame = client.encode(set.cmd, &set.payload).unwrap();
        let (raw, _) = frame::decode(&frame, Framing::Frame6699(KEY))
            .unwrap()
            .unwrap();
        assert_eq!(&raw.body[..3], b"3.5");
        assert_eq!(&raw.body[3..15], &[0; 12]);
        // Two frames never share a nonce.
        let again = client.encode(set.cmd, &set.payload).unwrap();
        assert_ne!(frame[18..30], again[18..30]);
    }

    /// The device side of a negotiation, as the mock in the tests does it.
    fn answer(device: &mut Codec, request: &Message, device_nonce: [u8; 16]) -> Vec<u8> {
        let proof = crypto::hmac_sha256(device.local_key(), &[&request.payload]);
        let mut payload = device_nonce.to_vec();
        payload.extend_from_slice(&proof);
        device
            .encode_frame(request.seq, cmd::SESS_KEY_NEG_RESP, 0, &payload)
            .unwrap()
    }

    #[test]
    fn negotiation_agrees_on_a_session_key() {
        for version in [Version::V34, Version::V35] {
            let (mut client, mut device) = pair(version);
            let (handshake, start) = client.begin_handshake().unwrap();
            let (request, _) = device.decode(&start).unwrap().unwrap();
            assert_eq!(request.cmd, cmd::SESS_KEY_NEG_START);
            assert_eq!(&request.payload[..], handshake.nonce());

            let device_nonce = *b"fedcba9876543210";
            let reply = answer(&mut device, &request, device_nonce);
            let (reply, _) = client.decode(&reply).unwrap().unwrap();
            let finish = client.complete_handshake(handshake, &reply).unwrap();

            // The finish frame is still under the local key.
            let (finish, _) = device.decode(&finish).unwrap().unwrap();
            assert_eq!(finish.cmd, cmd::SESS_KEY_NEG_FINISH);
            assert_eq!(
                finish.payload,
                crypto::hmac_sha256(KEY, &[&device_nonce]).to_vec()
            );
            let mut client_nonce = [0; 16];
            client_nonce.copy_from_slice(&request.payload);
            let key = session_key(version, KEY, &client_nonce, &device_nonce).unwrap();
            assert_ne!(&key, KEY);
            device.set_session_key(key);

            // Both sides now speak with the session key.
            let frame = client.encode(cmd::DP_QUERY_NEW, b"{}").unwrap();
            assert!(device.decode(&frame).unwrap().is_some(), "{version}");
            let stale = Codec::new(version, *KEY, Side::Device);
            assert!(stale.decode(&frame).is_err(), "{version}");
        }
    }

    #[test]
    fn session_key_derivation_matches_the_spec() {
        let a = *b"0123456789abcdef";
        let b = *b"ABCDEFGHIJKLMNOP";
        let mut mixed = [0u8; 16];
        for i in 0..16 {
            mixed[i] = a[i] ^ b[i];
        }
        let v34 = session_key(Version::V34, KEY, &a, &b).unwrap();
        assert_eq!(
            v34.to_vec(),
            crypto::ecb_encrypt(KEY, &mixed, false).unwrap()
        );
        let v35 = session_key(Version::V35, KEY, &a, &b).unwrap();
        let nonce: [u8; 12] = a[..12].try_into().unwrap();
        assert_eq!(
            v35.to_vec(),
            crypto::gcm_encrypt(KEY, &nonce, &[], &mixed).unwrap()[..16].to_vec()
        );
    }

    #[test]
    fn a_device_without_the_key_is_refused() {
        for version in [Version::V34, Version::V35] {
            let mut client = Codec::new(version, *KEY, Side::Client);
            let mut impostor = Codec::new(version, *b"fedcba9876543210", Side::Device);
            let (handshake, _) = client.begin_handshake().unwrap();
            // The impostor proves knowledge of the wrong key; it cannot even
            // produce a frame the client accepts with 3.4/3.5 framing.
            let fake = Message {
                seq: 1,
                cmd: cmd::SESS_KEY_NEG_RESP,
                retcode: Some(0),
                payload: [
                    [7u8; 16].as_slice(),
                    &crypto::hmac_sha256(b"wrong", &[b"x"]),
                ]
                .concat(),
            };
            assert_eq!(
                client.complete_handshake(handshake, &fake),
                Err(ProtocolError::HandshakeRejected)
            );
            let frame = impostor
                .encode_frame(1, cmd::SESS_KEY_NEG_RESP, 0, &[0; 48])
                .unwrap();
            assert!(client.decode(&frame).is_err(), "{version}");
        }
    }

    #[test]
    fn data_points_in_both_layouts() {
        let v33 = br#"{"devId":"bf00000000000000000000","dps":{"1":true,"22":125},"t":1}"#;
        let v35 = br#"{"protocol":4,"t":1,"data":{"dps":{"1":false}}}"#;
        assert_eq!(data_points(v33).unwrap().get("22"), Some(&json!(125)));
        assert_eq!(data_points(v35).unwrap().get("1"), Some(&json!(false)));
        assert_eq!(data_points(b"{}"), None);
        assert_eq!(data_points(b"not json"), None);
        assert_eq!(device_id(v33).as_deref(), Some("bf00000000000000000000"));
        assert_eq!(device_id(v35), None);
    }

    #[test]
    fn listing_query_sets_nothing() {
        let query = status_query_listing(
            "bf00000000000000000000",
            &["1".to_owned(), "20".to_owned()],
            5,
        );
        assert_eq!(query.cmd, cmd::CONTROL_NEW);
        let value: Value = serde_json::from_slice(&query.payload).unwrap();
        assert_eq!(value["dps"], json!({ "1": null, "20": null }));
    }
}
