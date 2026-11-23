//! Supported database versions and the tier of a running server.
//!
//! [`SUPPORTED`](supported) lists the lines CI tests per backend, edited by
//! hand with the CI version matrix. Backends read the server version once
//! per process, parse it into a [`Line`], detect a fork [`Variant`], and
//! [`classify`] it. Outside [`Tier::Supported`], [`Tier::warning`] gives the
//! one startup warning. Nothing is refused by tier.

use std::fmt;

use dbmcp_config::DatabaseBackend;

/// A release line: a `PostgreSQL` major, or a `major.minor` elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Line {
    major: u32,
    minor: Option<u32>,
}

impl Line {
    /// Builds a `major.minor` line.
    #[must_use]
    pub const fn new(major: u32, minor: u32) -> Self {
        Self {
            major,
            minor: Some(minor),
        }
    }

    /// Builds a major-only line, as `PostgreSQL` numbers them.
    #[must_use]
    pub const fn major(major: u32) -> Self {
        Self { major, minor: None }
    }

    /// Reads the leading `major.minor` of a version string.
    ///
    /// Fits `MySQL`/`MariaDB` `VERSION()` and `SQLite` `sqlite_version()`,
    /// such as `11.8.9-MariaDB-ubu2404` → 11.8. `None` without both numbers.
    #[must_use]
    pub fn parse(version: &str) -> Option<Self> {
        let (major, rest) = version.split_once('.')?;
        let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        let minor = &rest[..end];
        if major.is_empty() || minor.is_empty() || !major.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some(Self::new(major.parse().ok()?, minor.parse().ok()?))
    }

    /// Reads a `PostgreSQL` `server_version_num`, such as `170004` → 17.
    ///
    /// Before 10 a line had two parts, so `90624` → 9.6.
    #[must_use]
    pub const fn from_version_num(num: u32) -> Self {
        if num < 100_000 {
            Self::new(num / 10_000, num / 100 % 100)
        } else {
            Self::major(num / 10_000)
        }
    }
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.minor {
            Some(minor) => write!(f, "{}.{minor}", self.major),
            None => write!(f, "{}", self.major),
        }
    }
}

/// Lines `PostgreSQL` CI tests.
const POSTGRES: &[Line] = &[
    Line::major(15),
    Line::major(16),
    Line::major(17),
    Line::major(18),
    Line::major(19),
];

/// Lines `MySQL` CI tests.
const MYSQL: &[Line] = &[Line::new(8, 4), Line::new(9, 7), Line::new(26, 10)];

/// Lines `MariaDB` CI tests.
const MARIADB: &[Line] = &[
    Line::new(10, 11),
    Line::new(11, 4),
    Line::new(11, 8),
    Line::new(12, 3),
    Line::new(13, 0),
];

/// The bundled `SQLite` line.
const SQLITE: &[Line] = &[Line::new(3, 54)];

/// Returns the Supported lines for `backend`, ascending.
#[must_use]
pub const fn supported(backend: DatabaseBackend) -> &'static [Line] {
    match backend {
        DatabaseBackend::Postgres => POSTGRES,
        DatabaseBackend::Mysql => MYSQL,
        DatabaseBackend::Mariadb => MARIADB,
        DatabaseBackend::Sqlite => SQLITE,
    }
}

/// A fork or re-implementation the server identified itself as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Amazon Aurora `MySQL`.
    AuroraMysql,
    /// Amazon Aurora `PostgreSQL`.
    AuroraPostgres,
    /// `TiDB`.
    Tidb,
    /// `CockroachDB`.
    Cockroachdb,
    /// `YugabyteDB`.
    Yugabytedb,
}

impl fmt::Display for Variant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AuroraMysql => "Aurora MySQL",
            Self::AuroraPostgres => "Aurora PostgreSQL",
            Self::Tidb => "TiDB",
            Self::Cockroachdb => "CockroachDB",
            Self::Yugabytedb => "YugabyteDB",
        })
    }
}

/// The support tier of a running server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// A listed line on the upstream server.
    Supported,
    /// An upstream line that is not listed.
    BestEffort {
        /// The configured backend.
        backend: DatabaseBackend,
        /// The server's line.
        line: Line,
        /// Whether the line is above every listed one.
        newer: bool,
    },
    /// A fork or re-implementation.
    Unsupported(Variant),
}

impl Tier {
    /// Returns the startup warning, or `None` when Supported.
    ///
    /// With `rules`, the text adds that column-rule guarantees are not claimed.
    #[must_use]
    pub fn warning(&self, rules: bool) -> Option<String> {
        let mut text = match *self {
            Self::Supported => return None,
            Self::Unsupported(variant) => format!("server is {variant}, an Unsupported fork"),
            Self::BestEffort {
                backend,
                line,
                newer: true,
            } => {
                let latest = supported(backend).last().map(ToString::to_string).unwrap_or_default();
                format!(
                    "{} {line} is newer than the versions tested (Best effort); latest tested is {latest}",
                    name(backend)
                )
            }
            Self::BestEffort {
                backend,
                line,
                newer: false,
            } => {
                let list = supported(backend)
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{} {line} is not Supported (Best effort: end of life or untested); Supported: {list}",
                    name(backend)
                )
            }
        };
        if rules {
            text.push_str("; column-rule guarantees not claimed");
        }
        Some(text)
    }
}

