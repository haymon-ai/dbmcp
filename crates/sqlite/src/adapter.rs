//! `SQLite` adapter definition and connection configuration.

use database_mcp_config::DatabaseConfig;
use database_mcp_server::AppError;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tracing::info;

/// `SQLite` file-based database adapter.
#[derive(Clone)]
pub struct SqliteAdapter {
    pub(crate) config: DatabaseConfig,
    pub(crate) pool: SqlitePool,
}

impl std::fmt::Debug for SqliteAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteAdapter")
            .field("read_only", &self.config.read_only)
            .finish_non_exhaustive()
    }
}

impl SqliteAdapter {
    /// Creates a new `SQLite` adapter from configuration.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Connection`] if the database file cannot be opened.
    pub async fn new(config: &DatabaseConfig) -> Result<Self, AppError> {
        let name = config.name.as_deref().unwrap_or_default();
        let pool = SqlitePoolOptions::new()
            .max_connections(1) // SQLite is single-writer
            .connect_with(connect_options(config))
            .await
            .map_err(|e| AppError::Connection(format!("Failed to open SQLite: {e}")))?;

        info!("SQLite connection initialized: {name}");

        Ok(Self {
            config: config.clone(),
            pool,
        })
    }

    /// Wraps `name` in double quotes for safe use in `SQLite` SQL statements.
    pub(crate) fn quote_identifier(name: &str) -> String {
        database_mcp_sql::identifier::quote_identifier(name, '"')
    }
}

/// Builds [`SqliteConnectOptions`] from a [`DatabaseConfig`].
fn connect_options(config: &DatabaseConfig) -> SqliteConnectOptions {
    let name = config.name.as_deref().unwrap_or_default();
    SqliteConnectOptions::new().filename(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use database_mcp_config::DatabaseBackend;

    #[test]
    fn try_from_sets_filename() {
        let config = DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: Some("test.db".into()),
            ..DatabaseConfig::default()
        };
        let opts = connect_options(&config);

        assert_eq!(opts.get_filename().to_str().expect("valid path"), "test.db");
    }

    #[test]
    fn try_from_empty_name_defaults() {
        let config = DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: None,
            ..DatabaseConfig::default()
        };
        let opts = connect_options(&config);

        // Empty string filename — validated elsewhere by Config::validate()
        assert_eq!(opts.get_filename().to_str().expect("valid path"), "");
    }
}
