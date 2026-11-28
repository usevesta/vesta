//! Tuya frames on the wire. Two layouts:
//!
//! **55AA** (protocol 3.1–3.4), all integers big-endian:
//!
//! ```text
//! 00 00 55 AA | seq u32 | cmd u32 | len u32 | body … | check | 00 00 AA 55
//! ```
//!
//! `len` counts everything after itself: body, check and suffix. `check` is
//! a CRC-32 of everything before it (3.1–3.3) or an HMAC-SHA256 of the same
//! bytes (3.4). Frames from a device start the body with a 4-byte return
//! code.
//!
//! **6699** (protocol 3.5):
//!
//! ```text
//! 00 00 66 99 | 00 00 | seq u32 | cmd u32 | len u32 | nonce (12) | ciphertext … | tag (16) | 00 00 99 66
//! ```
//!
//! `len` counts nonce, ciphertext and tag (not the suffix). The body is
//! AES-128-GCM encrypted with the 14 header bytes after the prefix as
//! associated data. Frames from a device start the plaintext with a 4-byte
//! return code.
//!
//! This module only frames and checks; what goes in the body is
//! `protocol.rs`.

use crate::crypto::{self, CryptoError, GCM_NONCE_LEN, GCM_TAG_LEN, HMAC_LEN, Key};

pub const PREFIX_55AA: [u8; 4] = [0x00, 0x00, 0x55, 0xaa];
pub const SUFFIX_55AA: [u8; 4] = [0x00, 0x00, 0xaa, 0x55];
pub const PREFIX_6699: [u8; 4] = [0x00, 0x00, 0x66, 0x99];
pub const SUFFIX_6699: [u8; 4] = [0x00, 0x00, 0x99, 0x66];
const HEADER_55AA: usize = 16;
const HEADER_6699: usize = 18;
/// Larger frames are not Tuya status messages; the stream is resynced.
pub const MAX_FRAME_LEN: usize = 16 * 1024;

/// How a 55AA frame is checked.
#[derive(Clone, Copy)]
pub enum Check<'a> {
    /// CRC-32 (3.1–3.3, UDP broadcasts).
    Crc,
    /// HMAC-SHA256 with this key (3.4).
    Hmac(&'a [u8]),
}

/// Which framing a session uses, with its key.
#[derive(Clone, Copy)]
pub enum Framing<'a> {
    Frame55aa(Check<'a>),
    /// GCM key (3.5).
    Frame6699(&'a Key),
}

/// One frame, checked (and for 6699 decrypted). `body` still holds the
/// return code, if the sender adds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrame {
    pub seq: u32,
    pub cmd: u32,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("frame of {0} bytes is too large")]
    TooLarge(usize),
    #[error("frame length field is inconsistent")]
    BadLength,
    #[error("frame suffix is missing")]
    BadSuffix,
    #[error("bytes that are not a frame")]
    Noise,
    #[error("frame checksum does not match")]
    BadCheck,
    #[error("6699 frame used where 55AA was expected, or the other way round")]
    WrongFraming,
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}

/// A frame that could not be read, and how many bytes to drop to get past
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skip {
    pub error: FrameError,
    pub consumed: usize,
}

fn be32(bytes: &[u8], at: usize) -> u32 {
    bytes
        .get(at..at + 4)
        .and_then(|slice| <[u8; 4]>::try_from(slice).ok())
        .map_or(0, u32::from_be_bytes)
}

fn len_u32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// Builds a 55AA frame around `body` (already encrypted as the protocol
/// version requires).
pub fn encode_55aa(seq: u32, cmd: u32, body: &[u8], check: Check<'_>) -> Vec<u8> {
    let check_len = match check {
        Check::Crc => 4,
        Check::Hmac(_) => HMAC_LEN,
    };
    let len = body.len() + check_len + SUFFIX_55AA.len();
    let mut frame = Vec::with_capacity(HEADER_55AA + len);
    frame.extend_from_slice(&PREFIX_55AA);
    frame.extend_from_slice(&seq.to_be_bytes());
    frame.extend_from_slice(&cmd.to_be_bytes());
    frame.extend_from_slice(&len_u32(len).to_be_bytes());
    frame.extend_from_slice(body);
    match check {
        Check::Crc => {
            let crc = crypto::crc32(&frame);
            frame.extend_from_slice(&crc.to_be_bytes());
        }
        Check::Hmac(key) => {
            let tag = crypto::hmac_sha256(key, &[&frame]);
            frame.extend_from_slice(&tag);
        }
    }
    frame.extend_from_slice(&SUFFIX_55AA);
    frame
}

