//! `Hash` operator: SHA-256 / SHA-512, optional HMAC keying.

use std::fmt::Write;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};

use super::HashAlgorithm;

pub(crate) fn apply(candidate: &str, algorithm: HashAlgorithm, hash_key: Option<&[u8]>) -> String {
    let bytes = candidate.as_bytes();
    match (algorithm, hash_key) {
        (HashAlgorithm::Sha256, None) => to_hex(Sha256::digest(bytes).as_slice()),
        (HashAlgorithm::Sha512, None) => to_hex(Sha512::digest(bytes).as_slice()),
        (HashAlgorithm::Sha256, Some(key)) => {
            let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts variable-length keys");
            mac.update(bytes);
            to_hex(mac.finalize().into_bytes().as_slice())
        }
        (HashAlgorithm::Sha512, Some(key)) => {
            let mut mac = Hmac::<Sha512>::new_from_slice(key).expect("HMAC accepts variable-length keys");
            mac.update(bytes);
            to_hex(mac.finalize().into_bytes().as_slice())
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(out, "{b:02x}").expect("writing to a String is infallible");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{HashAlgorithm, apply};

    #[test]
    fn sha256_known_answer() {
        let out = apply("abc", HashAlgorithm::Sha256, None);
        assert_eq!(out, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn sha512_known_answer() {
        let out = apply("abc", HashAlgorithm::Sha512, None);
        assert_eq!(
            out,
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn hmac_sha256_rfc4231_test1() {
        let key = vec![0x0bu8; 20];
        let out = apply("Hi There", HashAlgorithm::Sha256, Some(&key));
        assert_eq!(out, "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
    }

    #[test]
    fn hmac_changes_with_key() {
        let a = apply("hello", HashAlgorithm::Sha256, Some(b"k1"));
        let b = apply("hello", HashAlgorithm::Sha256, Some(b"k2"));
        let bare = apply("hello", HashAlgorithm::Sha256, None);
        assert_ne!(a, b);
        assert_ne!(a, bare);
    }

    #[test]
    fn deterministic() {
        let a = apply("hello", HashAlgorithm::Sha256, None);
        let b = apply("hello", HashAlgorithm::Sha256, None);
        assert_eq!(a, b);
    }
}
