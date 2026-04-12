//! MCP tool: `get_table_schema`.

use std::borrow::Cow;
use std::collections::HashMap;

use database_mcp_server::AppError;
use database_mcp_server::types::TableSchemaResponse;
use database_mcp_sql::Connection as _;
use database_mcp_sql::identifier::validate_identifier;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::{Value, json};

use crate::SqliteHandler;
use crate::types::GetTableSchemaRequest;

/// Marker type for the `get_table_schema` MCP tool.
pub(crate) struct GetTableSchemaTool;

impl GetTableSchemaTool {
    const NAME: &'static str = "get_table_schema";
    const DESCRIPTION: &'static str = r#"Get column definitions and foreign key relationships for a table. Requires `table_name` — call `list_tables` first.

<usecase>
ALWAYS call this before writing queries to understand:
- Column names and data types
- Which columns are nullable, primary keys, or have defaults
- Foreign key relationships for writing JOINs
</usecase>

<examples>
✓ "What columns does the orders table have?" → get_table_schema(table_name="orders")
✓ Before writing a SELECT → get_table_schema first to confirm column names
✓ "How are users and orders related?" → check foreign keys in both tables
</examples>

<what_it_returns>
A JSON object with table_name and columns keyed by column name, each containing type, nullable, key, default, and foreign_key info.
</what_it_returns>"#;
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

impl AsyncTool<SqliteHandler> for GetTableSchemaTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.get_table_schema(&params).await?)
    }
}

impl SqliteHandler {
    /// Returns column definitions with foreign key relationships.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if validation fails or the query errors.
    pub async fn get_table_schema(&self, request: &GetTableSchemaRequest) -> Result<TableSchemaResponse, AppError> {
        let table = &request.table_name;
        validate_identifier(table)?;

        // 1. Get basic schema
        let pragma_sql = format!("PRAGMA table_info({})", self.connection.quote_identifier(table));
        let rows = self.connection.fetch(pragma_sql.as_str(), None).await?;

        if rows.is_empty() {
            return Err(AppError::TableNotFound(table.clone()));
        }

        let mut columns: HashMap<String, Value> = HashMap::new();
        for row in &rows {
            let col_name = row.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
            let col_type = row.get("type").and_then(Value::as_str).unwrap_or_default().to_owned();
            let notnull = row.get("notnull").and_then(Value::as_i64).unwrap_or(0);
            let default = row.get("dflt_value").and_then(Value::as_str).map(str::to_owned);
            let pk = row.get("pk").and_then(Value::as_i64).unwrap_or(0);
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
        let fk_pragma_sql = format!("PRAGMA foreign_key_list({})", self.connection.quote_identifier(table));
        let fk_rows = self.connection.fetch(fk_pragma_sql.as_str(), None).await?;

        for fk_row in &fk_rows {
            let from_col = fk_row
                .get("from")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if let Some(col_info) = columns.get_mut(&from_col)
                && let Some(obj) = col_info.as_object_mut()
            {
                let ref_table = fk_row
                    .get("table")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let ref_col = fk_row.get("to").and_then(Value::as_str).unwrap_or_default().to_owned();
                let on_update = fk_row
                    .get("on_update")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let on_delete = fk_row
                    .get("on_delete")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
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

        Ok(TableSchemaResponse {
            table_name: table.clone(),
            columns: json!(columns),
        })
    }
}
