//! `SQLite` table schema introspection.

use std::collections::HashMap;

use backend::error::AppError;
use backend::identifier::validate_identifier;
use serde_json::{Value, json};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::SqliteBackend;

impl SqliteBackend {
    /// Returns column definitions with foreign key relationships.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if validation fails or the query errors.
    pub async fn get_table_schema(&self, _database: &str, table: &str) -> Result<Value, AppError> {
        validate_identifier(table)?;

        // 1. Get basic schema
        let rows: Vec<SqliteRow> = sqlx::query(&format!("PRAGMA table_info({})", Self::quote_identifier(table)))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        if rows.is_empty() {
            return Err(AppError::TableNotFound(table.to_string()));
        }

        let mut columns: HashMap<String, Value> = HashMap::new();
        for row in &rows {
            let col_name: String = row.try_get("name").unwrap_or_default();
            let col_type: String = row.try_get("type").unwrap_or_default();
            let notnull: i32 = row.try_get("notnull").unwrap_or(0);
            let default: Option<String> = row.try_get("dflt_value").ok();
            let pk: i32 = row.try_get("pk").unwrap_or(0);
            columns.insert(
                col_name,
                json!({
                    "type": col_type,
                    "nullable": notnull == 0,
                    "key": if pk > 0 { "PRI" } else { "" },
                    "default": default,
                    "extra": Value::Null,
                    "foreign_key": Value::Null,
                }),
            );
        }

        // 2. Get FK info via PRAGMA
        let fk_rows: Vec<SqliteRow> =
            sqlx::query(&format!("PRAGMA foreign_key_list({})", Self::quote_identifier(table)))
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;

        for fk_row in &fk_rows {
            let from_col: String = fk_row.try_get("from").unwrap_or_default();
            if let Some(col_info) = columns.get_mut(&from_col)
                && let Some(obj) = col_info.as_object_mut()
            {
                let ref_table: String = fk_row.try_get("table").unwrap_or_default();
                let ref_col: String = fk_row.try_get("to").unwrap_or_default();
                let on_update: String = fk_row.try_get("on_update").unwrap_or_default();
                let on_delete: String = fk_row.try_get("on_delete").unwrap_or_default();
                obj.insert(
                    "foreign_key".to_string(),
                    json!({
                        "constraint_name": Value::Null,
                        "referenced_table": ref_table,
                        "referenced_column": ref_col,
                        "on_update": on_update,
                        "on_delete": on_delete,
                    }),
                );
            }
        }

        Ok(json!({
            "table_name": table,
            "columns": columns,
        }))
    }
}
