//! Logic of the `write` tool.

use dbmcp_policy::Access;
use dbmcp_privacy::MaybeRedact as _;
use dbmcp_server::truncate::{Truncation, caps};
use dbmcp_server::types::WriteResponse;
use dbmcp_sql::Connection as _;
use dbmcp_truncate::{Window, fit};
use rmcp::model::ErrorData;
use sqlparser::ast::{ObjectType, Statement};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use crate::PostgresHandler;

impl PostgresHandler {
    /// Executes a write SQL query.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode, or
    /// [`SqlError`] if the query fails. With column rules, also the
    /// policy's rejection or a withheld database error.
    ///
    /// [`SqlError`]: dbmcp_sql::SqlError
    /// [`SqlError::ReadOnlyViolation`]: dbmcp_sql::SqlError::ReadOnlyViolation
    pub async fn write(&self, query: String, database: Option<String>) -> Result<WriteResponse, ErrorData> {
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let caps = caps(self.config.max_cell_chars, self.config.max_response_chars);
        let mut window = Window::new(0, None, caps);
        let (rows, rows_affected, last_insert_id) = if let Some(policy) = &self.policy {
            let timeout = self.connection.query_timeout();
            let enforced = self
                .enforce(policy, &query, Access::Write, timeout, database, &mut window)
                .await?;
            (enforced.rows, enforced.rows_affected, enforced.last_insert_id)
        } else {
            let (rows_affected, last_insert_id) = self
                .connection
                .execute_json(query.as_str(), database, |row| window.push(row))
                .await?;
            let rows = self
                .redactor
                .redact_rows(window.items)
                .await
                .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
            (rows, rows_affected, last_insert_id)
        };
        if let Some(name) = dropped_database(&query) {
            self.connection.invalidate(&name).await;
        }
        let fitted = fit(rows, caps);

        Ok(WriteResponse {
            rows: fitted.items,
            rows_affected,
            last_insert_id,
            truncation: Truncation::new(fitted.cut, fitted.dropped.saturating_add(window.rest)),
        })
    }
}

/// Names the database a `DROP DATABASE` statement drops, folded as the server does.
///
/// Its cached pool must go, so a recreated database of that name gets
/// fresh connections.
fn dropped_database(sql: &str) -> Option<String> {
    let statements = Parser::parse_sql(&PostgreSqlDialect {}, sql).ok()?;
    let [
        Statement::Drop {
            object_type: ObjectType::Database,
            names,
            ..
        },
    ] = statements.as_slice()
    else {
        return None;
    };
    let [name] = names.as_slice() else {
        return None;
    };
    let ident = name.0.last()?.as_ident()?;
    Some(if ident.quote_style.is_some() {
        ident.value.clone()
    } else {
        ident.value.to_lowercase()
    })
}

#[cfg(test)]
mod tests {
    use super::dropped_database;

    #[test]
    fn names_the_dropped_database() {
        assert_eq!(dropped_database("DROP DATABASE Tmp").as_deref(), Some("tmp"));
        assert_eq!(
            dropped_database(r#"DROP DATABASE IF EXISTS "Tmp""#).as_deref(),
            Some("Tmp")
        );
    }

    #[test]
    fn ignores_other_statements() {
        assert_eq!(dropped_database("DROP TABLE tmp"), None);
        assert_eq!(dropped_database("DROP SCHEMA tmp"), None);
        assert_eq!(dropped_database("not sql"), None);
    }
}
