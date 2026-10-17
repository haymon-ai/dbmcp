//! SSH tunnel settings, host-key policy enum, and `~` path expansion.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::database::DatabaseBackend;
use crate::error::{ConfigError, ConfigErrors};
use crate::secret::Secret;

/// Host-key verification policy for the SSH tunnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum StrictHostKeyChecking {
    /// Reject hosts missing from `known_hosts`.
    Yes,
    /// Trust unknown hosts on first use and record them.
    AcceptNew,
    /// Disable host-key verification (insecure).
    No,
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
/// Present on [`Config`](crate::Config) only when `SSH_HOST` is set.
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
    pub password: Option<Secret>,

    /// Path to the SSH private key.
    pub key: Option<PathBuf>,

    /// Passphrase for the SSH private key.
    pub key_passphrase: Option<Secret>,

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
    /// - the `sqlite` backend cannot be tunnelled.
    /// - `host` and `user` must not be empty.
    /// - a password or a key is required; a passphrase requires a key.
    /// - a set `key` path must be an existing file.
    /// - `known_hosts` must be resolved unless host-key checking is `no`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigErrors`] containing one [`ConfigError`] per failing rule.
    pub fn validate(&self, backend: DatabaseBackend) -> Result<(), ConfigErrors> {
        let mut errors = Vec::new();

        if backend == DatabaseBackend::Sqlite {
            errors.push(ConfigError::SshSqliteUnsupported);
        }
        if self.host.is_empty() {
            errors.push(ConfigError::EmptySshHost);
        }
        if self.user.is_empty() {
            errors.push(ConfigError::MissingSshUser);
        }
        if self.password.is_none() && self.key.is_none() {
            errors.push(ConfigError::MissingSshAuth);
        }
        if let Some(key) = &self.key
            && !key.is_file()
        {
            errors.push(ConfigError::FileNotFound("SSH_KEY".into(), key.display().to_string()));
        }
        if self.key_passphrase.is_some() && self.key.is_none() {
            errors.push(ConfigError::SshKeyPassphraseWithoutKey);
        }
        if self.known_hosts.is_none() && self.strict_host_key_checking != StrictHostKeyChecking::No {
            errors.push(ConfigError::MissingSshKnownHosts);
        }

        ConfigErrors::from_vec(errors).map_or(Ok(()), Err)
    }
}

/// Expands a leading `~` path component against `home`.
///
/// Requires at least one component after `~`, so `~`, `~/` and `~user`
/// stay literal. Returns `path` unchanged when `home` is `None`.
#[must_use]
pub fn expand_tilde(path: &Path, home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return path.to_path_buf();
    };
    match path.strip_prefix("~") {
        Ok(rest) if rest.components().next().is_some() => {
            let mut expanded = home.to_path_buf();
            expanded.extend(rest.components());
            expanded
        }
        _ => path.to_path_buf(),
    }
}

