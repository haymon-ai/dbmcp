//! `MySQL`-family flavour detection: `MySQL` or `MariaDB`, from a `VERSION()` string.
//!
//! Callers read `VERSION()` themselves (sqlx keeps the handshake version
//! private): [`MysqlConnection`](crate::connection::MysqlConnection) once per
//! process to warn, the column policy on every statement to refuse.

use dbmcp_config::DatabaseBackend;

/// The server is not the configured `MySQL`-family backend.
#[derive(Debug, thiserror::Error)]
#[error(
    "database.backend is {configured} but the server is {server}; \
     set database.backend / DBMCP_DB_BACKEND to {server}"
)]
pub(crate) struct BackendMismatch {
    pub(crate) configured: DatabaseBackend,
    pub(crate) server: DatabaseBackend,
}

/// Classifies a `VERSION()` string: `MariaDB` if it contains `mariadb` (any case).
#[must_use]
pub(crate) fn of(version: &str) -> DatabaseBackend {
    if version
        .as_bytes()
        .windows(7)
        .any(|w| w.eq_ignore_ascii_case(b"mariadb"))
    {
        DatabaseBackend::Mariadb
    } else {
        DatabaseBackend::Mysql
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn of_is_mariadb_only_for_mariadb_versions() {
        use DatabaseBackend::{Mariadb, Mysql};
        for (version, expected) in [
            ("11.8.9-MariaDB", Mariadb),
            ("10.11.10-MariaDB-log", Mariadb),
            ("5.5.5-10.11.6-MariaDB", Mariadb),
            ("10.6.0-mariadb", Mariadb),
            ("8.4.11", Mysql),
            ("9.6.0", Mysql),
            ("8.0.11-TiDB-v7.5.0", Mysql),
            ("", Mysql),
        ] {
            assert_eq!(of(version), expected, "{version}");
        }
    }
}
