//! MySQL/MariaDB table schema introspection.
//!
//! Retrieves column definitions and foreign key relationships
//! from `information_schema`.

use std::collections::HashMap;

use database_mcp_backend::error::AppError;
use database_mcp_backend::identifier::validate_identifier;
use serde_json::{Value, json};
use sqlx::Row;
use sqlx::mysql::MySqlRow;

use super::MysqlBackend;

impl MysqlBackend {
    /// Returns column definitions with foreign key relationships.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if validation fails or the query errors.
    pub async fn get_table_schema(&self, database: &str, table: &str) -> Result<Value, AppError> {
        validate_identifier(database)?;
        validate_identifier(table)?;

        // 1. Get basic schema
        let describe_sql = format!(
            "DESCRIBE {}.{}",
            Self::quote_identifier(database),
            Self::quote_identifier(table)
        );
        let schema_results = self.query_to_json(&describe_sql, None).await?;
        let schema_rows = schema_results.as_array().map_or([].as_slice(), Vec::as_slice);

        if schema_rows.is_empty() {
            return Err(AppError::TableNotFound(format!("{database}.{table}")));
        }

        let mut columns: HashMap<String, Value> = HashMap::new();
        for row in schema_rows {
            if let Some(col_name) = row.get("Field").and_then(|v| v.as_str()) {
                columns.insert(
                    col_name.to_string(),
                    json!({
                        "type": row.get("Type").unwrap_or(&Value::Null),
                        "nullable": row.get("Null").and_then(|v| v.as_str()).is_some_and(|s| s.to_uppercase() == "YES"),
                        "key": row.get("Key").unwrap_or(&Value::Null),
                        "default": row.get("Default").unwrap_or(&Value::Null),
                        "extra": row.get("Extra").unwrap_or(&Value::Null),
                        "foreign_key": Value::Null,
                    }),
                );
            }
        }

        // 2. Get FK relationships
        let fk_sql = r"
            SELECT
                kcu.COLUMN_NAME as column_name,
                kcu.CONSTRAINT_NAME as constraint_name,
                kcu.REFERENCED_TABLE_NAME as referenced_table,
                kcu.REFERENCED_COLUMN_NAME as referenced_column,
                rc.UPDATE_RULE as on_update,
                rc.DELETE_RULE as on_delete
            FROM information_schema.KEY_COLUMN_USAGE kcu
            INNER JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
                ON kcu.CONSTRAINT_NAME = rc.CONSTRAINT_NAME
                AND kcu.CONSTRAINT_SCHEMA = rc.CONSTRAINT_SCHEMA
            WHERE kcu.TABLE_SCHEMA = ?
              AND kcu.TABLE_NAME = ?
              AND kcu.REFERENCED_TABLE_NAME IS NOT NULL
            ORDER BY kcu.CONSTRAINT_NAME, kcu.ORDINAL_POSITION
        ";

        let fk_rows: Vec<MySqlRow> = sqlx::query(fk_sql)
            .bind(database)
            .bind(table)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        for fk_row in &fk_rows {
            let col_name: Option<String> = fk_row.try_get("column_name").ok();
            if let Some(col_name) = col_name
                && let Some(col_info) = columns.get_mut(&col_name)
                && let Some(obj) = col_info.as_object_mut()
            {
                let constraint_name: Option<String> = fk_row.try_get("constraint_name").ok();
                let referenced_table: Option<String> = fk_row.try_get("referenced_table").ok();
                let referenced_column: Option<String> = fk_row.try_get("referenced_column").ok();
                let on_update: Option<String> = fk_row.try_get("on_update").ok();
                let on_delete: Option<String> = fk_row.try_get("on_delete").ok();
                obj.insert(
                    "foreign_key".to_string(),
                    json!({
                        "constraint_name": constraint_name,
                        "referenced_table": referenced_table,
                        "referenced_column": referenced_column,
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
