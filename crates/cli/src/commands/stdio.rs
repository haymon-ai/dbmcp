//! Stdio transport command.
//!
//! Runs the MCP server over stdin/stdout for use with Claude Desktop,
//! Cursor, and other MCP clients that communicate via stdio.

use clap::{ArgMatches, Parser};
use dbmcp_config::{Config, ConfigLayer, HttpLayer, LoadError, Resolved, Transport};
use rmcp::ServiceExt;
use tracing::{error, info};

use crate::commands::common::{self, DatabaseArguments, PiiArguments, SharedArguments, SshArguments, explicit};
use crate::error::ErrorKind;

/// Runs the MCP server in stdio mode.
#[derive(Debug, Parser)]
#[command(mut_args(common::flag_conventions))]
pub(crate) struct StdioCommand {
    /// Config-file and logging flags.
    #[command(flatten)]
    shared: SharedArguments,

    /// Shared database connection flags.
    #[command(flatten)]
    database: DatabaseArguments,

    /// Shared PII flags.
    #[command(flatten)]
    pii: PiiArguments,

    /// Shared SSH tunnel flags.
    #[command(flatten)]
    ssh: SshArguments,
}

impl StdioCommand {
    /// Returns the flags set on the command line or via env vars as a layer.
    ///
    /// `matches` must be the matches this command was parsed from.
    pub(crate) fn layer(&self, matches: &ArgMatches) -> ConfigLayer {
        ConfigLayer {
            log_level: explicit(matches, "log_level", Some(&self.shared.log_level)),
            database: self.database.layer(matches),
            http: HttpLayer::default(),
            pii: self.pii.layer(matches),
            ssh: self.ssh.layer(matches),
        }
    }

    /// Merges these flags over the `--config` file and resolves the result.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] when loading or validation fails.
    pub(crate) fn load(&self, matches: &ArgMatches) -> Result<Resolved, LoadError> {
        common::load(&self.shared, self.layer(matches), Transport::Stdio)
    }