/// Classifies a server by its line and detected variant.
///
/// A variant is Unsupported whatever line it reports, since forks fake it.
#[must_use]
pub fn classify(backend: DatabaseBackend, line: Line, variant: Option<Variant>) -> Tier {
    if let Some(variant) = variant {
        return Tier::Unsupported(variant);
    }
    let lines = supported(backend);
    if lines.contains(&line) {
        return Tier::Supported;
    }
    Tier::BestEffort {
        backend,
        line,
        newer: lines.last().is_some_and(|latest| line > *latest),
    }
}

/// Returns the product name of `backend`.
const fn name(backend: DatabaseBackend) -> &'static str {
    match backend {
        DatabaseBackend::Postgres => "PostgreSQL",
        DatabaseBackend::Mysql => "MySQL",
        DatabaseBackend::Mariadb => "MariaDB",
        DatabaseBackend::Sqlite => "SQLite",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use DatabaseBackend::{Mariadb, Mysql, Postgres, Sqlite};

    #[test]
    fn parse_reads_the_leading_major_minor() {
        for (version, expected) in [
            ("8.4.11", Some(Line::new(8, 4))),
            ("26.10.0", Some(Line::new(26, 10))),
            ("11.8.9-MariaDB-ubu2404", Some(Line::new(11, 8))),
            ("10.11.10-MariaDB-log", Some(Line::new(10, 11))),
            ("8.0.11-TiDB-v7.5.0", Some(Line::new(8, 0))),
            ("3.54.1", Some(Line::new(3, 54))),
            ("9.7", Some(Line::new(9, 7))),
            ("9", None),
            ("9.", None),
            (".4", None),
            ("x8.4", None),
            ("", None),
        ] {
            assert_eq!(Line::parse(version), expected, "{version}");
        }
    }

    #[test]
    fn from_version_num_reads_the_postgres_major() {
        assert_eq!(Line::from_version_num(150_012), Line::major(15));
        assert_eq!(Line::from_version_num(190_000), Line::major(19));
        assert_eq!(Line::from_version_num(170_004).to_string(), "17");
        assert_eq!(Line::from_version_num(90_624), Line::new(9, 6));
    }

    #[test]
    fn variant_is_unsupported_even_on_a_listed_line() {
        assert_eq!(
            classify(Mysql, Line::new(8, 4), Some(Variant::AuroraMysql)),
            Tier::Unsupported(Variant::AuroraMysql)
        );
        assert_eq!(
            classify(Postgres, Line::major(16), Some(Variant::Cockroachdb)),
            Tier::Unsupported(Variant::Cockroachdb)
        );
    }

    #[test]
    fn listed_lines_are_supported() {
        assert_eq!(classify(Postgres, Line::major(15), None), Tier::Supported);
        assert_eq!(classify(Mysql, Line::new(26, 10), None), Tier::Supported);
        assert_eq!(classify(Mariadb, Line::new(10, 11), None), Tier::Supported);
        assert_eq!(classify(Sqlite, Line::new(3, 54), None), Tier::Supported);
    }

    #[test]
    fn line_above_the_list_is_newer() {
        assert_eq!(
            classify(Mysql, Line::new(27, 4), None),
            Tier::BestEffort {
                backend: Mysql,
                line: Line::new(27, 4),
                newer: true
            }
        );
        assert!(matches!(
            classify(Postgres, Line::major(20), None),
            Tier::BestEffort { newer: true, .. }
        ));
    }

    #[test]
    fn unlisted_line_below_the_top_is_untested() {
        assert_eq!(
            classify(Mysql, Line::new(9, 5), None),
            Tier::BestEffort {
                backend: Mysql,
                line: Line::new(9, 5),
                newer: false
            }
        );
        assert!(matches!(
            classify(Mysql, Line::new(8, 0), None),
            Tier::BestEffort { newer: false, .. }
        ));
        assert!(matches!(
            classify(Postgres, Line::major(14), None),
            Tier::BestEffort { newer: false, .. }
        ));
    }

    #[test]
    fn supported_has_no_warning() {
        assert_eq!(Tier::Supported.warning(false), None);
        assert_eq!(Tier::Supported.warning(true), None);
    }

    #[test]
    fn warnings_name_the_tier() {
        assert_eq!(
            classify(Mysql, Line::new(8, 4), Some(Variant::AuroraMysql)).warning(false),
            Some("server is Aurora MySQL, an Unsupported fork".into())
        );
        assert_eq!(
            classify(Mysql, Line::new(27, 4), None).warning(false),
            Some("MySQL 27.4 is newer than the versions tested (Best effort); latest tested is 26.10".into())
        );
        assert_eq!(
            classify(Mysql, Line::new(9, 5), None).warning(false),
            Some(
                "MySQL 9.5 is not Supported (Best effort: end of life or untested); Supported: 8.4, 9.7, 26.10".into()
            )
        );
        assert_eq!(
            classify(Postgres, Line::major(14), None).warning(false),
            Some(
                "PostgreSQL 14 is not Supported (Best effort: end of life or untested); Supported: 15, 16, 17, 18, 19"
                    .into()
            )
        );
    }

    #[test]
    fn rules_add_the_suffix() {
        assert_eq!(
            classify(Postgres, Line::major(16), Some(Variant::Yugabytedb)).warning(true),
            Some("server is YugabyteDB, an Unsupported fork; column-rule guarantees not claimed".into())
        );
        assert_eq!(
            classify(Mariadb, Line::new(10, 6), None).warning(true),
            Some(
                "MariaDB 10.6 is not Supported (Best effort: end of life or untested); \
                 Supported: 10.11, 11.4, 11.8, 12.3, 13.0; column-rule guarantees not claimed"
                    .into()
            )
        );
    }
}
