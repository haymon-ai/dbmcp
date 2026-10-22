//! Clap argument groups and config loading shared by transport subcommands.
//!
//! Hosts [`SharedArgs`], [`DatabaseArgs`], [`PiiArgs`], and
//! [`SshArgs`] — clap argument groups bundling `--config`,
//! `--log-level`, and every `--db-*`, `--pii-*`, and `--ssh-*` flag so each
//! is defined exactly once and embedded via `#[command(flatten)]` wherever
//! needed. Every setting flag is an `Option` without a clap default, so
//! `None` means "not set on the command line or through an env var", and
//! its clap id is its config path. Each group converts into its
//! [`ConfigLayer`] section. [`load`] fills the gaps from dotenv files and
//! merges the result over the config file.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::PathBuf;

use clap::builder::BoolishValueParser;
use clap::{ArgAction, Args, CommandFactory, FromArgMatches};
use dbmcp_config::{
    ConfigLayer, ConfigWarning, DatabaseBackend, DatabaseLayer, Dotenv, LoadError, LogLevel, PiiCategory, PiiLayer,
    PiiOperator, Provenance, Resolved, Route, SecretString, Source, Sources, SshLayer, StrictHostKeyChecking, ssh,
};

/// Config-file and logging flags shared by every transport subcommand.
#[derive(Debug, Args)]
pub(crate) struct SharedArgs {
    /// Path to a YAML config file
    #[arg(long = "config", value_name = "PATH", help_heading = "Config File")]
    pub(crate) config: Option<PathBuf>,

    /// Path to a dotenv file; repeatable, later wins. Replaces the .env beside --config
    #[arg(long = "env", value_name = "PATH", help_heading = "Config File")]
    pub(crate) env: Vec<PathBuf>,

    /// Log level [default: info]
    #[arg(
        id = "log_level",
        long = "log-level",
        value_name = "LOG_LEVEL",
        env = "LOG_LEVEL",
        ignore_case = true,
        help_heading = "Logging"
    )]
    pub(crate) log_level: Option<LogLevel>,
}

/// Loads the configuration of the transport subcommand `cli`.
///
/// `shared` holds the config-file flags of `cli`, and `layer` turns a
/// parsed `C` into a layer. Reads the `--config` and dotenv files, fills
/// every flag not set on the command line or via its env var from a
/// dotenv variable of the same name, and merges the result over the config
/// file. Warnings are appended to `warnings`, including when loading fails.
///
/// # Errors
///
/// Returns [`LoadError`] from the first failing stage.
pub(crate) fn load<C: CommandFactory + FromArgMatches>(
    cli: &C,
    shared: &SharedArgs,
    layer: fn(&C) -> ConfigLayer,
    warnings: &mut Vec<ConfigWarning>,
) -> Result<Resolved, LoadError> {
    let process_env: HashMap<String, String> = std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .collect();
    let sources = Sources::read(shared.config.as_deref(), &shared.env, &process_env, warnings)?;

    let cli = layer(cli);
    let set = cli.set_paths();
    let routes = flag_routes(&C::command());
    let mut provenance = Provenance::default();
    for (path, route) in &routes {
        if set.contains(&path.as_str()) {
            provenance.set_source(path, Source::Cli);
        }
        provenance.add_route(path, route.clone());
    }

    let dotenv = parse_dotenv::<C>(&set, sources.dotenv(), &routes, &mut provenance)?;
    sources.resolve(cli.or(layer(&dotenv)), provenance, ssh::home_dir().as_deref())
}

/// Returns the config path and route of every setting flag of `command`.
///
/// A setting flag's clap id is its config path; `--config`, `--env`, and
/// `--help` are not settings.
fn flag_routes(command: &clap::Command) -> Vec<(String, Route)> {
    command
        .get_arguments()
        .filter(|arg| !matches!(arg.get_id().as_str(), "config" | "env" | "help"))
        .filter_map(|arg| {
            let route = Route {
                flag: format!("--{}", arg.get_long()?),
                env: arg.get_env().and_then(OsStr::to_str).map(str::to_owned),
            };
            Some((arg.get_id().to_string(), route))
        })
        .collect()
}