/// Builds a 6699 frame: `plaintext` is encrypted with AES-128-GCM under
/// `key` and `nonce` (which must never repeat for a key).
pub fn encode_6699(
    seq: u32,
    cmd: u32,
    plaintext: &[u8],
    key: &Key,
    nonce: &[u8; GCM_NONCE_LEN],
) -> Result<Vec<u8>, FrameError> {
    let len = GCM_NONCE_LEN + plaintext.len() + GCM_TAG_LEN;
    let mut frame = Vec::with_capacity(HEADER_6699 + len + SUFFIX_6699.len());
    frame.extend_from_slice(&PREFIX_6699);
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&seq.to_be_bytes());
    frame.extend_from_slice(&cmd.to_be_bytes());
    frame.extend_from_slice(&len_u32(len).to_be_bytes());
    let sealed = crypto::gcm_encrypt(key, nonce, &frame[PREFIX_6699.len()..], plaintext)?;
    frame.extend_from_slice(nonce);
    frame.extend_from_slice(&sealed);
    frame.extend_from_slice(&SUFFIX_6699);
    Ok(frame)
}

/// Where the next frame starts in `buf` (garbage before it is skipped).
fn find_prefix(buf: &[u8], prefix: &[u8; 4]) -> Option<usize> {
    buf.windows(prefix.len())
        .position(|window| window == prefix)
}

