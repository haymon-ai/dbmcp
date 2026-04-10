//! MCP tool: `get_table_schema`.

use std::borrow::Cow;
use std::collections::HashMap;

use database_mcp_server::AppError;
use database_mcp_server::types::{GetTableSchemaRequest, TableSchemaResponse};
use database_mcp_sql::identifier::validate_identifier;
use database_mcp_sql::timeout::execute_with_timeout;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::{Value, json};
use sqlx::Row;
use sqlx::postgres::PgRow;

use crate::PostgresHandler;

/// Marker type for the `get_table_schema` MCP tool.
pub(crate) struct GetTableSchemaTool;

impl GetTableSchemaTool {
    const NAME: &'static str = "get_table_schema";
    const DESCRIPTION: &'static str = "Get column definitions (type, nullable, key, default) and foreign key\nrelationships for a table. Requires `database_name` and `table_name`.";
}

impl ToolBase for GetTableSchemaTool {
    type Parameter = GetTableSchemaRequest;
    type Output = TableSchemaResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        Self::NAME.into()
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(Self::DESCRIPTION.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        )
    }
}

impl AsyncTool<PostgresHandler> for GetTableSchemaTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.get_table_schema(&params).await?)
    }
}

impl PostgresHandler {
    /// Returns column definitions with foreign key relationships.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if validation fails or the query errors.
    pub async fn get_table_schema(&self, request: &GetTableSchemaRequest) -> Result<TableSchemaResponse, AppError> {
        let table = &request.table_name;
        validate_identifier(table)?;
        let db = if request.database_name.is_empty() {
            None
        } else {
            Some(request.database_name.as_str())
        };
        let pool = self.get_pool(db).await?;

        // 1. Get basic schema
        let schema_sql = r"SELECT column_name, data_type, is_nullable, column_default,
                      character_maximum_length
               FROM information_schema.columns
               WHERE table_schema = 'public' AND table_name = $1
               ORDER BY ordinal_position";
        let rows: Vec<PgRow> = execute_with_timeout(
            self.config.query_timeout,
            schema_sql,
            sqlx::query(schema_sql).bind(table).fetch_all(&pool),
        )
        .await?;

        if rows.is_empty() {
            return Err(AppError::TableNotFound(table.clone()));
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
        let fk_sql = r"SELECT
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
                AND tc.table_schema = 'public'";
        let fk_rows: Vec<PgRow> = execute_with_timeout(
            self.config.query_timeout,
            fk_sql,
            sqlx::query(fk_sql).bind(table).fetch_all(&pool),
        )
        .await?;

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

        Ok(TableSchemaResponse {
            table_name: table.clone(),
            columns: json!(columns),
        })
    }
}
