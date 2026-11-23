//! `MySQL`-family flavour detection: `MySQL` or `MariaDB`, from a `VERSION()` string.
//!
//! Also reads the release [`line`] and a fork's [`variant`] from it.
//!
//! Callers read `VERSION()` themselves (sqlx keeps the handshake version
//! private): [`MysqlConnection`](crate::connection::MysqlConnection) once per
//! process to warn, the column policy on every statement to refuse.

use dbmcp_config::DatabaseBackend;
use dbmcp_sql::support::{Line, Variant};

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

/// Reads the release line of a `VERSION()` string from a `server` of that flavour.
///
/// Skips the `5.5.5-` replication prefix some `MariaDB` servers and proxies
/// report, such as `5.5.5-10.11.6-MariaDB` → 10.11.
#[must_use]
pub(crate) fn line(version: &str, server: DatabaseBackend) -> Option<Line> {
    let version = match version.strip_prefix("5.5.5-") {
        Some(rest) if server == DatabaseBackend::Mariadb => rest,
        _ => version,
    };
    Line::parse(version)
}

/// Detects a fork that names itself in a `VERSION()` string.
///
/// Aurora `MySQL` does not; callers probe its `aurora_version` variable.
#[must_use]
pub(crate) fn variant(version: &str) -> Option<Variant> {
    if version.contains("-TiDB-") {
        Some(Variant::Tidb)
    } else if version.contains("-Vitess") {
        Some(Variant::Vitess)
    } else {
        None
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

    #[test]
    fn line_skips_the_mariadb_replication_prefix() {
        use DatabaseBackend::{Mariadb, Mysql};
        assert_eq!(line("5.5.5-10.11.6-MariaDB", Mariadb), Some(Line::new(10, 11)));
        assert_eq!(line("11.8.9-MariaDB-ubu2404", Mariadb), Some(Line::new(11, 8)));
        assert_eq!(line("8.4.11", Mysql), Some(Line::new(8, 4)));
        assert_eq!(line("5.5.5-log", Mysql), Some(Line::new(5, 5)));
    }

    #[test]
    fn variant_reads_forks_from_the_version() {
        assert_eq!(variant("8.0.11-TiDB-v7.5.0"), Some(Variant::Tidb));
        assert_eq!(variant("8.0.40-Vitess"), Some(Variant::Vitess));
        assert_eq!(variant("8.4.3-Vitess-planetscale"), Some(Variant::Vitess));
        assert_eq!(variant("8.4.11"), None);
        assert_eq!(variant("11.8.9-MariaDB"), None);
    }
}
