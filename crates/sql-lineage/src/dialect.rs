//! SQL dialects and their identifier rules.

use serde::{Deserialize, Serialize};
use sqlparser::ast::{Ident, Statement};
use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::{Parser, ParserError};

/// The database dialect a statement is written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dialect {
    /// `PostgreSQL`.
    Postgres,
    /// `MySQL`.
    Mysql,
    /// `MariaDB`.
    Mariadb,
    /// `SQLite`.
    Sqlite,
}

impl Dialect {
    pub(crate) fn parse(self, sql: &str) -> Result<Vec<Statement>, ParserError> {
        match self {
            Self::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql),
            Self::Mysql | Self::Mariadb => Parser::parse_sql(&MySqlDialect {}, sql),
            Self::Sqlite => Parser::parse_sql(&SQLiteDialect {}, sql),
        }
    }

    /// Returns whether a written identifier names the actual name.
    ///
    /// `PostgreSQL` folds unquoted identifiers to ASCII lowercase and matches
    /// quoted ones exactly. `MySQL` and `MariaDB` compare case-insensitively;
    /// `SQLite` compares ASCII case-insensitively.
    pub(crate) fn matches(self, written: &Ident, actual: &str) -> bool {
        match self {
            Self::Postgres => {
                if written.quote_style.is_some() {
                    written.value == actual
                } else {
                    written.value.bytes().map(|b| b.to_ascii_lowercase()).eq(actual.bytes())
                }
            }
            Self::Mysql | Self::Mariadb => written.value.to_lowercase() == actual.to_lowercase(),
            Self::Sqlite => written.value.eq_ignore_ascii_case(actual),
        }
    }

    /// Returns the name the server gives a written identifier.
    pub(crate) fn normalize(self, ident: &Ident) -> String {
        match self {
            Self::Postgres if ident.quote_style.is_none() => ident.value.to_ascii_lowercase(),
            _ => ident.value.clone(),
        }
    }

    /// Returns whether CTE names match only when written identically.
    ///
    /// `MySQL`'s case rules for CTE names versus tables vary with server
    /// settings, so a case-only match is rejected rather than guessed.
    pub(crate) fn strict_cte_case(self) -> bool {
        matches!(self, Self::Mysql | Self::Mariadb)
    }

    /// Returns the server's name for the `index`-th (0-based) `VALUES` column.
    pub(crate) fn values_column(self, index: usize) -> String {
        match self {
            Self::Mysql | Self::Mariadb => format!("column_{index}"),
            Self::Postgres | Self::Sqlite => format!("column{}", index + 1),
        }
    }
}
