//! The primitives the Tuya local protocol requires, and nothing else:
//! AES-128-ECB with PKCS#7 padding (3.3, 3.4), AES-128-GCM (3.5),
//! HMAC-SHA256 (3.4 frames, session negotiation) and CRC-32 (3.3 frames).
//! All from RustCrypto; no home-made cryptography.
//!
//! ECB leaks patterns and has no integrity of its own. It is what the
//! devices speak, so it is used exactly where the protocol says and nowhere
//! else.

use aes::Aes128;
use aes::cipher::{Array, BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};
use aes_gcm::Aes128Gcm;
use aes_gcm::aead::{Aead, Payload};
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// AES-128 key length: Tuya local keys are 16 ASCII characters.
pub const KEY_LEN: usize = 16;
pub const BLOCK_LEN: usize = 16;
pub const GCM_NONCE_LEN: usize = 12;
pub const GCM_TAG_LEN: usize = 16;
pub const HMAC_LEN: usize = 32;

pub type Key = [u8; KEY_LEN];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CryptoError {
    #[error("the data is not a whole number of AES blocks")]
    BlockLength,
    #[error("the padding is not valid")]
    Padding,
    #[error("the message did not authenticate")]
    Authentication,
    #[error("the system random number generator failed")]
    Random,
}

fn aes(key: &Key) -> Aes128 {
    Aes128::new(&Array::from(*key))
}

/// Runs `f` on every whole block of `data`, in place.
fn each_block(data: &mut [u8], f: impl Fn(&mut Array<u8, aes::cipher::consts::U16>)) {
    for block in data.as_chunks_mut::<BLOCK_LEN>().0 {
        let mut array = Array::from(*block);
        f(&mut array);
        block.copy_from_slice(&array);
    }
}

/// AES-128-ECB. With `pad`, PKCS#7 padding is added (always at least one
/// byte, as the devices expect); without it the data must be whole blocks.
pub fn ecb_encrypt(key: &Key, data: &[u8], pad: bool) -> Result<Vec<u8>, CryptoError> {
    let mut out = data.to_vec();
    if pad {
        let fill = BLOCK_LEN - data.len() % BLOCK_LEN;
        // `fill` is 1..=16, so it always fits in a byte.
        out.extend(std::iter::repeat_n(fill as u8, fill));
    } else if !out.len().is_multiple_of(BLOCK_LEN) {
        return Err(CryptoError::BlockLength);
    }
    let cipher = aes(key);
    each_block(&mut out, |block| cipher.encrypt_block(block));
    Ok(out)
}

/// AES-128-ECB decryption. With `unpad`, the PKCS#7 padding is checked and
/// removed.
pub fn ecb_decrypt(key: &Key, data: &[u8], unpad: bool) -> Result<Vec<u8>, CryptoError> {
    if data.is_empty() || !data.len().is_multiple_of(BLOCK_LEN) {
        return Err(CryptoError::BlockLength);
    }
    let mut out = data.to_vec();
    let cipher = aes(key);
    each_block(&mut out, |block| cipher.decrypt_block(block));
    if unpad {
        let fill = usize::from(*out.last().ok_or(CryptoError::Padding)?);
        if fill == 0 || fill > BLOCK_LEN || fill > out.len() {
            return Err(CryptoError::Padding);
        }
        let (rest, padding) = out.split_at(out.len() - fill);
        if padding.iter().any(|&byte| usize::from(byte) != fill) {
            return Err(CryptoError::Padding);
        }
        out.truncate(rest.len());
    }
    Ok(out)
}

/// AES-128-GCM: returns `ciphertext ‖ tag`.
pub fn gcm_encrypt(
    key: &Key,
    nonce: &[u8; GCM_NONCE_LEN],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    Aes128Gcm::new(&Array::from(*key))
        .encrypt(
            &Array::from(*nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CryptoError::Authentication)
}

/// AES-128-GCM: `sealed` is `ciphertext ‖ tag`. Fails unless the tag
/// matches (the frame was not changed and the key is right).
pub fn gcm_decrypt(
    key: &Key,
    nonce: &[u8; GCM_NONCE_LEN],
    aad: &[u8],
    sealed: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    Aes128Gcm::new(&Array::from(*key))
        .decrypt(&Array::from(*nonce), Payload { msg: sealed, aad })
        .map_err(|_| CryptoError::Authentication)
}

/// HMAC-SHA256 over the concatenation of `parts`.
pub fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; HMAC_LEN] {
    // HMAC takes keys of any length, so this never fails; the fallback only
    // keeps the signature infallible.
    let Ok(mut mac) = <Hmac<Sha256> as KeyInit>::new_from_slice(key) else {
        return [0; HMAC_LEN];
    };
    for part in parts {
        mac.update(part);
    }
    mac.finalize().into_bytes().into()
}

