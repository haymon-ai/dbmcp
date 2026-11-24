//! Resolved SSH tunnel settings.

use std::path::PathBuf;
use std::time::Duration;

use secrecy::SecretString;

use super::StrictHostKeyChecking;

/// Resolved SSH tunnel settings.
///
/// Present on [`Config`](crate::Config) only when `ssh.host` / `DBMCP_SSH_HOST` is set.
/// Paths are already `~`-expanded and the `known_hosts` default is resolved.
#[derive(Clone, Debug)]
pub struct Config {
    /// SSH server host.
    pub host: String,

    /// SSH server port (resolved default: [`Self::DEFAULT_PORT`]).
    pub port: u16,

    /// SSH user.
    pub user: String,

    /// SSH password.
    pub password: Option<SecretString>,

    /// Path to the SSH private key.
    pub key: Option<PathBuf>,

    /// Passphrase for the SSH private key.
    pub key_passphrase: Option<SecretString>,

    /// Path to the `known_hosts` file (`None` only when unverified and no home).
    pub known_hosts: Option<PathBuf>,

    /// Host-key verification policy.
    pub strict_host_key_checking: StrictHostKeyChecking,

    /// Keepalive interval (`Duration::ZERO` disables keepalive).
    pub keepalive_interval: Duration,

    /// Local tunnel port (`None` = ephemeral).
    pub local_port: Option<u16>,
}

impl Config {
    /// Default SSH server port.
    pub const DEFAULT_PORT: u16 = 22;
    /// Default host-key verification policy.
    pub const DEFAULT_STRICT_HOST_KEY_CHECKING: StrictHostKeyChecking = StrictHostKeyChecking::Yes;
    /// Default keepalive interval in seconds.
    pub const DEFAULT_KEEPALIVE_INTERVAL_SECS: u64 = 30;
}
