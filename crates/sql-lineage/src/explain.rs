//! `EXPLAIN` forms, classified from the parsed statement's options.
//!
//! Only the exact plain forms each dialect's `explainQuery` builds classify as
//! [`ExplainForm::Json`] or [`ExplainForm::QueryPlan`]; any `ANALYZE` is
//! [`ExplainForm::Analyze`], and every other option, format, or spelling is
//! [`ExplainForm::Other`].

use serde::{Deserialize, Serialize};
use sqlparser::ast::{AnalyzeFormat, AnalyzeFormatKind, DescribeAlias, Expr, Statement, UtilityOption};

use crate::dialect::Dialect;

/// The form of an `EXPLAIN`, named by what its result looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExplainForm {
    /// A plain JSON plan: `PostgreSQL` `(FORMAT JSON)`, `MySQL`-family `FORMAT=JSON`.
    Json,
    /// `SQLite` `EXPLAIN QUERY PLAN`.
    QueryPlan,
    /// Any `ANALYZE`, which runs the statement; its argument is not read.
    Analyze,
    /// Every other option, format, or spelling.
    Other,
}

impl ExplainForm {
    /// Classifies `statement`'s `EXPLAIN` options under `dialect`; [`Self::Other`] for any other statement.
    pub(crate) fn of(dialect: Dialect, statement: &Statement) -> Self {
        let Statement::Explain {
            describe_alias,
            analyze,
            verbose,
            query_plan,
            estimate,
            format,
            options,
            ..
        } = statement
        else {
            return Self::Other;
        };
        let list = options.as_deref().unwrap_or_default();
        if *analyze || list.iter().any(|option| named(option, "analyze")) {
            return Self::Analyze;
        }
        if *describe_alias != DescribeAlias::Explain || *verbose || *estimate {
            return Self::Other;
        }
        match (dialect, *query_plan, format, list) {
            (Dialect::Postgres, false, None, [option])
                if named(option, "format")
                    && matches!(&option.arg, Some(Expr::Identifier(arg)) if arg.value.eq_ignore_ascii_case("json")) =>
            {
                Self::Json
            }
            (
                Dialect::Mysql | Dialect::Mariadb,
                false,
                Some(
                    AnalyzeFormatKind::Keyword(AnalyzeFormat::JSON)
                    | AnalyzeFormatKind::Assignment(AnalyzeFormat::JSON),
                ),
                [],
            ) if options.is_none() => Self::Json,
            (Dialect::Sqlite, true, None, []) if options.is_none() => Self::QueryPlan,
            _ => Self::Other,
        }
    }
}

/// Whether `option` is named `name`, in any case or quoting.
fn named(option: &UtilityOption, name: &str) -> bool {
    option.name.value.eq_ignore_ascii_case(name)
}
