//! `PostgreSQL` backend implementation via sqlx.
//!
//! Implements [`DatabaseBackend`] for `PostgreSQL` databases. Supports
//! cross-database operations by maintaining a concurrent cache of connection
//! pools keyed by database name.

use crate::config::DatabaseConfig;
use crate::db::backend::DatabaseBackend;
use crate::db::identifier::validate_identifier;
use crate::error::AppError;
use moka::future::Cache;
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgRow, PgSslMode};
use sqlx::{PgPool, Row};
use sqlx_to_json::RowExt;
use std::collections::HashMap;
use tracing::info;

/// Maximum number of database connection pools to cache (including the default).
const POOL_CACHE_CAPACITY: u64 = 6;

/// Converts [`DatabaseConfig`] into [`PgConnectOptions`].
///
/// Uses [`PgConnectOptions::new_without_pgpass`] to avoid unintended
/// `PG*` environment variable influence, since our config already
/// resolves values from CLI/env.
impl From<&DatabaseConfig> for PgConnectOptions {
    fn from(config: &DatabaseConfig) -> Self {
        let mut opts = PgConnectOptions::new_without_pgpass()
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

        if config.ssl {
            opts = if config.ssl_verify_cert {
                opts.ssl_mode(PgSslMode::VerifyCa)
            } else {
                opts.ssl_mode(PgSslMode::Require)
            };
            if let Some(ref ca) = config.ssl_ca {
                opts = opts.ssl_root_cert(ca);
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
}

/// `PostgreSQL` database backend.
///
/// All connection pools — including the default — live in a single
/// concurrent cache keyed by database name. No external mutex required.
#[derive(Clone)]
pub struct PostgresBackend {
    config: DatabaseConfig,
    default_db: String,
    pools: Cache<String, PgPool>,
    pub read_only: bool,
}

impl std::fmt::Debug for PostgresBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresBackend")
            .field("read_only", &self.read_only)
            .field("default_db", &self.default_db)
            .finish_non_exhaustive()
    }
}

impl PostgresBackend {
    /// Creates a new `PostgreSQL` backend from configuration.
    ///
    /// Stores a clone of the configuration for constructing connection options
    /// for non-default databases at runtime. The initial pool is placed into
    /// the shared cache keyed by the configured database name.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Connection`] if the connection fails.
    pub async fn new(config: &DatabaseConfig) -> Result<Self, AppError> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_pool_size)
            .connect_with(config.into())
            .await
            .map_err(|e| AppError::Connection(format!("Failed to connect to PostgreSQL: {e}")))?;

        info!(
            "PostgreSQL connection pool initialized (max size: {})",
            config.max_pool_size
        );

        // PostgreSQL defaults to a database named after the connecting user.
        let default_db = config
            .name
            .as_deref()
            .filter(|n| !n.is_empty())
            .map_or_else(|| config.user.clone(), String::from);

        let pools = Cache::builder()
            .max_capacity(POOL_CACHE_CAPACITY)
            .eviction_listener(|_key, pool: PgPool, _cause| {
                tokio::spawn(async move {
                    pool.close().await;
                });
            })
            .build();

        pools.insert(default_db.clone(), pool).await;

        Ok(Self {
            config: config.clone(),
            default_db,
            pools,
            read_only: config.read_only,
        })
    }
}

impl PostgresBackend {
    /// Wraps `name` in double quotes for safe use in `PostgreSQL` SQL statements.
    ///
    /// Escapes internal double quotes by doubling them.
    fn quote_identifier(name: &str) -> String {
        let escaped = name.replace('"', "\"\"");
        format!("\"{escaped}\"")
    }

