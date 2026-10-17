//! Private-key loading with clear errors for russh's format gaps.
//!
//! Wraps [`russh::keys::decode_secret_key`] so every supported format
//! loads with or without a stray passphrase, and every failure maps to a
//! single, actionable [`SshError`].

use std::path::Path;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use russh::keys::{self, PrivateKey};
use tracing::debug;

use crate::error::SshError;

/// PEM header marking legacy (PKCS#1 / SEC1) encryption.
const LEGACY_ENCRYPTED: &str = "Proc-Type: 4,ENCRYPTED";

/// The only legacy PEM cipher russh can decrypt.
const LEGACY_SUPPORTED_CIPHER: &str = "DEK-Info: AES-128-CBC,";

/// PEM header of an encrypted PKCS#8 key.
const PKCS8_ENCRYPTED_BEGIN: &str = "-----BEGIN ENCRYPTED PRIVATE KEY-----";

/// PEM header of an RSA (PKCS#1) key, the only legacy-encrypted type russh decodes.
const PKCS1_BEGIN: &str = "-----BEGIN RSA PRIVATE KEY-----";

/// DER encoding of the PBES2 OID (1.2.840.113549.1.5.13).
const PBES2_OID: &[u8] = &[0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x05, 0x0D];

/// Loads and decrypts the private key at `path`.
///
/// A passphrase given for an unencrypted key is ignored.
///
/// # Errors
///
/// - [`SshError::KeyRead`] when the file cannot be read.
/// - [`SshError::KeyUnsupportedEncryption`] for encryption russh cannot
///   decrypt (legacy PEM other than AES-128-CBC, encrypted SEC1, PKCS#8 PBES1).
/// - [`SshError::KeyEncrypted`] when the key is encrypted and no passphrase is set.
/// - [`SshError::KeyDecrypt`] when decryption with the passphrase fails.
/// - [`SshError::KeyParse`] when an unencrypted key cannot be parsed.
#[allow(clippy::result_large_err)]
pub(crate) fn load_private_key(path: &Path, passphrase: Option<&str>) -> Result<PrivateKey, SshError> {
    let text = std::fs::read_to_string(path).map_err(|source| SshError::KeyRead {
        path: path.to_owned(),
        source,
    })?;
    let pem = Pem::sniff(&text);
    if pem.unsupported_encryption {
        return Err(SshError::KeyUnsupportedEncryption { path: path.to_owned() });
    }

    let first = match keys::decode_secret_key(&text, passphrase) {
        Ok(key) => return Ok(key),
        Err(e) => e,
    };

    if passphrase.is_none() {
        if pem.encrypted || matches!(first, keys::Error::KeyIsEncrypted) {
            return Err(SshError::KeyEncrypted { path: path.to_owned() });
        }
        return Err(SshError::KeyParse {
            path: path.to_owned(),
            source: first,
        });
    }

    // russh rejects a passphrase for unencrypted PKCS#8 and SEC1 keys.
    // Armour-marked keys are encrypted, so retrying them is pointless.
    let retry = if pem.encrypted {
        None
    } else {
        match keys::decode_secret_key(&text, None) {
            Ok(key) => return Ok(key),
            Err(keys::Error::KeyIsEncrypted) => None,
            Err(retry) => Some(retry),
        }
    };
    if let Some(retry) = retry {
        return Err(SshError::KeyParse {
            path: path.to_owned(),
            source: retry,
        });
    }

    debug!("SSH key {} could not be decrypted: {first}", path.display());
    Err(SshError::KeyDecrypt {
        path: path.to_owned(),
        source: first,
    })
}

/// Logs a warning when group or others have any access to the key file.
///
/// Never fails: secret mounts in Docker and Kubernetes are often 0644.
#[cfg(unix)]
pub(crate) fn warn_if_key_exposed(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;

    let Ok(metadata) = std::fs::metadata(path) else {
        return;
    };
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        tracing::warn!(
            "SSH key {} is accessible by group or others (mode {mode:03o}); run `chmod 600 {}`",
            path.display(),
            path.display()
        );
    }
}

/// No-op: Windows ACLs are not checked.
#[cfg(not(unix))]
pub(crate) fn warn_if_key_exposed(_path: &Path) {}

/// Encryption facts read from the PEM armour before decoding.
#[derive(Debug, Default)]
struct Pem {
    /// The armour declares the key encrypted.
    encrypted: bool,
    /// The key uses encryption russh cannot decrypt.
    unsupported_encryption: bool,
}

impl Pem {
    /// Inspects PEM headers and, for encrypted PKCS#8, the algorithm OID.
    fn sniff(text: &str) -> Self {
        let lines = || text.lines().map(str::trim);

        if lines().any(|line| line == LEGACY_ENCRYPTED) {
            let rsa = lines().any(|line| line == PKCS1_BEGIN);
            let aes128 = lines().any(|line| line.starts_with(LEGACY_SUPPORTED_CIPHER));
            return Self {
                encrypted: true,
                unsupported_encryption: !(rsa && aes128),
            };
        }

        if lines().any(|line| line == PKCS8_ENCRYPTED_BEGIN) {
            let body: String = lines()
                .skip_while(|line| *line != PKCS8_ENCRYPTED_BEGIN)
                .skip(1)
                .take_while(|line| !line.starts_with("-----END "))
                .collect();
            // Undecodable bodies are left to russh to report.
            let pbes1 = STANDARD
                .decode(body)
                .is_ok_and(|der| !der.windows(PBES2_OID.len()).take(32).any(|w| w == PBES2_OID));
            return Self {
                encrypted: true,
                unsupported_encryption: pbes1,
            };
        }

        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use russh::keys::Algorithm;
    use russh::keys::ssh_key::EcdsaCurve;

    use super::*;

    const PASSPHRASE: &str = "secret";

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata").join(name)
    }