/// Checks an HMAC-SHA256 in constant time.
pub fn hmac_verify(key: &[u8], data: &[u8], tag: &[u8]) -> bool {
    let Ok(mut mac) = <Hmac<Sha256> as KeyInit>::new_from_slice(key) else {
        return false;
    };
    mac.update(data);
    mac.verify_slice(tag).is_ok()
}

/// CRC-32 (IEEE), as used by 55AA frames without HMAC.
pub fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

/// Bytes from the operating system's random number generator.
pub fn random<const N: usize>() -> Result<[u8; N], CryptoError> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| CryptoError::Random)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &Key = b"0123456789abcdef";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn ecb_matches_the_fips_197_vector() {
        // FIPS-197 appendix C.1 (AES-128).
        let key: Key = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f,
        ];
        let plain = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        let sealed = ecb_encrypt(&key, &plain, false).unwrap();
        assert_eq!(hex(&sealed), "69c4e0d86a7b0430d8cdb78070b4c55a");
        assert_eq!(ecb_decrypt(&key, &sealed, false).unwrap(), plain);
    }

    #[test]
    fn ecb_pads_like_pkcs7() {
        for len in [0, 1, 15, 16, 17, 31, 32] {
            let data = vec![b'x'; len];
            let sealed = ecb_encrypt(KEY, &data, true).unwrap();
            assert_eq!(sealed.len(), (len / 16 + 1) * 16, "length {len}");
            assert_eq!(ecb_decrypt(KEY, &sealed, true).unwrap(), data);
        }
        assert_eq!(
            ecb_encrypt(KEY, &[1; 5], false),
            Err(CryptoError::BlockLength)
        );
        assert_eq!(
            ecb_decrypt(KEY, &[1; 5], true),
            Err(CryptoError::BlockLength)
        );
        assert_eq!(ecb_decrypt(KEY, &[], true), Err(CryptoError::BlockLength));
        // Unpadded garbage: the padding check catches it.
        let garbage = ecb_encrypt(KEY, &[0x41; 16], false).unwrap();
        assert_eq!(ecb_decrypt(KEY, &garbage, true), Err(CryptoError::Padding));
    }

    fn unhex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn gcm_matches_the_nist_test_case_3() {
        // McGrew & Viega, "The Galois/Counter Mode of Operation", test case 3
        // (AES-128, 96-bit IV, no associated data).
        let key: Key = unhex("feffe9928665731c6d6a8f9467308308")
            .try_into()
            .unwrap();
        let nonce: [u8; 12] = unhex("cafebabefacedbaddecaf888").try_into().unwrap();
        let plain = unhex(concat!(
            "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72",
            "1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b391aafd255"
        ));
        let sealed = gcm_encrypt(&key, &nonce, &[], &plain).unwrap();
        assert_eq!(sealed.len(), 64 + GCM_TAG_LEN);
        assert_eq!(hex(&sealed[64..]), "4d5c2af327cd64a62cf35abd2ba6fab4");
        assert_eq!(hex(&sealed[..16]), "42831ec2217774244b7221b784d0d49c");
        assert_eq!(gcm_decrypt(&key, &nonce, &[], &sealed).unwrap(), plain);

        // Any change, or other associated data, is refused.
        let mut tampered = sealed.clone();
        tampered[3] ^= 1;
        assert_eq!(
            gcm_decrypt(&key, &nonce, &[], &tampered),
            Err(CryptoError::Authentication)
        );
        assert_eq!(
            gcm_decrypt(&key, &nonce, b"header", &sealed),
            Err(CryptoError::Authentication)
        );
    }

    #[test]
    fn hmac_matches_rfc_4231_case_2() {
        let tag = hmac_sha256(b"Jefe", &[b"what do ya want ", b"for nothing?"]);
        assert_eq!(
            hex(&tag),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert!(hmac_verify(b"Jefe", b"what do ya want for nothing?", &tag));
        assert!(!hmac_verify(b"Jefe", b"what do ya want for nothing!", &tag));
        assert!(!hmac_verify(
            b"Jefe",
            b"what do ya want for nothing?",
            &tag[..31]
        ));
    }

    #[test]
    fn crc32_is_the_ieee_checksum() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn random_bytes_differ() {
        let a: [u8; 16] = random().unwrap();
        let b: [u8; 16] = random().unwrap();
        assert_ne!(a, b);
    }
}