    /// Returns a connection pool for the requested database.
    ///
    /// Resolves `None` or empty names to the default pool. On a cache miss
    /// a new pool is created and cached. Evicted pools are closed via the
    /// cache's eviction listener.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::InvalidIdentifier`] if the database name fails
    /// validation, or [`AppError::Connection`] if the new pool cannot connect.
    async fn get_pool(&self, database: Option<&str>) -> Result<PgPool, AppError> {
        let db_key = match database {
            Some(name) if !name.is_empty() => name,
            _ => &self.default_db,
        };

        if let Some(pool) = self.pools.get(db_key).await {
            return Ok(pool);
        }

        // Cache miss — validate then create a new pool.
        validate_identifier(db_key)?;

        let config = self.config.clone();
        let db_key_owned = db_key.to_owned();

        let pool = self
            .pools
            .try_get_with(db_key_owned, async {
                let mut cfg = config;
                cfg.name = Some(db_key.to_owned());
                PgPoolOptions::new()
                    .max_connections(cfg.max_pool_size)
                    .connect_with((&cfg).into())
                    .await
                    .map_err(|e| {
                        AppError::Connection(format!("Failed to connect to PostgreSQL database '{db_key}': {e}"))
                    })
            })
            .await
            .map_err(|e| match e.as_ref() {
                AppError::Connection(msg) => AppError::Connection(msg.clone()),
                other => AppError::Connection(other.to_string()),
            })?;

        Ok(pool)
    }
}

