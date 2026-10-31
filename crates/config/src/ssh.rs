//! SSH tunnel settings, host-key policy enum, and `~` path expansion.

use std::path::PathBuf;
use std::time::Duration;

use secrecy::SecretString;

use crate::env;
use crate::error::{ConfigError, Diagnostic, into_result};

/// Host-key verification policy for the SSH tunnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrictHostKeyChecking {
    /// Reject hosts missing from `known_hosts`.
    Yes,
    /// Trust unknown hosts on first use and record them.
    AcceptNew,
    /// Disable host-key verification (insecure).
    No,
}

impl StrictHostKeyChecking {
    /// Accepted setting values, in the order error messages list them.
    pub(crate) const CHOICES: &[(&str, Self)] =
        &[("yes", Self::Yes), ("accept-new", Self::AcceptNew), ("no", Self::No)];
}

impl std::fmt::Display for StrictHostKeyChecking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Yes => write!(f, "yes"),
            Self::AcceptNew => write!(f, "accept-new"),
            Self::No => write!(f, "no"),
        }
    }
}

/// Resolved SSH tunnel settings.
///
/// Present on [`Config`](crate::Config) only when `ssh.host` / `DBMCP_SSH_HOST` is set.
/// Paths are already `~`-expanded and the `known_hosts` default is resolved.
#[derive(Clone, Debug)]
pub struct SshConfig {
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

impl SshConfig {
    /// Default SSH server port.
    pub const DEFAULT_PORT: u16 = 22;
    /// Default host-key verification policy.
    pub const DEFAULT_STRICT_HOST_KEY_CHECKING: StrictHostKeyChecking = StrictHostKeyChecking::Yes;
    /// Default keepalive interval in seconds.
    pub const DEFAULT_KEEPALIVE_INTERVAL_SECS: u64 = 30;

    /// Validates this configuration, accumulating every rule violation.
    ///
    /// Rules enforced:
    /// - `host` and `user` must not be empty.
    /// - a password or a key is required; a passphrase requires a key.
    /// - a set `key` path must be an existing file.
    /// - `known_hosts` must be resolved unless host-key checking is `no`.
    ///
    /// # Errors
    ///
    /// Returns one [`Diagnostic`] per failing rule.
    pub(crate) fn validate(&self) -> Result<(), Vec<Diagnostic>> {
        let mut errors = Vec::new();

        if self.host.is_empty() {
            errors.push(Diagnostic::new(ConfigError::EmptySshHost).involving(&[env::SSH_HOST]));
        }
        if self.user.is_empty() {
            errors.push(
                Diagnostic::new(ConfigError::MissingSshUser)
                    .involving(&[env::SSH_HOST])
                    .missing(&[env::SSH_USER]),
            );
        }
        if self.password.is_none() && self.key.is_none() {
            errors.push(
                Diagnostic::new(ConfigError::MissingSshAuth)
                    .involving(&[env::SSH_HOST])
                    .missing(&[env::SSH_PASSWORD, env::SSH_KEY]),
            );
        }
        if let Some(key) = &self.key
            && !key.is_file()
        {
            errors.push(
                Diagnostic::new(ConfigError::FileNotFound(env::SSH_KEY.path, key.display().to_string()))
                    .involving(&[env::SSH_KEY]),
            );
        }
        if self.key_passphrase.is_some() && self.key.is_none() {
            errors.push(
                Diagnostic::new(ConfigError::SshKeyPassphraseWithoutKey)
                    .involving(&[env::SSH_KEY_PASSPHRASE])
                    .missing(&[env::SSH_KEY]),
            );
        }
        if self.known_hosts.is_none() && self.strict_host_key_checking != StrictHostKeyChecking::No {
            errors.push(Diagnostic::new(ConfigError::MissingSshKnownHosts).missing(&[env::SSH_KNOWN_HOSTS]));
        }

        into_result(errors)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use super::*;

    pub(crate) fn ssh_config() -> SshConfig {
        SshConfig {
            host: "bastion.example.com".into(),
            port: SshConfig::DEFAULT_PORT,
            user: "deploy".into(),
            password: Some("pw".into()),
            key: None,
            key_passphrase: None,
            known_hosts: Some(PathBuf::from("/home/deploy/.ssh/known_hosts")),
            strict_host_key_checking: StrictHostKeyChecking::Yes,
            keepalive_interval: Duration::from_secs(SshConfig::DEFAULT_KEEPALIVE_INTERVAL_SECS),
            local_port: None,
        }
    }

    fn errors_of(config: &SshConfig) -> Vec<String> {
        config
            .validate()
            .expect_err("must fail")
            .iter()
            .map(|diagnostic| diagnostic.error.to_string())
            .collect()
    }

    #[test]
    fn validate_accepts_password_auth() {
        ssh_config().validate().expect("valid");
    }

    #[test]
    fn validate_rejects_empty_host() {
        let config = SshConfig {
            host: String::new(),
            ..ssh_config()
        };
        assert_eq!(errors_of(&config), ["ssh.host must not be empty"]);
    }

    #[test]
    fn validate_rejects_missing_user() {
        let config = SshConfig {
            user: String::new(),
            ..ssh_config()
        };
        assert_eq!(errors_of(&config), [ConfigError::MissingSshUser.to_string()]);
    }

    #[test]
    fn validate_rejects_missing_password_and_key() {
        let config = SshConfig {
            password: None,
            ..ssh_config()
        };
        assert_eq!(errors_of(&config), [ConfigError::MissingSshAuth.to_string()]);
    }

    #[test]
    fn validate_rejects_missing_key_file() {
        let config = SshConfig {
            key: Some(PathBuf::from("/nonexistent/id_ed25519")),
            ..ssh_config()
        };
        let errors = config.validate().expect_err("must fail");
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            &errors[0].error,
            ConfigError::FileNotFound(name, _) if *name == "ssh.key"
        ));
        assert_eq!(
            errors[0].to_string(),
            format!(
                "ssh.key: file not found: {}",
                Path::new("/nonexistent/id_ed25519").display()
            )
        );
    }