/// Returns the command of `C` without binary name and with every env var binding removed.
///
/// Parsing `--flag=value` arguments with it sets exactly those flags,
/// whatever the process environment contains.
fn bare_command<C: CommandFactory>() -> clap::Command {
    C::command()
        .no_binary_name(true)
        .mut_args(|arg| arg.env(None::<&'static str>))
}

/// Parses the dotenv value of every flag whose config path is not in `set`.
///
/// Each value is parsed as `--<flag>=<value>`, so it goes through exactly
/// the parser its env var would, and gets a [`Source::Dotenv`] in
/// `provenance`. Returns all of them parsed together as a `C`.
///
/// # Errors
///
/// Returns [`LoadError::Env`] listing every invalid value from the first
/// env file that has one.
fn parse_dotenv<C: CommandFactory + FromArgMatches>(
    set: &[&str],
    dotenv: &Dotenv,
    routes: &[(String, Route)],
    provenance: &mut Provenance,
) -> Result<C, LoadError> {
    let mut bare = bare_command::<C>();
    let mut args = Vec::new();
    let mut errors: Vec<(PathBuf, String)> = Vec::new();
    for (path, route) in routes {
        if set.contains(&path.as_str()) {
            continue;
        }
        let Some((name, var)) = route
            .env
            .as_deref()
            .and_then(|name| dotenv.get(name).map(|var| (name, var)))
        else {
            continue;
        };
        let arg = format!("{}={}", route.flag, var.value);
        if bare.try_get_matches_from_mut([&arg]).is_err() {
            let error = format!("{}: {name}: invalid value for {}", var.location(), route.flag);
            errors.push((var.path.clone(), error));
            continue;
        }
        let source = Source::Dotenv {
            name: name.to_owned(),
            location: var.location(),
        };
        provenance.set_source(path, source);
        args.push(arg);
    }

    if let Some((first, _)) = errors.first() {
        let path = first.clone();
        let errors = errors
            .into_iter()
            .filter(|(p, _)| *p == path)
            .map(|(_, error)| error)
            .collect();
        return Err(LoadError::Env { path, errors });
    }
    let matches = bare
        .try_get_matches_from_mut(args)
        .expect("each value parsed on its own and no flags conflict");
    Ok(C::from_arg_matches(&matches).expect("matches were parsed by `C::command()`"))
}

/// Test helpers shared by the subcommand tests.
#[cfg(test)]
pub(crate) mod tests_support {
    use std::path::PathBuf;

    use clap::{CommandFactory, FromArgMatches};
    use dbmcp_config::ConfigLayer;

    use super::{bare_command, flag_routes};

    /// Removes every env var bound to a flag of `C`, except those in `keep`.
    pub(crate) fn clear_env<C: CommandFactory>(keep: &[&str]) {
        for arg in C::command().get_arguments() {
            if let Some(env) = arg.get_env().filter(|env| !keep.iter().any(|keep| env == keep)) {
                // SAFETY: tests in this crate only remove these env vars, never set them.
                unsafe {
                    std::env::remove_var(env);
                }
            }
        }
    }

    /// Writes `yaml` to a fresh `dbmcp.yaml` and returns its directory and path.
    pub(crate) fn config_file(yaml: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("dbmcp.yaml");
        std::fs::write(&path, yaml).expect("write config");
        (dir, path)
    }

    /// Asserts every setting flag of `C` is routed and sets exactly its config path.
    ///
    /// Each flag is parsed alone with a sample value (its first possible
    /// value, or `1`).
    pub(crate) fn check_routes<C: CommandFactory + FromArgMatches>(layer: fn(&C) -> ConfigLayer) {
        let command = C::command();
        let routes = flag_routes(&command);
        let mut bare = bare_command::<C>();
        for arg in command.get_arguments() {
            let Some(long) = arg.get_long() else { continue };
            if ["config", "env", "help"].contains(&long) {
                continue;
            }
            let flag = format!("--{long}");
            let (path, _) = routes
                .iter()
                .find(|(_, route)| route.flag == flag)
                .unwrap_or_else(|| panic!("{flag} has no route"));
            let sample = arg
                .get_possible_values()
                .first()
                .map_or_else(|| "1".to_owned(), |value| value.get_name().to_owned());
            let matches = bare
                .try_get_matches_from_mut([format!("{flag}={sample}")])
                .unwrap_or_else(|e| panic!("{flag}={sample} must parse: {e}"));
            let cli = C::from_arg_matches(&matches).expect("from matches");
            assert_eq!(layer(&cli).set_paths(), [path.as_str()], "{flag}");
        }
    }
}

/// Shared database connection flags embedded in transport subcommands.
#[derive(Debug, Args)]
#[command(next_help_heading = "Database")]
pub(crate) struct DatabaseArgs {
    /// Database backend [default: mysql]
    #[arg(id = "database.backend", long = "db-backend", env = "DB_BACKEND")]
    pub(crate) backend: Option<DatabaseBackend>,

    /// Database host (as seen from the SSH server when tunnelling) [default: localhost]
    #[arg(id = "database.host", long = "db-host", env = "DB_HOST")]
    pub(crate) host: Option<String>,

    /// Database port (default: backend-dependent)
    #[arg(id = "database.port", long = "db-port", env = "DB_PORT")]
    pub(crate) port: Option<u16>,

    /// Database user (default: backend-dependent)
    #[arg(id = "database.user", long = "db-user", env = "DB_USER")]
    pub(crate) user: Option<String>,

    /// Database password
    #[arg(
        id = "database.password",
        long = "db-password",
        env = "DB_PASSWORD",
        hide_env_values = true
    )]
    pub(crate) password: Option<SecretString>,

    /// Database name or `SQLite` file path
    #[arg(id = "database.name", long = "db-name", env = "DB_NAME")]
    pub(crate) name: Option<String>,

    /// Character set (MySQL/MariaDB only)
    #[arg(id = "database.charset", long = "db-charset", env = "DB_CHARSET")]
    pub(crate) charset: Option<String>,

    /// Enable SSL for database connection [default: false]
    #[arg(
        id = "database.ssl", long = "db-ssl",
        env = "DB_SSL",
        action = ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new(),
    )]
    pub(crate) ssl: Option<bool>,

    /// Path to CA certificate
    #[arg(id = "database.ssl_ca", long = "db-ssl-ca", env = "DB_SSL_CA")]
    pub(crate) ssl_ca: Option<String>,

    /// Path to client certificate
    #[arg(id = "database.ssl_cert", long = "db-ssl-cert", env = "DB_SSL_CERT")]
    pub(crate) ssl_cert: Option<String>,

    /// Path to a client key
    #[arg(id = "database.ssl_key", long = "db-ssl-key", env = "DB_SSL_KEY")]
    pub(crate) ssl_key: Option<String>,

    /// Verify server certificate [default: true]
    #[arg(
        id = "database.ssl_verify_cert", long = "db-ssl-verify-cert",
        env = "DB_SSL_VERIFY_CERT",
        action = ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new(),
    )]
    pub(crate) ssl_verify_cert: Option<bool>,

    /// Enable read-only mode [default: true]
    #[arg(
        id = "database.read_only", long = "db-read-only",
        env = "DB_READ_ONLY",
        action = ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new(),
    )]
    pub(crate) read_only: Option<bool>,

    /// Maximum connection pool size [default: 5]
    #[arg(id = "database.max_pool_size", long = "db-max-pool-size", env = "DB_MAX_POOL_SIZE")]
    pub(crate) max_pool_size: Option<u32>,

    /// Connection timeout in seconds
    #[arg(
        id = "database.connection_timeout",
        long = "db-connection-timeout",
        env = "DB_CONNECTION_TIMEOUT"
    )]
    pub(crate) connection_timeout: Option<u64>,

    /// Query execution timeout in seconds [default: 30]
    #[arg(id = "database.query_timeout", long = "db-query-timeout", env = "DB_QUERY_TIMEOUT")]
    pub(crate) query_timeout: Option<u64>,

    /// Maximum items returned in a single paginated tool response [default: 100]
    #[arg(id = "database.page_size", long = "db-page-size", env = "DB_PAGE_SIZE")]
    pub(crate) page_size: Option<u16>,
}

