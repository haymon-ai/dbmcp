//! Stdio transport command.
//!
//! Runs the MCP server over stdin/stdout for use with Claude Desktop,
//! Cursor, and other MCP clients that communicate via stdio.

use clap::{ArgMatches, Parser};
use dbmcp_config::{Config, ConfigLayer, HttpLayer, Transport};
use rmcp::ServiceExt;
use tracing::{error, info};

use crate::commands::common::{
    self, DatabaseArguments, LoadCommand, PiiArguments, SharedArguments, SshArguments, explicit,
};
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

impl LoadCommand for StdioCommand {
    const TRANSPORT: Transport = Transport::Stdio;

    fn shared(&self) -> &SharedArguments {
        &self.shared
    }

    fn layer(&self, matches: &ArgMatches) -> ConfigLayer {
        ConfigLayer {
            log_level: explicit(matches, "log_level", Some(&self.shared.log_level)),
            database: self.database.layer(matches),
            http: HttpLayer::default(),
            pii: self.pii.layer(matches),
            ssh: self.ssh.layer(matches),
        }
    }
}

impl StdioCommand {
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
    use clap::{CommandFactory, FromArgMatches};
    use dbmcp_config::{
        ConfigError, ConfigWarning, DatabaseBackend, DatabaseConfig, LoadError, LogLevel, PiiCategory, Resolved, Secret,
    };

    use super::*;
    use crate::commands::common::tests_support::{self, config_file};

    fn load(args: &[&str]) -> Result<Resolved, LoadError> {
        tests_support::load::<StdioCommand>(args)
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

        std::fs::write(path.with_file_name(".env"), "LOG_LEVEL=debug\n").expect("write .env");
        assert_eq!(
            load(&with_config(&path, &[])).expect("valid").log_level,
            LogLevel::Debug,
            "dotenv beats file"
        );
        // SAFETY: only this test touches LOG_LEVEL.
        unsafe {
            std::env::set_var("LOG_LEVEL", "error");
        }
        let env = load(&with_config(&path, &[])).map(|r| r.log_level);
        // SAFETY: only this test touches LOG_LEVEL.
        unsafe {
            std::env::remove_var("LOG_LEVEL");
        }
        assert_eq!(env.expect("valid"), LogLevel::Error, "process env beats dotenv");
    }

    #[test]
    fn dotenv_beside_config_fills_unset_flags() {
        let (dir, path) = config_file("database:\n  host: file-host\n  user: file-user\n");
        std::fs::write(
            dir.path().join(".env"),
            "DB_PASSWORD=from-dotenv\nDB_HOST=dotenv-host\nDB_READ_ONLY=no\nPII_CATEGORIES=financial,contact\n",
        )
        .expect("write .env");
        let config = load(&with_config(&path, &["--db-host", "flag-host"]))
            .expect("valid")
            .config;
        let db = &config.database;
        assert_eq!(db.password.as_ref().map(Secret::expose), Some("from-dotenv"));
        assert_eq!(db.host, "flag-host", "flag beats dotenv");
        assert_eq!(db.user, "file-user", "file fills what dotenv leaves");
        assert!(!db.read_only, "dotenv values use the env parser");
        assert_eq!(
            config.pii.categories,
            Some(vec![PiiCategory::Financial, PiiCategory::Contact])
        );
    }

    #[test]
    fn env_files_override_in_order_and_replace_default_dotenv() {
        let (dir, path) = config_file("");
        std::fs::write(dir.path().join(".env"), "DB_USER=default\nDB_NAME=default\n").expect("write .env");
        let a = dir.path().join("a.env");
        let b = dir.path().join("b.env");
        std::fs::write(&a, "DB_USER=a\nDB_HOST=a\n").expect("write a");
        std::fs::write(&b, "DB_USER=b\n").expect("write b");
        let (a, b) = (a.to_str().expect("utf-8"), b.to_str().expect("utf-8"));
        let db = load(&with_config(&path, &["--env", a, "--env", b]))
            .expect("valid")
            .config
            .database;
        assert_eq!(db.user, "b");
        assert_eq!(db.host, "a");
        assert_eq!(db.name, None, "default .env is ignored with --env");
    }

