//! Validation rules for the `ssh` section.

use dbmcp_types::ssh;

use crate::env;
use crate::error::{ConfigError, Diagnostic, into_result};

/// Validates the `ssh` section.
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
pub(crate) fn ssh(config: &ssh::Config) -> Result<(), Vec<Diagnostic>> {
    let mut errors = Vec::new();

    if config.host.is_empty() {
        errors.push(Diagnostic::new(ConfigError::EmptySshHost).involving(&[env::SSH_HOST]));
    }
    if config.user.is_empty() {
        errors.push(
            Diagnostic::new(ConfigError::MissingSshUser)
                .involving(&[env::SSH_HOST])
                .missing(&[env::SSH_USER]),
        );
    }
    if config.password.is_none() && config.key.is_none() {
        errors.push(
            Diagnostic::new(ConfigError::MissingSshAuth)
                .involving(&[env::SSH_HOST])
                .missing(&[env::SSH_PASSWORD, env::SSH_KEY]),
        );
    }
    if let Some(key) = &config.key
        && !key.is_file()
    {
        errors.push(
            Diagnostic::new(ConfigError::FileNotFound(env::SSH_KEY.path, key.display().to_string()))
                .involving(&[env::SSH_KEY]),
        );
    }
    if config.key_passphrase.is_some() && config.key.is_none() {
        errors.push(
            Diagnostic::new(ConfigError::SshKeyPassphraseWithoutKey)
                .involving(&[env::SSH_KEY_PASSPHRASE])
                .missing(&[env::SSH_KEY]),
        );
    }
    if config.known_hosts.is_none() && config.strict_host_key_checking != ssh::StrictHostKeyChecking::No {
        errors.push(Diagnostic::new(ConfigError::MissingSshKnownHosts).missing(&[env::SSH_KNOWN_HOSTS]));
    }

    into_result(errors)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    pub(crate) fn ssh_config() -> ssh::Config {
        ssh::Config {
            host: "bastion.example.com".into(),
            port: ssh::Config::DEFAULT_PORT,
            user: "deploy".into(),
            password: Some("pw".into()),
            key: None,
            key_passphrase: None,
            known_hosts: Some(PathBuf::from("/home/deploy/.ssh/known_hosts")),
            strict_host_key_checking: ssh::StrictHostKeyChecking::Yes,
            keepalive_interval: Duration::from_secs(ssh::Config::DEFAULT_KEEPALIVE_INTERVAL_SECS),
            local_port: None,
        }
    }

    fn errors_of(config: &ssh::Config) -> Vec<String> {
        super::ssh(config)
            .expect_err("must fail")
            .iter()
            .map(|diagnostic| diagnostic.error.to_string())
            .collect()
    }

    #[test]
    fn validate_accepts_password_auth() {
        super::ssh(&ssh_config()).expect("valid");
    }

    #[test]
    fn validate_rejects_empty_host() {
        let config = ssh::Config {
            host: String::new(),
            ..ssh_config()
        };
        assert_eq!(errors_of(&config), ["ssh.host must not be empty"]);
    }

    #[test]
    fn validate_rejects_missing_user() {
        let config = ssh::Config {
            user: String::new(),
            ..ssh_config()
        };
        assert_eq!(errors_of(&config), [ConfigError::MissingSshUser.to_string()]);
    }

    #[test]
    fn validate_rejects_missing_password_and_key() {
        let config = ssh::Config {
            password: None,
            ..ssh_config()
        };
        assert_eq!(errors_of(&config), [ConfigError::MissingSshAuth.to_string()]);
    }

    #[test]
    fn validate_rejects_missing_key_file() {
        let config = ssh::Config {
            key: Some(PathBuf::from("/nonexistent/id_ed25519")),
            ..ssh_config()
        };
        let errors = super::ssh(&config).expect_err("must fail");
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
        let config = ssh::Config {
            key: Some(std::env::temp_dir()),
            ..ssh_config()
        };
        let errors = super::ssh(&config).expect_err("must fail");
        assert!(matches!(&errors[0].error, ConfigError::FileNotFound(_, _)));
    }

    #[test]
    fn validate_rejects_passphrase_without_key() {
        let config = ssh::Config {
            key_passphrase: Some("secret".into()),
            ..ssh_config()
        };
        assert_eq!(errors_of(&config), ["ssh.key_passphrase requires ssh.key"]);
    }

    #[test]
    fn validate_rejects_unresolved_known_hosts_unless_mode_no() {
        for mode in [ssh::StrictHostKeyChecking::Yes, ssh::StrictHostKeyChecking::AcceptNew] {
            let config = ssh::Config {
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

        let config = ssh::Config {
            known_hosts: None,
            strict_host_key_checking: ssh::StrictHostKeyChecking::No,
            ..ssh_config()
        };
        super::ssh(&config).expect("mode no needs no known_hosts");
    }

    #[test]
    fn validate_accumulates_errors_in_rule_order() {
        let config = ssh::Config {
            host: String::new(),
            user: String::new(),
            password: None,
            key_passphrase: Some("secret".into()),
            known_hosts: None,
            ..ssh_config()
        };
        let errors = super::ssh(&config).expect_err("must fail");
        assert!(matches!(errors[0].error, ConfigError::EmptySshHost));
        assert!(matches!(errors[1].error, ConfigError::MissingSshUser));
        assert!(matches!(errors[2].error, ConfigError::MissingSshAuth));
        assert!(matches!(errors[3].error, ConfigError::SshKeyPassphraseWithoutKey));
        assert!(matches!(errors[4].error, ConfigError::MissingSshKnownHosts));
        assert_eq!(errors.len(), 5);
    }

    #[test]
    fn strict_host_key_checking_display_matches_value_names() {
        assert_eq!(ssh::StrictHostKeyChecking::Yes.to_string(), "yes");
        assert_eq!(ssh::StrictHostKeyChecking::AcceptNew.to_string(), "accept-new");
        assert_eq!(ssh::StrictHostKeyChecking::No.to_string(), "no");
    }
}
