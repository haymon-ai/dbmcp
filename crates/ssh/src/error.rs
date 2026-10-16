//! Errors raised while starting or running the SSH tunnel.

/// Errors produced by the SSH tunnel.
///
/// Messages never embed passwords, passphrases, or key material.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    /// A configured option is not supported by this build yet.
    #[error("{0} is not yet implemented")]
    NotImplemented(String),

    /// No password is configured for the SSH user.
    #[error("DB_SSH_PASSWORD is required for SSH authentication")]
    MissingPassword,

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
