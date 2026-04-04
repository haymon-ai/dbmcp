//! MySQL/MariaDB adapter definition and connection configuration.
//!
//! Builds [`MySqlConnectOptions`] from a [`DatabaseConfig`] and checks
//! for dangerous server privileges on startup.

use database_mcp_config::DatabaseConfig;
use database_mcp_server::AppError;
use sqlx::MySqlPool;
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode};
use tracing::{error, info};

/// MySQL/MariaDB database adapter.
#[derive(Clone)]
pub struct MysqlAdapter {
    pub(crate) config: DatabaseConfig,
    pub(crate) pool: MySqlPool,
}

impl std::fmt::Debug for MysqlAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MysqlAdapter")
            .field("read_only", &self.config.read_only)
            .finish_non_exhaustive()
    }
}

impl MysqlAdapter {
    /// Creates a new `MySQL` adapter from configuration.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Connection`] if the connection fails.
    pub async fn new(config: &DatabaseConfig) -> Result<Self, AppError> {
        let pool = MySqlPoolOptions::new()
            .max_connections(config.max_pool_size)
            .connect_with(connect_options(config))
            .await
            .map_err(|e| AppError::Connection(format!("Failed to connect to MySQL: {e}")))?;

        info!("MySQL connection pool initialized (max size: {})", config.max_pool_size);

        let backend = Self {
            config: config.clone(),
            pool,
        };

        if backend.config.read_only {
            backend.warn_if_file_privilege().await;
        }

        Ok(backend)
    }

    /// Wraps `name` in backticks for safe use in `MySQL` SQL statements.
    pub(crate) fn quote_identifier(name: &str) -> String {
        database_mcp_sql::identifier::quote_identifier(name, '`')
    }

    /// Wraps a value in single quotes for use as a SQL string literal.
    ///
    /// Escapes internal single quotes by doubling them.
    pub(crate) fn quote_string(value: &str) -> String {
        let escaped = value.replace('\'', "''");
        format!("'{escaped}'")
    }

    async fn warn_if_file_privilege(&self) {
        let result: Result<(), AppError> = async {
            let current_user: Option<String> = sqlx::query_scalar("SELECT CURRENT_USER()")
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;

            let Some(current_user) = current_user else {
                return Ok(());
            };

            let quoted_user = if let Some((user, host)) = current_user.split_once('@') {
                format!("'{user}'@'{host}'")
            } else {
                format!("'{current_user}'")
            };

            let grants: Vec<String> = sqlx::query_scalar(&format!("SHOW GRANTS FOR {quoted_user}"))
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;

            let has_file_priv = grants.iter().any(|grant| {
                let upper = grant.to_uppercase();
                upper.contains("FILE") && upper.contains("ON *.*")
            });

            if has_file_priv {
                error!(
                    "Connected database user has the global FILE privilege. \
                     Revoke FILE for the database user you are connecting as."
                );
            }

            Ok(())
        }
        .await;

        if let Err(e) = result {
            tracing::debug!("Unable to determine whether FILE privilege is enabled: {e}");
        }
    }
}

/// Builds [`MySqlConnectOptions`] from a [`DatabaseConfig`].
fn connect_options(config: &DatabaseConfig) -> MySqlConnectOptions {
    let mut opts = MySqlConnectOptions::new()
        .host(&config.host)
        .port(config.port)
        .username(&config.user);

    if let Some(ref password) = config.password {
        opts = opts.password(password);
    }
    if let Some(ref name) = config.name
        && !name.is_empty()
    {
        opts = opts.database(name);
    }
    if let Some(ref charset) = config.charset {
        opts = opts.charset(charset);
    }

    if config.ssl {
        opts = if config.ssl_verify_cert {
            opts.ssl_mode(MySqlSslMode::VerifyCa)
        } else {
            opts.ssl_mode(MySqlSslMode::Required)
        };
        if let Some(ref ca) = config.ssl_ca {
            opts = opts.ssl_ca(ca);
        }
        if let Some(ref cert) = config.ssl_cert {
            opts = opts.ssl_client_cert(cert);
        }
        if let Some(ref key) = config.ssl_key {
            opts = opts.ssl_client_key(key);
        }
    }

    opts
}

#[cfg(test)]
mod tests {
    use super::*;
    use database_mcp_config::DatabaseBackend;

    fn base_config() -> DatabaseConfig {
        DatabaseConfig {
            backend: DatabaseBackend::Mysql,
            host: "db.example.com".into(),
            port: 3307,
            user: "admin".into(),
            password: Some("s3cret".into()),
            name: Some("mydb".into()),
            ..DatabaseConfig::default()
        }
    }

    #[test]
    fn try_from_basic_config() {
        let config = base_config();
        let opts = connect_options(&config);

        assert_eq!(opts.get_host(), "db.example.com");
        assert_eq!(opts.get_port(), 3307);
        assert_eq!(opts.get_username(), "admin");
        assert_eq!(opts.get_database(), Some("mydb"));
    }

    #[test]
    fn try_from_with_charset() {
        let config = DatabaseConfig {
            charset: Some("utf8mb4".into()),
            ..base_config()
        };
        let opts = connect_options(&config);

        assert_eq!(opts.get_charset(), "utf8mb4");
    }

    #[test]
    fn try_from_with_ssl_required() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_verify_cert: false,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert!(
            matches!(opts.get_ssl_mode(), MySqlSslMode::Required),
            "expected Required, got {:?}",
            opts.get_ssl_mode()
        );
    }

    #[test]
    fn try_from_with_ssl_verify_ca() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_verify_cert: true,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert!(
            matches!(opts.get_ssl_mode(), MySqlSslMode::VerifyCa),
            "expected VerifyCa, got {:?}",
            opts.get_ssl_mode()
        );
    }

    #[test]
    fn try_from_without_password() {
        let config = DatabaseConfig {
            password: None,
            ..base_config()
        };
        let opts = connect_options(&config);

        // Should not panic — password is simply omitted
        assert_eq!(opts.get_host(), "db.example.com");
    }

    #[test]
    fn try_from_without_database_name() {
        let config = DatabaseConfig {
            name: None,
            ..base_config()
        };
        let opts = connect_options(&config);

        assert_eq!(opts.get_database(), None);
    }
}