    #[allow(clippy::result_large_err)]
    fn load(name: &str, passphrase: Option<&str>) -> Result<PrivateKey, SshError> {
        load_private_key(&fixture(name), passphrase)
    }

    #[test]
    fn loads_openssh_key_types() {
        for (name, algorithm) in [
            ("openssh_ed25519", Algorithm::Ed25519),
            (
                "openssh_ecdsa",
                Algorithm::Ecdsa {
                    curve: EcdsaCurve::NistP256,
                },
            ),
            ("openssh_rsa", Algorithm::Rsa { hash: None }),
        ] {
            let key = load(name, None).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(key.algorithm(), algorithm, "{name}");
        }
    }

    #[test]
    fn decrypts_encrypted_keys_with_passphrase() {
        for name in ["openssh_ed25519_enc", "pkcs8_ed25519_enc", "pkcs1_rsa_aes128"] {
            load(name, Some(PASSPHRASE)).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn k1_ignores_passphrase_for_unencrypted_keys() {
        for name in ["pkcs8_ed25519", "sec1_ecdsa", "openssh_ed25519"] {
            load(name, Some(PASSPHRASE)).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn k2_encrypted_key_without_passphrase() {
        for name in ["openssh_ed25519_enc", "pkcs8_ed25519_enc", "pkcs1_rsa_aes128"] {
            let err = load(name, None).expect_err(name);
            assert!(matches!(err, SshError::KeyEncrypted { .. }), "{name}: {err:?}");
            assert_eq!(
                err.to_string(),
                format!(
                    "SSH key {} is encrypted; set SSH_KEY_PASSPHRASE",
                    fixture(name).display()
                )
            );
        }
    }

    #[test]
    fn k3_unsupported_encryption_suggests_ssh_keygen() {
        for name in ["pkcs1_rsa_aes256", "sec1_ecdsa_enc", "pkcs8_rsa_pbes1"] {
            for passphrase in [None, Some(PASSPHRASE)] {
                let err = load(name, passphrase).expect_err(name);
                let path = fixture(name);
                assert!(
                    matches!(err, SshError::KeyUnsupportedEncryption { .. }),
                    "{name}: {err:?}"
                );
                assert_eq!(
                    err.to_string(),
                    format!(
                        "SSH key {} uses an unsupported encryption format; \
                         re-encrypt it with `ssh-keygen -p -f {}`",
                        path.display(),
                        path.display()
                    )
                );
            }
        }
    }

    #[test]
    fn k5_wrong_passphrase_has_one_message() {
        for name in ["openssh_ed25519_enc", "pkcs8_ed25519_enc", "pkcs1_rsa_aes128"] {
            let err = load(name, Some("wrong")).expect_err(name);
            assert!(matches!(err, SshError::KeyDecrypt { .. }), "{name}: {err:?}");
            assert_eq!(
                err.to_string(),
                format!(
                    "could not decrypt SSH key {} (wrong passphrase or unsupported format)",
                    fixture(name).display()
                )
            );
        }
    }

    #[test]
    fn missing_file_is_a_read_error() {
        let err = load("does_not_exist", None).expect_err("missing");
        assert!(matches!(err, SshError::KeyRead { .. }), "{err:?}");
    }

    #[test]
    fn garbage_is_a_parse_error() {
        let err = load("openssh_ed25519.pub", Some(PASSPHRASE)).expect_err("public key");
        assert!(matches!(err, SshError::KeyParse { .. }), "{err:?}");
    }

    #[cfg(unix)]
    mod permissions {
        use std::io::Write as _;
        use std::os::unix::fs::PermissionsExt as _;
        use std::sync::{Arc, Mutex};

        use super::*;

        /// Captures log output written by `tracing_subscriber::fmt`.
        #[derive(Clone, Default)]
        struct Capture(Arc<Mutex<Vec<u8>>>);

        impl std::io::Write for Capture {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("capture lock").extend_from_slice(buf);
                Ok(buf.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        /// Runs the permission check on a key with `mode` and returns the log.
        fn logs_for_mode(mode: u32) -> String {
            let path = std::env::temp_dir().join(format!("dbmcp-ssh-key-{}-{mode:o}", std::process::id()));
            std::fs::File::create(&path)
                .and_then(|mut file| file.write_all(b"key"))
                .expect("write key");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");

            let capture = Capture::default();
            let writer = capture.clone();
            let subscriber = tracing_subscriber::fmt()
                .with_writer(move || writer.clone())
                .with_ansi(false)
                .finish();
            tracing::subscriber::with_default(subscriber, || warn_if_key_exposed(&path));
            std::fs::remove_file(&path).expect("remove key");

            String::from_utf8(capture.0.lock().expect("capture lock").clone()).expect("utf8")
        }

        #[test]
        fn warns_for_group_or_other_readable_key() {
            let logs = logs_for_mode(0o644);
            assert!(logs.contains("WARN"), "{logs}");
            assert!(logs.contains("mode 644"), "{logs}");
            assert!(logs.contains("chmod 600"), "{logs}");
        }

        #[test]
        fn warns_for_group_writable_key() {
            let logs = logs_for_mode(0o620);
            assert!(logs.contains("mode 620"), "{logs}");
        }

        #[test]
        fn silent_for_owner_only_key() {
            let logs = logs_for_mode(0o600);
            assert!(logs.is_empty(), "{logs}");
        }
    }
}