    /// Builds the server for `config` and runs the stdio transport.
    ///
    /// Serves JSON-RPC over stdin/stdout.
    ///
    /// # Errors
    ///
    /// Returns an error if the server cannot be built, the stdio
    /// transport fails to initialize, or the server encounters a fatal
    /// protocol error.
    pub(crate) async fn execute(config: Config) -> Result<(), ErrorKind> {
        let (tunnel, server) = common::create_server(&config).await?;

        info!("Starting MCP server via stdio transport...");
        let transport = rmcp::transport::io::stdio();
        let running = server.clone().serve(transport).await?;

        // Exit on stdin EOF (client disconnect) or Ctrl-C/SIGTERM. On a
        // signal the losing `waiting()` future drops, cancelling the
        // service via `RunningService`'s `Drop`.
        tokio::select! {
            res = running.waiting() => {
                if let Err(join_error) = res {
                    error!("stdio server task terminated abnormally: {join_error}");
                }
            }
            () = common::shutdown_signal() => {}
        }

        server.shutdown().await;
        info!("All connection pools closed.");

        // Pools close through the tunnel, so it must outlive the server.
        drop(server);
        drop(tunnel);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::{CommandFactory, FromArgMatches};
    use dbmcp_config::{ConfigError, DatabaseBackend, DatabaseConfig, LogLevel};

    use super::*;

    /// Removes every env var bound to a `stdio` flag except `LOG_LEVEL`.
    fn clear_env() {
        for arg in StdioCommand::command().get_arguments() {
            if let Some(env) = arg.get_env().filter(|env| *env != "LOG_LEVEL") {
                // SAFETY: tests in this crate only remove these env vars, never set them.
                unsafe {
                    std::env::remove_var(env);
                }
            }
        }
    }

    fn load(args: &[&str]) -> Result<Resolved, LoadError> {
        clear_env();
        let matches = StdioCommand::command()
            .try_get_matches_from(args)
            .unwrap_or_else(|e| panic!("{args:?} must parse: {e}"));
        StdioCommand::from_arg_matches(&matches)
            .expect("from matches")
            .load(&matches)
    }

    /// Writes `yaml` to a fresh `dbmcp.yaml` and returns its directory and path.
    fn config_file(yaml: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("dbmcp.yaml");
        std::fs::write(&path, yaml).expect("write config");
        (dir, path)
    }

    fn with_config<'a>(path: &'a std::path::Path, args: &[&'a str]) -> Vec<&'a str> {
        let mut all = vec!["_", "--config", path.to_str().expect("utf-8 path")];
        all.extend(args);
        all
    }

    #[test]
    fn db_read_only_defaults_to_true() {
        assert!(load(&["_"]).expect("valid").config.database.read_only);
    }

    #[test]
    fn db_query_timeout_default_matches_database_config_default() {
        let config = load(&["_"]).expect("valid").config;
        assert_eq!(config.database.query_timeout, DatabaseConfig::default().query_timeout);
    }

    #[test]
    fn file_value_applies_when_flag_is_unset() {
        let (_dir, path) = config_file("database:\n  backend: postgres\n  host: file-host\n  read_only: false\n");
        let db = load(&with_config(&path, &[])).expect("valid").config.database;
        assert_eq!(db.backend, DatabaseBackend::Postgres);
        assert_eq!(db.host, "file-host");
        assert_eq!(db.port, 5432, "default follows the file backend");
        assert!(!db.read_only, "file overrides a defaulted bool flag");
    }

    #[test]
    fn flag_beats_file_value() {
        let (_dir, path) = config_file("database:\n  host: file-host\n  read_only: false\n");
        let db = load(&with_config(&path, &["--db-host", "flag-host", "--db-read-only"]))
            .expect("valid")
            .config
            .database;
        assert_eq!(db.host, "flag-host");
        assert!(db.read_only);
    }

    #[test]
    fn default_applies_when_neither_layer_sets_value() {
        let (_dir, path) = config_file("database:\n  user: app\n");
        let db = load(&with_config(&path, &[])).expect("valid").config.database;
        assert_eq!(db.host, DatabaseConfig::DEFAULT_HOST);
        assert_eq!(db.page_size, DatabaseConfig::DEFAULT_PAGE_SIZE);
    }

    #[test]
    fn log_level_precedence_is_flag_then_env_then_file_then_default() {
        let (_dir, path) = config_file("log_level: warn\n");
        // SAFETY: only this test touches LOG_LEVEL.
        unsafe {
            std::env::remove_var("LOG_LEVEL");
        }
        assert_eq!(load(&["_"]).expect("valid").log_level, LogLevel::Info);
        assert_eq!(load(&with_config(&path, &[])).expect("valid").log_level, LogLevel::Warn);

        // SAFETY: only this test touches LOG_LEVEL.
        unsafe {
            std::env::set_var("LOG_LEVEL", "error");
        }
        let env = load(&with_config(&path, &[])).map(|r| r.log_level);
        let flag = load(&with_config(&path, &["--log-level", "TRACE"])).map(|r| r.log_level);
        // SAFETY: only this test touches LOG_LEVEL.
        unsafe {
            std::env::remove_var("LOG_LEVEL");
        }
        assert_eq!(env.expect("valid"), LogLevel::Error, "env beats file");
        assert_eq!(flag.expect("valid"), LogLevel::Trace, "flag beats env");
    }

    #[test]
    fn http_section_is_ignored_under_stdio() {
        let (_dir, path) = config_file("http:\n  host: ''\n");
        let config = load(&with_config(&path, &[])).expect("stdio ignores http").config;
        assert!(config.http.is_none());
    }

    #[test]
    fn config_flag_cannot_be_repeated() {
        let error = StdioCommand::command()
            .try_get_matches_from(["_", "--config", "a.yaml", "--config", "b.yaml"])
            .expect_err("repeated --config must fail");
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn unknown_file_key_is_rejected() {
        let (_dir, path) = config_file("database:\n  pasword: x\n");
        let error = load(&with_config(&path, &[])).expect_err("unknown key");
        assert!(matches!(error, LoadError::Parse { .. }), "{error}");
    }

    #[test]
    fn missing_config_file_is_a_read_error() {
        let error = load(&["_", "--config", "/nonexistent/dbmcp.yaml"]).expect_err("missing file");
        assert!(matches!(error, LoadError::Read { .. }), "{error}");
    }

    #[test]
    fn validation_errors_accumulate_in_section_order() {
        let error = load(&[
            "_",
            "--db-backend",
            "sqlite",
            "--db-ssl",
            "--db-ssl-ca",
            "/nonexistent/ca.pem",
            "--db-ssl-cert",
            "/nonexistent/cert.pem",
            "--db-ssl-key",
            "/nonexistent/key.pem",
            "--pii-ner",
        ])
        .expect_err("multi-section misconfig must fail");
        let LoadError::Invalid(errors) = error else {
            panic!("expected validation error");
        };
        assert_eq!(errors.len(), 5);
        assert!(matches!(errors[0], ConfigError::MissingSqliteDbName));
        assert!(matches!(&errors[1], ConfigError::FileNotFound(name, _) if name == "DB_SSL_CA"));
        assert!(matches!(&errors[2], ConfigError::FileNotFound(name, _) if name == "DB_SSL_CERT"));
        assert!(matches!(&errors[3], ConfigError::FileNotFound(name, _) if name == "DB_SSL_KEY"));
        assert!(matches!(errors[4], ConfigError::PiiNerModelMissing));
    }
}