impl DatabaseArgs {
    /// Returns the set database flags as a layer.
    pub(crate) fn layer(&self) -> DatabaseLayer {
        DatabaseLayer {
            backend: self.backend,
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            password: self.password.clone(),
            name: self.name.clone(),
            charset: self.charset.clone(),
            ssl: self.ssl,
            ssl_ca: self.ssl_ca.clone(),
            ssl_cert: self.ssl_cert.clone(),
            ssl_key: self.ssl_key.clone(),
            ssl_verify_cert: self.ssl_verify_cert,
            read_only: self.read_only,
            max_pool_size: self.max_pool_size,
            connection_timeout: self.connection_timeout,
            query_timeout: self.query_timeout,
            page_size: self.page_size,
        }
    }
}

/// Shared PII flags embedded in transport subcommands.
#[derive(Debug, Args)]
#[command(next_help_heading = "PII")]
pub(crate) struct PiiArgs {
    /// Enable PII redaction of query tool output [default: false]
    #[arg(
        id = "pii.enabled", long = "pii",
        env = "PII_ENABLE",
        action = ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new(),
    )]
    pub(crate) enabled: Option<bool>,

    /// Operator applied to detected PII spans [default: replace]
    #[arg(id = "pii.operator", long = "pii-operator", env = "PII_OPERATOR")]
    pub(crate) operator: Option<PiiOperator>,

    /// Comma-separated PII categories the analyzer should cover
    /// (personal, financial, government, contact, network, digital-identity, crypto)
    #[arg(
        id = "pii.categories", long = "pii-categories",
        env = "PII_CATEGORIES",
        value_delimiter = ',',
        num_args = 1..,
    )]
    pub(crate) categories: Option<Vec<PiiCategory>>,

    /// Enable the optional ML/NER pass (person & location detection) [default: false]
    #[arg(
        id = "pii.ner_enabled", long = "pii-ner",
        env = "PII_NER_ENABLE",
        action = ArgAction::Set, num_args = 0..=1, default_missing_value = "true", value_parser = BoolishValueParser::new(),
    )]
    pub(crate) ner_enabled: Option<bool>,

    /// Path to the NER model directory (required with --pii-ner)
    #[arg(id = "pii.ner_model", long = "pii-ner-model", env = "PII_NER_MODEL")]
    pub(crate) ner_model: Option<PathBuf>,
}