impl DatabaseBackend for PostgresBackend {
    // `list_databases` uses the default pool intentionally — `pg_database`
    // is a server-wide catalog that returns all databases regardless of
    // which database the connection targets.
    async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        let pool = self.get_pool(None).await?;
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname")
                .fetch_all(&pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn list_tables(&self, database: &str) -> Result<Vec<String>, AppError> {
        let db = if database.is_empty() { None } else { Some(database) };
        let pool = self.get_pool(db).await?;
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename")
                .fetch_all(&pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_table_schema(&self, database: &str, table: &str) -> Result<Value, AppError> {
        validate_identifier(table)?;
        let db = if database.is_empty() { None } else { Some(database) };
        let pool = self.get_pool(db).await?;

        // 1. Get basic schema
        let rows: Vec<PgRow> = sqlx::query(
            r"SELECT column_name, data_type, is_nullable, column_default,
                      character_maximum_length
               FROM information_schema.columns
               WHERE table_schema = 'public' AND table_name = $1
               ORDER BY ordinal_position",
        )
        .bind(table)
        .fetch_all(&pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;

        if rows.is_empty() {
            return Err(AppError::TableNotFound(table.to_string()));
        }

        let mut columns: HashMap<String, Value> = HashMap::new();
        for row in &rows {
            let col_name: String = row.try_get("column_name").unwrap_or_default();
            let data_type: String = row.try_get("data_type").unwrap_or_default();
            let nullable: String = row.try_get("is_nullable").unwrap_or_default();
            let default: Option<String> = row.try_get("column_default").ok();
            columns.insert(
                col_name,
                json!({
                    "type": data_type,
                    "nullable": nullable.to_uppercase() == "YES",
                    "key": Value::Null,
                    "default": default,
                    "extra": Value::Null,
                    "foreign_key": Value::Null,
                }),
            );
        }

        // 2. Get FK relationships
        let fk_rows: Vec<PgRow> = sqlx::query(
            r"SELECT
                kcu.column_name,
                tc.constraint_name,
                ccu.table_name AS referenced_table,
                ccu.column_name AS referenced_column,
                rc.update_rule AS on_update,
                rc.delete_rule AS on_delete
            FROM information_schema.table_constraints tc
            JOIN information_schema.key_column_usage kcu
                ON tc.constraint_name = kcu.constraint_name
                AND tc.table_schema = kcu.table_schema
            JOIN information_schema.constraint_column_usage ccu
                ON ccu.constraint_name = tc.constraint_name
                AND ccu.table_schema = tc.table_schema
            JOIN information_schema.referential_constraints rc
                ON rc.constraint_name = tc.constraint_name
                AND rc.constraint_schema = tc.table_schema
            WHERE tc.constraint_type = 'FOREIGN KEY'
                AND tc.table_name = $1
                AND tc.table_schema = 'public'",
        )
        .bind(table)
        .fetch_all(&pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;

        for fk_row in &fk_rows {
            let col_name: String = fk_row.try_get("column_name").unwrap_or_default();
            if let Some(col_info) = columns.get_mut(&col_name)
                && let Some(obj) = col_info.as_object_mut()
            {
                obj.insert(
                    "foreign_key".to_string(),
                    json!({
                        "constraint_name": fk_row.try_get::<String, _>("constraint_name").ok(),
                        "referenced_table": fk_row.try_get::<String, _>("referenced_table").ok(),
                        "referenced_column": fk_row.try_get::<String, _>("referenced_column").ok(),
                        "on_update": fk_row.try_get::<String, _>("on_update").ok(),
                        "on_delete": fk_row.try_get::<String, _>("on_delete").ok(),
                    }),
                );
            }
        }

        Ok(json!({
            "table_name": table,
            "columns": columns,
        }))
    }

    async fn execute_query(&self, sql: &str, database: Option<&str>) -> Result<Value, AppError> {
        let pool = self.get_pool(database).await?;
        let rows: Vec<PgRow> = sqlx::query(sql)
            .fetch_all(&pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(Value::Array(rows.iter().map(RowExt::to_json).collect()))
    }

    async fn create_database(&self, name: &str) -> Result<Value, AppError> {
        if self.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        validate_identifier(name)?;

        let pool = self.get_pool(None).await?;

        // PostgreSQL CREATE DATABASE can't use parameterized queries
        sqlx::query(&format!("CREATE DATABASE {}", Self::quote_identifier(name)))
            .execute(&pool)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("already exists") {
                    return AppError::Query(format!("Database '{name}' already exists."));
                }
                AppError::Query(msg)
            })?;

        Ok(json!({
            "status": "success",
            "message": format!("Database '{name}' created successfully."),
            "database_name": name,
        }))
    }

    fn dialect(&self) -> Box<dyn sqlparser::dialect::Dialect> {
        Box::new(sqlparser::dialect::PostgreSqlDialect {})
    }

    fn read_only(&self) -> bool {
        self.read_only
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DatabaseBackend;

    fn base_config() -> DatabaseConfig {
        DatabaseConfig {
            backend: DatabaseBackend::Postgres,
            host: "pg.example.com".into(),
            port: 5433,
            user: "pgadmin".into(),
            password: Some("pgpass".into()),
            name: Some("mydb".into()),
            ..DatabaseConfig::default()
        }
    }

    #[test]
    fn quote_identifier_wraps_in_double_quotes() {
        assert_eq!(PostgresBackend::quote_identifier("users"), "\"users\"");
        assert_eq!(PostgresBackend::quote_identifier("eu-docker"), "\"eu-docker\"");
    }

    #[test]
    fn quote_identifier_escapes_double_quotes() {
        assert_eq!(PostgresBackend::quote_identifier("test\"db"), "\"test\"\"db\"");
        assert_eq!(PostgresBackend::quote_identifier("a\"b\"c"), "\"a\"\"b\"\"c\"");
    }

    #[test]
    fn try_from_basic_config() {
        let config = base_config();
        let opts = PgConnectOptions::from(&config);

        assert_eq!(opts.get_host(), "pg.example.com");
        assert_eq!(opts.get_port(), 5433);
        assert_eq!(opts.get_username(), "pgadmin");
        assert_eq!(opts.get_database(), Some("mydb"));
    }

    #[test]
    fn try_from_with_ssl_require() {
        let config = DatabaseConfig {
            ssl: true,
            ssl_verify_cert: false,
            ..base_config()
        };
        let opts = PgConnectOptions::from(&config);

        assert!(
            matches!(opts.get_ssl_mode(), PgSslMode::Require),
            "expected Require, got {:?}",
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
        let opts = PgConnectOptions::from(&config);

        assert!(
            matches!(opts.get_ssl_mode(), PgSslMode::VerifyCa),
            "expected VerifyCa, got {:?}",
            opts.get_ssl_mode()
        );
    }

    #[test]
    fn try_from_without_database_name() {
        let config = DatabaseConfig {
            name: None,
            ..base_config()
        };
        let opts = PgConnectOptions::from(&config);

        assert_eq!(opts.get_database(), None);
    }

    #[test]
    fn try_from_without_password() {
        let config = DatabaseConfig {
            password: None,
            ..base_config()
        };
        let opts = PgConnectOptions::from(&config);

        assert_eq!(opts.get_host(), "pg.example.com");
    }
}
