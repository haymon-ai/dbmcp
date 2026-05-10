//! `Hash` operator: SHA-256 / SHA-512 bare digest.

use sha2::{Digest, Sha256, Sha512};

use super::HashAlgorithm;

pub(crate) fn apply(candidate: &str, algorithm: HashAlgorithm) -> String {
    let bytes = candidate.as_bytes();
    match algorithm {
        HashAlgorithm::Sha256 => to_hex(Sha256::digest(bytes).as_slice()),
        HashAlgorithm::Sha512 => to_hex(Sha512::digest(bytes).as_slice()),
    }
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = vec![0u8; bytes.len() * 2];
    for (i, &b) in bytes.iter().enumerate() {
        out[i * 2] = HEX[(b >> 4) as usize];
        out[i * 2 + 1] = HEX[(b & 0x0f) as usize];
    }
    String::from_utf8(out).expect("hex digits are ASCII")
}

#[cfg(test)]
mod tests {
    use super::{HashAlgorithm, apply};

    #[test]
    fn sha256_known_answer() {
        let out = apply("abc", HashAlgorithm::Sha256);
        assert_eq!(out, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn sha512_known_answer() {
        let out = apply("abc", HashAlgorithm::Sha512);
        assert_eq!(
            out,
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn deterministic() {
        let a = apply("hello", HashAlgorithm::Sha256);
        let b = apply("hello", HashAlgorithm::Sha256);
        assert_eq!(a, b);
    }
}