impl PiiArgs {
    /// Returns the set PII flags as a layer.
    pub(crate) fn layer(&self) -> PiiLayer {
        PiiLayer {
            enabled: self.enabled,
            operator: self.operator,
            categories: self.categories.clone(),
            ner_enabled: self.ner_enabled,
            ner_model: self.ner_model.clone(),
        }
    }
}

/// Shared SSH tunnel flags embedded in transport subcommands.
///
/// Like every setting flag, each argument's clap id is its config path
/// (`ssh.host`), which also keeps it apart from [`DatabaseArgs`]. Every
/// option stays `Option`: setting the host enables the tunnel, and the
/// defaults are applied by [`ConfigLayer::resolve`].
#[derive(Debug, Args)]
#[command(next_help_heading = "SSH")]
pub(crate) struct SshArgs {
    /// SSH server host; enables the tunnel when set
    #[arg(id = "ssh.host", long = "ssh-host", value_name = "HOST", env = "SSH_HOST")]
    pub(crate) host: Option<String>,

    /// SSH server port
    #[arg(id = "ssh.port", long = "ssh-port", value_name = "PORT", env = "SSH_PORT")]
    pub(crate) port: Option<u16>,

    /// SSH user
    #[arg(id = "ssh.user", long = "ssh-user", value_name = "USER", env = "SSH_USER")]
    pub(crate) user: Option<String>,

    /// SSH password
    #[arg(
        id = "ssh.password",
        long = "ssh-password",
        value_name = "PASSWORD",
        env = "SSH_PASSWORD",
        hide_env_values = true
    )]
    pub(crate) password: Option<SecretString>,

    /// Path to SSH private key (OpenSSH, PKCS#1, PKCS#8, SEC1)
    #[arg(id = "ssh.key", long = "ssh-key", value_name = "PATH", env = "SSH_KEY")]
    pub(crate) key: Option<PathBuf>,

    /// Passphrase for the SSH private key
    #[arg(
        id = "ssh.key_passphrase",
        long = "ssh-key-passphrase",
        value_name = "PASSPHRASE",
        env = "SSH_KEY_PASSPHRASE",
        hide_env_values = true
    )]
    pub(crate) key_passphrase: Option<SecretString>,

    /// Path to known_hosts file (default: ~/.ssh/known_hosts)
    #[allow(clippy::doc_markdown, reason = "doc comment is the verbatim --help text")]
    #[arg(
        id = "ssh.known_hosts",
        long = "ssh-known-hosts",
        value_name = "PATH",
        env = "SSH_KNOWN_HOSTS"
    )]
    pub(crate) known_hosts: Option<PathBuf>,

    /// Host key policy: yes rejects unknown hosts, accept-new trusts on first use, no disables verification (insecure)
    #[arg(
        id = "ssh.strict_host_key_checking",
        long = "ssh-strict-host-key-checking",
        value_name = "MODE",
        env = "SSH_STRICT_HOST_KEY_CHECKING"
    )]
    pub(crate) strict_host_key_checking: Option<StrictHostKeyChecking>,

    /// SSH keepalive interval in seconds (0 disables)
    #[arg(
        id = "ssh.keepalive_interval",
        long = "ssh-keepalive-interval",
        value_name = "SECONDS",
        env = "SSH_KEEPALIVE_INTERVAL"
    )]
    pub(crate) keepalive_interval: Option<u64>,

    /// Local tunnel port (default: ephemeral)
    #[arg(
        id = "ssh.local_port",
        long = "ssh-local-port",
        value_name = "PORT",
        env = "SSH_LOCAL_PORT"
    )]
    pub(crate) local_port: Option<u16>,
}