/// Returns the user's home directory, treating an empty path as absent.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    std::env::home_dir().filter(|home| !home.as_os_str().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh_config() -> SshConfig {
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

    fn errors_of(config: &SshConfig, backend: DatabaseBackend) -> Vec<String> {
        config
            .validate(backend)
            .expect_err("must fail")
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn validate_accepts_password_auth() {
        ssh_config().validate(DatabaseBackend::Mysql).expect("valid");
    }

    #[test]
    fn validate_rejects_sqlite_backend() {
        assert_eq!(
            errors_of(&ssh_config(), DatabaseBackend::Sqlite),
            ["SSH_HOST is not supported with the sqlite backend"]
        );
    }

    #[test]
    fn validate_rejects_empty_host() {
        let config = SshConfig {
            host: String::new(),
            ..ssh_config()
        };
        assert_eq!(
            errors_of(&config, DatabaseBackend::Mysql),
            ["SSH_HOST must not be empty"]
        );
    }

    #[test]
    fn validate_rejects_missing_user() {
        let config = SshConfig {
            user: String::new(),
            ..ssh_config()
        };
        assert_eq!(
            errors_of(&config, DatabaseBackend::Postgres),
            ["SSH_USER is required when SSH_HOST is set"]
        );
    }

    #[test]
    fn validate_rejects_missing_password_and_key() {
        let config = SshConfig {
            password: None,
            ..ssh_config()
        };
        assert_eq!(
            errors_of(&config, DatabaseBackend::Mysql),
            ["SSH_PASSWORD or SSH_KEY is required when SSH_HOST is set"]
        );
    }

    #[test]
    fn validate_rejects_missing_key_file() {
        let config = SshConfig {
            key: Some(PathBuf::from("/nonexistent/id_ed25519")),
            ..ssh_config()
        };
        let errors = config.validate(DatabaseBackend::Mysql).expect_err("must fail");
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            &errors[0],
            ConfigError::FileNotFound(name, _) if name == "SSH_KEY"
        ));
        assert_eq!(
            errors[0].to_string(),
            format!(
                "SSH_KEY file not found: {}",
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
        let errors = config.validate(DatabaseBackend::Mysql).expect_err("must fail");
        assert!(matches!(&errors[0], ConfigError::FileNotFound(_, _)));
    }

    #[test]
    fn validate_rejects_passphrase_without_key() {
        let config = SshConfig {
            key_passphrase: Some("secret".into()),
            ..ssh_config()
        };
        assert_eq!(
            errors_of(&config, DatabaseBackend::Mysql),
            ["SSH_KEY_PASSPHRASE requires SSH_KEY"]
        );
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
                errors_of(&config, DatabaseBackend::Mysql),
                ["SSH_KNOWN_HOSTS is required when the home directory cannot be determined"],
                "mode {mode}"
            );
        }

        let config = SshConfig {
            known_hosts: None,
            strict_host_key_checking: StrictHostKeyChecking::No,
            ..ssh_config()
        };
        config
            .validate(DatabaseBackend::Mysql)
            .expect("mode no needs no known_hosts");
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
        let errors = config.validate(DatabaseBackend::Sqlite).expect_err("must fail");
        assert!(matches!(errors[0], ConfigError::SshSqliteUnsupported));
        assert!(matches!(errors[1], ConfigError::EmptySshHost));
        assert!(matches!(errors[2], ConfigError::MissingSshUser));
        assert!(matches!(errors[3], ConfigError::MissingSshAuth));
        assert!(matches!(errors[4], ConfigError::SshKeyPassphraseWithoutKey));
        assert!(matches!(errors[5], ConfigError::MissingSshKnownHosts));
        assert_eq!(errors.len(), 6);
    }

    #[test]
    fn strict_host_key_checking_display_matches_value_names() {
        assert_eq!(StrictHostKeyChecking::Yes.to_string(), "yes");
        assert_eq!(StrictHostKeyChecking::AcceptNew.to_string(), "accept-new");
        assert_eq!(StrictHostKeyChecking::No.to_string(), "no");
    }

    fn home() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\deploy")
        } else {
            PathBuf::from("/home/deploy")
        }
    }

    fn expand(path: &str) -> PathBuf {
        expand_tilde(Path::new(path), Some(&home()))
    }

    #[test]
    fn expand_tilde_slash_prefix() {
        assert_eq!(expand("~/.ssh/id_ed25519"), home().join(".ssh").join("id_ed25519"));
    }

    #[test]
    fn expand_tilde_double_slash_stays_under_home() {
        assert_eq!(expand("~//.ssh/known_hosts"), home().join(".ssh").join("known_hosts"));
    }

    #[test]
    fn expand_tilde_leaves_bare_and_trailing_slash_literal() {
        assert_eq!(expand("~"), PathBuf::from("~"));
        assert_eq!(expand("~/"), PathBuf::from("~/"));
    }

    #[test]
    fn expand_tilde_leaves_tilde_user_literal() {
        assert_eq!(expand("~deploy/.ssh/id"), PathBuf::from("~deploy/.ssh/id"));
    }

    #[test]
    fn expand_tilde_leaves_non_tilde_paths_untouched() {
        assert_eq!(expand("keys/id"), PathBuf::from("keys/id"));
        assert_eq!(expand("a/~/b"), PathBuf::from("a/~/b"));
    }

    #[test]
    fn expand_tilde_without_home_is_literal() {
        assert_eq!(expand_tilde(Path::new("~/.ssh/id"), None), PathBuf::from("~/.ssh/id"));
    }

    #[cfg(windows)]
    #[test]
    fn expand_tilde_backslash_prefix_on_windows() {
        assert_eq!(expand(r"~\.ssh\id_ed25519"), home().join(".ssh").join("id_ed25519"));
        assert_eq!(expand(r"~\"), PathBuf::from(r"~\"));
    }

    #[cfg(not(windows))]
    #[test]
    fn expand_tilde_backslash_is_literal_off_windows() {
        assert_eq!(expand(r"~\.ssh\id"), PathBuf::from(r"~\.ssh\id"));
    }
}
