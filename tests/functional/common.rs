//! Shared helpers for integration tests.
//!
//! Provides adapter creation functions used across all backend test files.

use database_mcp_config::{DatabaseBackend, DatabaseConfig};

/// Creates a [`DatabaseConfig`] for `SQLite` tests.
pub fn sqlite_config(db_path: &str, read_only: bool) -> DatabaseConfig {
    DatabaseConfig {
        backend: DatabaseBackend::Sqlite,
        port: 0,
        user: String::new(),
        name: Some(db_path.to_string()),
        read_only,
        ..DatabaseConfig::default()
    }
}

/// Creates a [`DatabaseConfig`] for `MySQL`/`MariaDB` tests.
pub fn mysql_config(read_only: bool) -> DatabaseConfig {
    DatabaseConfig {
        backend: DatabaseBackend::Mysql,
        host: std::env::var("DB_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("DB_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3306),
        user: std::env::var("DB_USER").unwrap_or_else(|_| "root".into()),
        password: std::env::var("DB_PASSWORD").ok(),
        name: Some("app".into()),
        read_only,
        ..DatabaseConfig::default()
    }
}

/// Creates a [`DatabaseConfig`] for `PostgreSQL` tests.
pub fn postgres_config(read_only: bool) -> DatabaseConfig {
    DatabaseConfig {
        backend: DatabaseBackend::Postgres,
        host: std::env::var("DB_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("DB_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(5432),
        user: std::env::var("DB_USER").unwrap_or_else(|_| "postgres".into()),
        password: std::env::var("DB_PASSWORD").ok(),
        name: Some("app".into()),
        read_only,
        ..DatabaseConfig::default()
    }
}