impl SshArgs {
    /// Returns the set SSH flags as a layer.
    pub(crate) fn layer(&self) -> SshLayer {
        SshLayer {
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            password: self.password.clone(),
            key: self.key.clone(),
            key_passphrase: self.key_passphrase.clone(),
            known_hosts: self.known_hosts.clone(),
            strict_host_key_checking: self.strict_host_key_checking,
            keepalive_interval: self.keepalive_interval,
            local_port: self.local_port,
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::{DatabaseArgs, PiiArgs, SshArgs};
    use dbmcp_config::{
        Config, ConfigError, ConfigErrors, ConfigLayer, DatabaseBackend, PiiCategory, PiiOperator,
        StrictHostKeyChecking,
    };

    #[derive(Debug, Parser)]
    #[command(no_binary_name = true)]
    struct TestCli {
        #[command(flatten)]
        db: DatabaseArgs,
        #[command(flatten)]
        pii: PiiArgs,
        #[command(flatten)]
        ssh: SshArgs,
    }

    /// Removes every env var bound to a [`TestCli`] flag.
    fn clear_env() {
        super::tests_support::clear_env::<TestCli>(&[]);
    }

    fn try_parse(args: &[&str]) -> Result<TestCli, clap::Error> {
        clear_env();
        TestCli::try_parse_from(args)
    }

    /// Parses `args` and converts them into the CLI layer.
    fn layer(args: &[&str]) -> ConfigLayer {
        clear_env();
        let cli = TestCli::try_parse_from(args).unwrap_or_else(|e| panic!("{args:?} must parse: {e}"));
        ConfigLayer {
            database: cli.db.layer(),
            pii: cli.pii.layer(),
            ssh: cli.ssh.layer(),
            ..ConfigLayer::default()
        }
    }

    fn resolve_with_home(args: &[&str], home: Option<&Path>) -> Result<Config, ConfigErrors> {
        layer(args).resolve(home).map(|resolved| resolved.config)
    }

    fn resolve(args: &[&str]) -> Result<Config, ConfigErrors> {
        resolve_with_home(args, Some(&home()))
    }

    fn home() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\deploy")
        } else {
            PathBuf::from("/home/deploy")
        }
    }

    #[test]
    fn defaults_are_blanked_from_layer() {
        let layer = layer(&[]);
        assert_eq!(
            format!("{layer:?}"),
            format!("{:?}", ConfigLayer::default()),
            "no flag set must yield an empty layer"
        );
    }

    #[test]
    fn every_flag_reaches_the_layer() {
        let empty = format!("{:?}", ConfigLayer::default());
        for arg in TestCli::command().get_arguments() {
            let Some(long) = arg.get_long().filter(|long| *long != "help") else {
                continue;
            };
            let value = arg
                .get_possible_values()
                .first()
                .map_or_else(|| "1".to_owned(), |value| value.get_name().to_owned());
            let flag = format!("--{long}={value}");
            assert_ne!(format!("{:?}", layer(&[&flag])), empty, "{flag} must reach the layer");
        }
    }

    #[test]
    fn explicit_values_reach_the_resolved_config() {
        let config = resolve(&[
            "--db-backend",
            "postgres",
            "--db-user",
            "pg",
            "--db-name",
            "app",
            "--db-read-only=false",
            "--db-query-timeout",
            "0",
        ])
        .expect("valid");
        assert_eq!(config.database.backend, DatabaseBackend::Postgres);
        assert_eq!(config.database.port, 5432);
        assert_eq!(config.database.user, "pg");
        assert_eq!(config.database.name.as_deref(), Some("app"));
        assert!(!config.database.read_only);
        assert_eq!(config.database.query_timeout, 0);
    }

    #[test]
    fn page_size_range_is_validated_after_merge() {
        for value in ["0", "501"] {
            let errors = resolve(&["--db-page-size", value]).expect_err("out of range");
            assert!(
                errors
                    .iter()
                    .any(|e| matches!(e, ConfigError::OutOfRange { field, .. } if *field == "database.page_size")),
                "{value}: {errors:?}"
            );
        }
        for value in ["1", "500"] {
            let config = resolve(&["--db-page-size", value]).expect("in range");
            assert_eq!(config.database.page_size.to_string(), value);
        }
        assert_eq!(resolve(&[]).expect("default").database.page_size, 100);
    }