    #[test]
    fn validate_rejects_key_path_that_is_a_directory() {
        let config = SshConfig {
            key: Some(std::env::temp_dir()),
            ..ssh_config()
        };
        let errors = config.validate().expect_err("must fail");
        assert!(matches!(&errors[0].error, ConfigError::FileNotFound(_, _)));
    }

    #[test]
    fn validate_rejects_passphrase_without_key() {
        let config = SshConfig {
            key_passphrase: Some("secret".into()),
            ..ssh_config()
        };
        assert_eq!(errors_of(&config), ["ssh.key_passphrase requires ssh.key"]);
    }

    #[test]
    fn validate_rejects_unresolved_known_hosts_unless_mode_no() {
        for mode in [StrictHostKeyChecking::Yes, StrictHostKeyChecking::AcceptNew] {
            let config = SshConfig {
                known_hosts: None,
                strict_host_key_checking: mode,
                ..ssh_config()
            };
            assert_eq!(
                errors_of(&config),
                [ConfigError::MissingSshKnownHosts.to_string()],
                "mode {mode}"
            );
        }

        let config = SshConfig {
            known_hosts: None,
            strict_host_key_checking: StrictHostKeyChecking::No,
            ..ssh_config()
        };
        config.validate().expect("mode no needs no known_hosts");
    }

    #[test]
    fn validate_accumulates_errors_in_rule_order() {
        let config = SshConfig {
            host: String::new(),
            user: String::new(),
            password: None,
            key_passphrase: Some("secret".into()),
            known_hosts: None,
            ..ssh_config()
        };
        let errors = config.validate().expect_err("must fail");
        assert!(matches!(errors[0].error, ConfigError::EmptySshHost));
        assert!(matches!(errors[1].error, ConfigError::MissingSshUser));
        assert!(matches!(errors[2].error, ConfigError::MissingSshAuth));
        assert!(matches!(errors[3].error, ConfigError::SshKeyPassphraseWithoutKey));
        assert!(matches!(errors[4].error, ConfigError::MissingSshKnownHosts));
        assert_eq!(errors.len(), 5);
    }

    #[test]
    fn strict_host_key_checking_display_matches_value_names() {
        assert_eq!(StrictHostKeyChecking::Yes.to_string(), "yes");
        assert_eq!(StrictHostKeyChecking::AcceptNew.to_string(), "accept-new");
        assert_eq!(StrictHostKeyChecking::No.to_string(), "no");
    }
}