    #[test]
    fn invalid_dotenv_value_reports_location_without_value() {
        let (dir, path) = config_file("");
        let env = dir.path().join(".env");
        std::fs::write(&env, "\nDB_PORT=not-a-port\nDB_READ_ONLY=\n").expect("write .env");
        let error = load(&with_config(&path, &[])).expect_err("invalid value");
        assert_eq!(
            error.to_string(),
            format!(
                "failed to load env file {0}:\n  - {0}:2: DB_PORT: invalid value for --db-port\n  - {0}:3: DB_READ_ONLY: invalid value for --db-read-only",
                env.display()
            )
        );
        assert!(!error.to_string().contains("not-a-port"));
    }

    fn validation_message(args: &[&str]) -> String {
        let error = load(args).expect_err("invalid");
        assert!(matches!(error, LoadError::Invalid { .. }), "{error}");
        error.to_string()
    }

    #[test]
    fn unset_field_error_names_every_route() {
        assert_eq!(
            validation_message(&["_", "--db-backend", "sqlite"]),
            "configuration validation failed:\n  - database.name: required (file path) for the sqlite backend; \
             set --db-name, DB_NAME, or database.name in the config file"
        );
    }

    #[test]
    fn flag_and_dotenv_sources_are_named() {
        assert!(validation_message(&["_", "--db-page-size", "0"]).ends_with(" (from --db-page-size)"));

        let (dir, path) = config_file("");
        let env = dir.path().join(".env");
        std::fs::write(&env, "DB_PAGE_SIZE=0\n").expect("write .env");
        let message = validation_message(&with_config(&path, &[]));
        assert!(
            message.ends_with(&format!(" (from DB_PAGE_SIZE in {}:1)", env.display())),
            "{message}"
        );
    }

    #[test]
    fn cross_field_error_lists_file_and_flag_sources() {
        let (_dir, path) = config_file("ssh:\n  host: bastion\n  user: deploy\n  password: pw\n");
        let message = validation_message(&with_config(
            &path,
            &["--db-backend", "sqlite", "--db-name", ":memory:"],
        ));
        assert!(
            message.contains(&format!(
                "ssh.host: not supported with the sqlite backend (ssh.host from {}:2:9; database.backend from --db-backend)",
                path.display()
            )),
            "{message}"
        );
    }

    #[test]
    fn every_setting_flag_has_a_route() {
        tests_support::check_routes::<StdioCommand>();
    }

    #[test]
    fn load_returns_warnings_on_failure() {
        let (dir, path) = config_file("database:\n  pasword: x\n");
        let env = dir.path().join("x.env");
        std::fs::write(&env, "A=${UNSET_IN_TEST}\n").expect("write env");
        tests_support::clear_env::<StdioCommand>(&["LOG_LEVEL"]);
        let args = with_config(&path, &["--env", env.to_str().expect("utf-8")]);
        let matches = StdioCommand::command().try_get_matches_from(&args).expect("parse");
        let cmd = StdioCommand::from_arg_matches(&matches).expect("from matches");
        let mut warnings = Vec::new();
        let result = common::load(&cmd, &matches, &mut warnings);
        assert!(matches!(result, Err(LoadError::Parse { .. })));
        assert_eq!(
            warnings,
            [ConfigWarning::UnsetVariable {
                name: "UNSET_IN_TEST".into(),
                location: format!("{}:1", env.display())
            }]
        );
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
        let LoadError::Invalid { errors, .. } = error else {
            panic!("expected validation error");
        };
        assert_eq!(errors.len(), 5);
        assert!(matches!(errors[0], ConfigError::MissingSqliteDbName));
        assert!(matches!(
            &errors[1],
            ConfigError::FileNotFound {
                field: "database.ssl_ca",
                ..
            }
        ));
        assert!(matches!(
            &errors[2],
            ConfigError::FileNotFound {
                field: "database.ssl_cert",
                ..
            }
        ));
        assert!(matches!(
            &errors[3],
            ConfigError::FileNotFound {
                field: "database.ssl_key",
                ..
            }
        ));
        assert!(matches!(errors[4], ConfigError::PiiNerModelMissing));
    }
}