    #[test]
    fn clap_rejects_non_integer_page_size() {
        for value in ["-1", "abc"] {
            assert!(try_parse(&["--db-page-size", value]).is_err(), "{value}");
        }
    }

    #[test]
    fn zero_pool_size_and_connection_timeout_are_rejected() {
        let errors = resolve(&["--db-max-pool-size", "0", "--db-connection-timeout", "0"]).expect_err("zero");
        let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert_eq!(
            messages,
            [
                "database.max_pool_size: must be at least 1",
                "database.connection_timeout: must be at least 1"
            ]
        );
    }

    #[test]
    fn clap_pii_operator_default_is_replace() {
        let config = resolve(&[]).expect("valid");
        assert!(!config.pii.enabled);
        assert_eq!(config.pii.operator, PiiOperator::Replace);
    }

    #[test]
    fn clap_pii_operator_accepts_each_variant() {
        for (raw, expected) in [
            ("replace", PiiOperator::Replace),
            ("mask", PiiOperator::Mask),
            ("redact", PiiOperator::Redact),
            ("hash", PiiOperator::Hash),
        ] {
            let cli = try_parse(&["--pii-operator", raw]).expect("valid operator parses");
            assert_eq!(cli.pii.operator, Some(expected), "operator {raw} must parse");
        }
    }

