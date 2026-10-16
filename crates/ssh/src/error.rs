//! Errors raised while starting or running the SSH tunnel.

use std::path::PathBuf;

/// Errors produced by the SSH tunnel.
///
/// Messages never embed passwords, passphrases, or key material.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    /// Neither a password nor a private key is configured.
    #[error("DB_SSH_PASSWORD or DB_SSH_KEY is required for SSH authentication")]
    MissingCredentials,

    /// The private key file could not be read.
    #[error("failed to read SSH key {}: {source}", path.display())]
    KeyRead {
        /// Path of the key file.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The unencrypted private key could not be parsed.
    #[error("failed to parse SSH key {}: {source}", path.display())]
    KeyParse {
        /// Path of the key file.
        path: PathBuf,
        /// Underlying key error.
        #[source]
        source: russh::keys::Error,
    },

    /// The private key is encrypted but no passphrase is configured.
    #[error("SSH key {} is encrypted; set DB_SSH_KEY_PASSPHRASE", path.display())]
    KeyEncrypted {
        /// Path of the key file.
        path: PathBuf,
    },

    /// The private key uses encryption that cannot be decrypted.
    #[error(
        "SSH key {} uses an unsupported encryption format; re-encrypt it with `ssh-keygen -p -f {}`",
        path.display(),
        path.display()
    )]
    KeyUnsupportedEncryption {
        /// Path of the key file.
        path: PathBuf,
    },

    /// Decrypting the private key with the passphrase failed.
    ///
    /// The source is kept for debugging; the message is the same for every
    /// key format so a wrong passphrase is recognisable.
    #[error("could not decrypt SSH key {} (wrong passphrase or unsupported format)", path.display())]
    KeyDecrypt {
        /// Path of the key file.
        path: PathBuf,
        /// Underlying key error.
        #[source]
        source: russh::keys::Error,
    },

    /// The local tunnel listener could not be bound.
    #[error("failed to bind SSH tunnel listener on {addr}: {source}")]
    Bind {
        /// Local address the listener tried to bind.
        addr: std::net::SocketAddr,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The TCP dial or SSH handshake failed.
    #[error("SSH connection to {target} failed: {source}")]
    Connect {
        /// `host:port` of the SSH server.
        target: String,
        /// Underlying SSH error.
        #[source]
        source: russh::Error,
    },

    /// The TCP dial, handshake, and authentication exceeded the deadline.
    #[error("SSH connection to {target} timed out after {secs}s")]
    Timeout {
        /// `host:port` of the SSH server.
        target: String,
        /// Deadline in seconds.
        secs: u64,
    },

    /// The SSH server rejected the credentials.
    #[error("SSH authentication failed for {user}@{target}")]
    AuthRejected {
        /// SSH user.
        user: String,
        /// `host:port` of the SSH server.
        target: String,
    },

    /// The authentication exchange itself failed.
    #[error("SSH authentication for {user}@{target} failed: {source}")]
    Auth {
        /// SSH user.
        user: String,
        /// `host:port` of the SSH server.
        target: String,
        /// Underlying SSH error.
        #[source]
        source: russh::Error,
    },

    /// The `known_hosts` file exists but could not be read.
    #[error("failed to read known_hosts {}: {source}", path.display())]
    KnownHostsRead {
        /// Path of the `known_hosts` file.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The host key could not be added to `known_hosts` under `accept-new`.
    #[error("failed to add SSH host key for {host} to {}: {source}", path.display())]
    KnownHostsWrite {
        /// Host as recorded in `known_hosts` (`host` or `[host]:port`).
        host: String,
        /// Path of the `known_hosts` file.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The host key is not in `known_hosts` and checking is `yes`.
    #[error(
        "SSH host key for {host} ({fingerprint}) is not in {}; verify the fingerprint, then run \
         `ssh-keyscan -p {port} {ssh_host} >> {}`",
        path.display(),
        path.display()
    )]
    HostKeyUnknown {
        /// Host as recorded in `known_hosts` (`host` or `[host]:port`).
        host: String,
        /// SSH server host as configured.
        ssh_host: String,
        /// SSH server port.
        port: u16,
        /// SHA-256 fingerprint of the presented key.
        fingerprint: String,
        /// Path of the `known_hosts` file.
        path: PathBuf,
    },

    /// The host key differs from the one recorded in `known_hosts`.
    #[error(
        "SSH host key for {host} ({fingerprint}) does not match {}:{line}; \
         the server may be impersonated",
        path.display()
    )]
    HostKeyChanged {
        /// Host as recorded in `known_hosts` (`host` or `[host]:port`).
        host: String,
        /// SHA-256 fingerprint of the presented key.
        fingerprint: String,
        /// Path of the `known_hosts` file.
        path: PathBuf,
        /// Line of the recorded key.
        line: usize,
    },

    /// The host key is marked `@revoked` in `known_hosts`.
    #[error("SSH host key for {host} ({fingerprint}) is revoked in {}:{line}", path.display())]
    HostKeyRevoked {
        /// Host as recorded in `known_hosts` (`host` or `[host]:port`).
        host: String,
        /// SHA-256 fingerprint of the presented key.
        fingerprint: String,
        /// Path of the `known_hosts` file.
        path: PathBuf,
        /// Line of the `@revoked` entry.
        line: usize,
    },

    /// The SSH server presented a host certificate, which is unsupported.
    #[error("SSH server {target} presented a host certificate, which is not supported")]
    HostCertificate {
        /// `host:port` of the SSH server.
        target: String,
    },

    /// An established SSH session was lost.
    #[error("SSH session to {target} lost: {cause}")]
    SessionLost {
        /// `host:port` of the SSH server.
        target: String,
        /// Why the session ended.
        cause: String,
    },

    /// A concurrent connect attempt failed while this caller waited for it.
    ///
    /// Carries that attempt's message so it is not dialled again.
    #[error("{cause}")]
    Unavailable {
        /// Message of the failed attempt.
        cause: String,
    },

    /// The SSH server refused to open a `direct-tcpip` channel.
    #[error("failed to open SSH channel to {target}: {source}")]
    ChannelOpen {
        /// `host:port` of the database as seen from the SSH server.
        target: String,
        /// Underlying SSH error.
        #[source]
        source: russh::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_includes_source_exactly_once() {
        let err = SshError::Connect {
            target: "bastion:22".into(),
            source: russh::Error::IO(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "Connection refused",
            )),
        };
        let rendered = err.to_string();
        assert!(
            rendered.starts_with("SSH connection to bastion:22 failed: "),
            "{rendered}"
        );
        assert_eq!(rendered.matches("Connection refused").count(), 1, "{rendered}");
    }
}