/// Reads the first frame in `buf`.
///
/// - `Ok(None)`: not a whole frame yet; read more.
/// - `Ok(Some((frame, consumed)))`: one frame; drop `consumed` bytes.
/// - `Err(skip)`: a broken frame (or noise); drop `skip.consumed` bytes and
///   go on.
pub fn decode(buf: &[u8], framing: Framing<'_>) -> Result<Option<(RawFrame, usize)>, Skip> {
    let (prefix, other) = match framing {
        Framing::Frame55aa(_) => (&PREFIX_55AA, &PREFIX_6699),
        Framing::Frame6699(_) => (&PREFIX_6699, &PREFIX_55AA),
    };
    let Some(start) = find_prefix(buf, prefix) else {
        if find_prefix(buf, other).is_some() {
            return Err(Skip {
                error: FrameError::WrongFraming,
                consumed: buf.len(),
            });
        }
        // Keep the last bytes: they may be the start of a prefix.
        let keep = buf.len().min(prefix.len() - 1);
        return match buf.len() - keep {
            0 => Ok(None),
            consumed => Err(Skip {
                error: FrameError::Noise,
                consumed,
            }),
        };
    };
    let frame = &buf[start..];
    let skip = |error: FrameError, consumed: usize| Skip {
        error,
        consumed: start + consumed,
    };
    match framing {
        Framing::Frame55aa(check) => {
            if frame.len() < HEADER_55AA {
                return Ok(None);
            }
            let len = be32(frame, 12) as usize;
            let check_len = match check {
                Check::Crc => 4,
                Check::Hmac(_) => HMAC_LEN,
            };
            if len > MAX_FRAME_LEN {
                return Err(skip(FrameError::TooLarge(len), prefix.len()));
            }
            if len < check_len + SUFFIX_55AA.len() {
                return Err(skip(FrameError::BadLength, prefix.len()));
            }
            let total = HEADER_55AA + len;
            if frame.len() < total {
                return Ok(None);
            }
            if frame[total - 4..total] != SUFFIX_55AA {
                return Err(skip(FrameError::BadSuffix, prefix.len()));
            }
            let checked = &frame[..total - 4 - check_len];
            let received = &frame[total - 4 - check_len..total - 4];
            let good = match check {
                Check::Crc => crypto::crc32(checked).to_be_bytes() == received,
                Check::Hmac(key) => crypto::hmac_verify(key, checked, received),
            };
            if !good {
                return Err(skip(FrameError::BadCheck, total));
            }
            Ok(Some((
                RawFrame {
                    seq: be32(frame, 4),
                    cmd: be32(frame, 8),
                    body: frame[HEADER_55AA..total - 4 - check_len].to_vec(),
                },
                start + total,
            )))
        }
        Framing::Frame6699(key) => {
            if frame.len() < HEADER_6699 {
                return Ok(None);
            }
            let len = be32(frame, 14) as usize;
            if len > MAX_FRAME_LEN {
                return Err(skip(FrameError::TooLarge(len), prefix.len()));
            }
            if len < GCM_NONCE_LEN + GCM_TAG_LEN {
                return Err(skip(FrameError::BadLength, prefix.len()));
            }
            let total = HEADER_6699 + len + SUFFIX_6699.len();
            if frame.len() < total {
                return Ok(None);
            }
            if frame[total - 4..total] != SUFFIX_6699 {
                return Err(skip(FrameError::BadSuffix, prefix.len()));
            }
            let mut nonce = [0; GCM_NONCE_LEN];
            nonce.copy_from_slice(&frame[HEADER_6699..HEADER_6699 + GCM_NONCE_LEN]);
            let sealed = &frame[HEADER_6699 + GCM_NONCE_LEN..total - 4];
            let aad = &frame[PREFIX_6699.len()..HEADER_6699];
            let body = crypto::gcm_decrypt(key, &nonce, aad, sealed)
                .map_err(|error| skip(error.into(), total))?;
            Ok(Some((
                RawFrame {
                    seq: be32(frame, 6),
                    cmd: be32(frame, 10),
                    body,
                },
                start + total,
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &Key = b"0123456789abcdef";

    #[test]
    fn crc_frame_layout() {
        let frame = encode_55aa(1, 0x0a, b"abc", Check::Crc);
        // prefix, seq, cmd, len = 3 + 4 + 4
        assert_eq!(&frame[..4], &PREFIX_55AA);
        assert_eq!(&frame[4..8], &1u32.to_be_bytes());
        assert_eq!(&frame[8..12], &0x0au32.to_be_bytes());
        assert_eq!(&frame[12..16], &11u32.to_be_bytes());
        assert_eq!(&frame[16..19], b"abc");
        assert_eq!(&frame[19..23], &crypto::crc32(&frame[..19]).to_be_bytes());
        assert_eq!(&frame[23..], &SUFFIX_55AA);

        let (decoded, used) = decode(&frame, Framing::Frame55aa(Check::Crc))
            .unwrap()
            .unwrap();
        assert_eq!(used, frame.len());
        assert_eq!(
            decoded,
            RawFrame {
                seq: 1,
                cmd: 0x0a,
                body: b"abc".to_vec()
            }
        );
    }

    #[test]
    fn hmac_frames_need_the_right_key() {
        let frame = encode_55aa(7, 0x10, b"payload", Check::Hmac(KEY));
        assert_eq!(frame.len(), 16 + 7 + 32 + 4);
        assert!(
            decode(&frame, Framing::Frame55aa(Check::Hmac(KEY)))
                .unwrap()
                .is_some()
        );
        let wrong = decode(&frame, Framing::Frame55aa(Check::Hmac(b"fedcba9876543210")));
        assert_eq!(
            wrong,
            Err(Skip {
                error: FrameError::BadCheck,
                consumed: frame.len()
            })
        );
    }

    #[test]
    fn gcm_frame_layout_and_associated_data() {
        let nonce = *b"0123456789ab";
        let frame = encode_6699(3, 0x08, b"{\"dps\":{}}", KEY, &nonce).unwrap();
        assert_eq!(&frame[..4], &PREFIX_6699);
        assert_eq!(&frame[4..6], &[0, 0]);
        assert_eq!(&frame[6..10], &3u32.to_be_bytes());
        assert_eq!(&frame[10..14], &8u32.to_be_bytes());
        assert_eq!(&frame[14..18], &(12u32 + 10 + 16).to_be_bytes());
        assert_eq!(&frame[18..30], &nonce);
        assert_eq!(&frame[frame.len() - 4..], &SUFFIX_6699);
        // The header is authenticated: a different command is refused.
        let mut tampered = frame.clone();
        tampered[13] = 0x07;
        assert!(matches!(
            decode(&tampered, Framing::Frame6699(KEY)),
            Err(Skip {
                error: FrameError::Crypto(CryptoError::Authentication),
                ..
            })
        ));
        let (decoded, used) = decode(&frame, Framing::Frame6699(KEY)).unwrap().unwrap();
        assert_eq!(used, frame.len());
        assert_eq!(decoded.body, b"{\"dps\":{}}");
        assert_eq!((decoded.seq, decoded.cmd), (3, 8));
    }

    #[test]
    fn partial_frames_wait_and_noise_is_skipped() {
        let frame = encode_55aa(1, 9, b"", Check::Crc);
        for cut in 0..frame.len() {
            assert_eq!(
                decode(&frame[..cut], Framing::Frame55aa(Check::Crc)),
                Ok(None),
                "cut at {cut}"
            );
        }
        let mut noisy = b"noise".to_vec();
        noisy.extend_from_slice(&frame);
        noisy.extend_from_slice(&frame);
        let (_, used) = decode(&noisy, Framing::Frame55aa(Check::Crc))
            .unwrap()
            .unwrap();
        assert_eq!(used, 5 + frame.len());
        let (_, again) = decode(&noisy[used..], Framing::Frame55aa(Check::Crc))
            .unwrap()
            .unwrap();
        assert_eq!(again, frame.len());

        // Noise without any prefix is dropped (but a possible prefix start kept).
        assert_eq!(
            decode(b"garbage\x00\x00", Framing::Frame55aa(Check::Crc)),
            Err(Skip {
                error: FrameError::Noise,
                consumed: 6
            })
        );
    }

    #[test]
    fn broken_frames_are_skipped() {
        let mut frame = encode_55aa(1, 9, b"abc", Check::Crc);
        let last = frame.len() - 6;
        frame[last] ^= 0xff;
        assert!(matches!(
            decode(&frame, Framing::Frame55aa(Check::Crc)),
            Err(Skip {
                error: FrameError::BadCheck,
                ..
            })
        ));
        let mut huge = encode_55aa(1, 9, b"", Check::Crc);
        huge[12..16].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            decode(&huge, Framing::Frame55aa(Check::Crc)),
            Err(Skip {
                error: FrameError::TooLarge(_),
                consumed: 4
            })
        ));
        let gcm = encode_6699(1, 9, b"x", KEY, b"0123456789ab").unwrap();
        assert!(matches!(
            decode(&gcm, Framing::Frame55aa(Check::Crc)),
            Err(Skip {
                error: FrameError::WrongFraming,
                ..
            })
        ));
    }
}