    #[test]
    fn clap_rejects_unknown_pii_operator() {
        let err = try_parse(&["--pii-operator", "xyz"]).expect_err("unknown operator must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("xyz"), "error must name the offending value: {msg}");
        for accepted in ["replace", "mask", "redact", "hash"] {
            assert!(
                msg.contains(accepted),
                "error must list accepted value '{accepted}': {msg}"
            );
        }
    }

    #[test]
    fn sqlite_without_db_name_is_rejected() {
        let errors = resolve(&["--db-backend", "sqlite"]).expect_err("sqlite without name must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSqliteDbName)));
    }

    #[test]
    fn missing_ssl_cert_files_are_rejected() {
        let errors = resolve(&[
            "--db-ssl",
            "--db-ssl-ca",
            "/nonexistent/ca.pem",
            "--db-ssl-cert",
            "/nonexistent/cert.pem",
            "--db-ssl-key",
            "/nonexistent/key.pem",
        ])
        .expect_err("missing ssl cert files must fail");
        let cert_errors = errors
            .iter()
            .filter(|e| matches!(e, ConfigError::FileNotFound { .. }))
            .count();
        assert_eq!(cert_errors, 3, "expected three FileNotFound errors, got {errors:?}");
    }

    #[test]
    fn sqlite_name_and_ssl_errors_accumulate() {
        let errors = resolve(&[
            "--db-backend",
            "sqlite",
            "--db-ssl",
            "--db-ssl-ca",
            "/nonexistent/ca.pem",
        ])
        .expect_err("multiple errors must accumulate");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSqliteDbName)));
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::FileNotFound { field: name, path } if *name == "database.ssl_ca" && path == "/nonexistent/ca.pem"))
        );
    }

    #[test]
    fn clap_pii_flags_grouped_under_pii_heading() {
        let help = TestCli::command().render_help().to_string();
        let pii_pos = help.find("PII:").expect("PII heading must be present in --help");
        let flag_pos = help.find("--pii ").expect("--pii must be present in --help");
        assert!(
            pii_pos < flag_pos,
            "PII heading must precede --pii in help output:\n{help}"
        );
    }

    #[test]
    fn clap_pii_categories_default_unset() {
        assert!(resolve(&[]).expect("valid").pii.categories.is_none());
    }

    #[test]
    fn clap_pii_categories_comma_separated() {
        let config = resolve(&["--pii-categories", "financial,government"]).expect("valid");
        assert_eq!(
            config.pii.categories,
            Some(vec![PiiCategory::Financial, PiiCategory::Government])
        );
    }

    #[test]
    fn clap_pii_categories_kebab_digital_identity() {
        let config = resolve(&["--pii-categories", "digital-identity"]).expect("valid");
        assert_eq!(config.pii.categories, Some(vec![PiiCategory::DigitalIdentity]));
    }

    #[test]
    fn clap_rejects_unknown_pii_category() {
        let err = try_parse(&["--pii-categories", "healthcare"]).expect_err("healthcare is not in v1");
        let msg = err.to_string();
        assert!(msg.contains("healthcare"), "error must name the offending value: {msg}");
    }

    #[test]
    fn duplicate_pii_categories_are_rejected() {
        let errors = resolve(&["--pii-categories", "financial,financial"]).expect_err("duplicate");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::DuplicateListEntry { .. }))
        );
    }

    #[test]
    fn clap_pii_ner_default_off() {
        let config = resolve(&[]).expect("valid");
        assert!(!config.pii.ner_enabled, "ner must default off");
        assert!(config.pii.ner_model.is_none(), "ner model must default unset");
    }

    #[test]
    fn clap_pii_ner_flags_parse_and_convert() {
        let config = resolve(&["--pii-ner", "true", "--pii-ner-model", "/models/ner"]).expect("ner config validates");
        assert!(config.pii.ner_enabled);
        assert_eq!(config.pii.ner_model, Some(PathBuf::from("/models/ner")));
    }

    #[test]
    fn pii_ner_enabled_without_model_is_rejected() {
        let errors = resolve(&["--pii-ner", "true"]).expect_err("ner without model must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::PiiNerModelMissing)));
    }

    #[test]
    fn ssh_disabled_without_any_option() {
        assert!(resolve(&[]).expect("valid").ssh.is_none());
    }

    #[test]
    fn ssh_options_without_host_are_rejected() {
        for args in [
            &["--ssh-port", "2222"][..],
            &["--ssh-user", "deploy"],
            &["--ssh-password", "pw"],
            &["--ssh-key", "/k"],
            &["--ssh-key-passphrase", "pp"],
            &["--ssh-known-hosts", "/kh"],
            &["--ssh-strict-host-key-checking", "no"],
            &["--ssh-keepalive-interval", "0"],
            &["--ssh-local-port", "4000"],
        ] {
            let errors = resolve(args).expect_err("orphan option must fail");
            assert_eq!(errors.len(), 1, "{args:?}");
            assert_eq!(
                errors[0].to_string(),
                "ssh.host: required when other ssh options are set",
                "{args:?}"
            );
        }
    }

    #[test]
    fn ssh_defaults_are_resolved() {
        let config = resolve(&["--ssh-host", "bastion", "--ssh-user", "deploy", "--ssh-password", "pw"])
            .expect("valid")
            .ssh
            .expect("tunnel enabled");
        assert_eq!(config.host, "bastion");
        assert_eq!(config.port, 22);
        assert_eq!(config.user, "deploy");
        assert_eq!(
            config.password.as_ref().map(dbmcp_config::ExposeSecret::expose_secret),
            Some("pw")
        );
        assert_eq!(config.known_hosts, Some(home().join(".ssh").join("known_hosts")));
        assert_eq!(config.strict_host_key_checking, StrictHostKeyChecking::Yes);
        assert_eq!(config.keepalive_interval, Duration::from_secs(30));
        assert_eq!(config.local_port, None);
    }

    #[test]
    fn ssh_explicit_values_override_defaults() {
        let config = resolve(&[
            "--ssh-host",
            "bastion",
            "--ssh-port",
            "2222",
            "--ssh-user",
            "deploy",
            "--ssh-password",
            "pw",
            "--ssh-known-hosts",
            "~/.ssh/custom_hosts",
            "--ssh-strict-host-key-checking",
            "accept-new",
            "--ssh-keepalive-interval",
            "0",
            "--ssh-local-port",
            "4000",
        ])
        .expect("valid")
        .ssh
        .expect("tunnel enabled");
        assert_eq!(config.port, 2222);
        assert_eq!(config.known_hosts, Some(home().join(".ssh").join("custom_hosts")));
        assert_eq!(config.strict_host_key_checking, StrictHostKeyChecking::AcceptNew);
        assert_eq!(config.keepalive_interval, Duration::ZERO);
        assert_eq!(config.local_port, Some(4000));
    }

    #[test]
    fn ssh_key_path_is_tilde_expanded() {
        let errors = resolve(&[
            "--ssh-host",
            "bastion",
            "--ssh-user",
            "deploy",
            "--ssh-key",
            "~/.ssh/missing_key",
        ])
        .expect_err("missing key file must fail");
        let expected = home().join(".ssh").join("missing_key");
        assert!(
            errors.iter().any(|e| matches!(
                e,
                ConfigError::FileNotFound { field: name, path } if *name == "ssh.key" && *path == expected.display().to_string()
            )),
            "{errors:?}"
        );
    }

    #[test]
    fn ssh_known_hosts_unresolved_without_home() {
        let errors = resolve_with_home(
            &["--ssh-host", "bastion", "--ssh-user", "deploy", "--ssh-password", "pw"],
            None,
        )
        .expect_err("no home must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSshKnownHosts)));

        let config = resolve_with_home(
            &[
                "--ssh-host",
                "bastion",
                "--ssh-user",
                "deploy",
                "--ssh-password",
                "pw",
                "--ssh-strict-host-key-checking",
                "no",
            ],
            None,
        )
        .expect("mode no needs no known_hosts")
        .ssh
        .expect("tunnel enabled");
        assert_eq!(config.known_hosts, None);
    }

    #[test]
    fn ssh_missing_user_and_auth_accumulate() {
        let errors = resolve(&["--ssh-host", "bastion"]).expect_err("must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSshUser)));
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSshAuth)));
    }

    #[test]
    fn ssh_rejected_for_sqlite_backend() {
        let errors = resolve(&[
            "--db-backend",
            "sqlite",
            "--db-name",
            ":memory:",
            "--ssh-host",
            "bastion",
            "--ssh-user",
            "deploy",
            "--ssh-password",
            "pw",
        ])
        .expect_err("sqlite must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::SshSqliteUnsupported)));
    }

    #[test]
    fn ssh_empty_host_is_rejected() {
        let errors = resolve(&["--ssh-host", "", "--ssh-user", "deploy", "--ssh-password", "pw"])
            .expect_err("empty host must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::EmptySshHost)));
    }

    #[test]
    fn ssh_port_zero_and_local_port_zero_are_rejected() {
        let base = ["--ssh-host", "bastion", "--ssh-user", "deploy", "--ssh-password", "pw"];
        for (flag, field) in [("--ssh-port", "ssh.port"), ("--ssh-local-port", "ssh.local_port")] {
            let mut args = base.to_vec();
            args.extend([flag, "0"]);
            let errors = resolve(&args).expect_err("zero port");
            assert!(
                errors
                    .iter()
                    .any(|e| matches!(e, ConfigError::BelowMinimum { field: f, .. } if *f == field)),
                "{flag}: {errors:?}"
            );
        }
    }

    #[test]
    fn clap_strict_host_key_checking_rejects_openssh_aliases() {
        for value in ["ask", "off"] {
            assert!(
                try_parse(&["--ssh-strict-host-key-checking", value]).is_err(),
                "{value} must be rejected"
            );
        }
    }

    #[test]
    fn bool_flags_accept_bare_and_explicit_values() {
        let flags = [
            ("--db-ssl", "database.ssl"),
            ("--db-ssl-verify-cert", "database.ssl_verify_cert"),
            ("--db-read-only", "database.read_only"),
            ("--pii", "pii.enabled"),
            ("--pii-ner", "pii.ner_enabled"),
        ];
        for (flag, id) in flags {
            let mut cases = vec![
                (vec![flag.to_owned()], true),
                (vec![flag.to_owned(), "false".to_owned()], false),
            ];
            for value in ["true", "1", "yes", "on", "True", "TRUE"] {
                cases.push((vec![format!("{flag}={value}")], true));
            }
            for value in ["false", "0", "no", "off", "False", "FALSE"] {
                cases.push((vec![format!("{flag}={value}")], false));
            }
            for (args, expected) in cases {
                let matches = TestCli::command()
                    .try_get_matches_from(&args)
                    .unwrap_or_else(|e| panic!("{args:?} must parse: {e}"));
                assert_eq!(matches.get_one::<bool>(id), Some(&expected), "{args:?}");
            }
            for value in ["", "disabled", "nope", "2"] {
                let arg = format!("{flag}={value}");
                assert!(
                    TestCli::command().try_get_matches_from([&arg]).is_err(),
                    "{arg:?} must be rejected"
                );
            }
        }
    }

    #[test]
    fn clap_ssh_flags_grouped_under_ssh_heading() {
        let help = TestCli::command().render_help().to_string();
        let ssh_pos = help.find("SSH:").expect("SSH heading must be present in --help");
        let flag_pos = help.find("--ssh-host").expect("--ssh-host must be present in --help");
        assert!(ssh_pos < flag_pos, "SSH heading must precede --ssh-host:\n{help}");
    }

    #[test]
    fn db_host_help_mentions_ssh_tunnel() {
        let help = TestCli::command().render_help().to_string();
        assert!(
            help.contains("Database host (as seen from the SSH server when tunnelling)"),
            "{help}"
        );
    }
}
