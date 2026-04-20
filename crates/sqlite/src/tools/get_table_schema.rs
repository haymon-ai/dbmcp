//! MCP tool: `getTableSchema`.

use std::borrow::Cow;
use std::collections::HashMap;

use database_mcp_server::types::TableSchemaResponse;
use database_mcp_sql::Connection as _;
use database_mcp_sql::sanitize::{quote_ident, validate_ident};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::{Value, json};
use sqlparser::dialect::SQLiteDialect;

use database_mcp_sql::SqlError;

use crate::SqliteHandler;
use crate::types::GetTableSchemaRequest;

/// Marker type for the `getTableSchema` MCP tool.
pub(crate) struct GetTableSchemaTool;

impl GetTableSchemaTool {
    const NAME: &'static str = "getTableSchema";
    const TITLE: &'static str = "Get Table Schema";
    const DESCRIPTION: &'static str = r#"Get column definitions and foreign key relationships for a table. Requires `table` — call `listTables` first.

<usecase>
ALWAYS call this before writing queries to understand:
- Column names and data types
- Which columns are nullable, primary keys, or have defaults
- Foreign key relationships for writing JOINs
</usecase>

<examples>
✓ "What columns does the orders table have?" → getTableSchema(table="orders")
✓ Before writing a SELECT → getTableSchema first to confirm column names
✓ "How are users and orders related?" → check foreign keys in both tables
</examples>

<what_it_returns>
A JSON object with table and columns keyed by column name, each containing type, nullable, key, default, and foreignKey info.
</what_it_returns>"#;
}

impl ToolBase for GetTableSchemaTool {
    type Parameter = GetTableSchemaRequest;
    type Output = TableSchemaResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        Self::NAME.into()
    }

    fn title() -> Option<String> {
        Some(Self::TITLE.into())
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
        Ok(handler.get_table_schema(params).await?)
    }
}

impl SqliteHandler {
    /// Returns column definitions with foreign key relationships.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if validation fails or the query errors.
    pub async fn get_table_schema(
        &self,
        GetTableSchemaRequest { table }: GetTableSchemaRequest,
    ) -> Result<TableSchemaResponse, SqlError> {
        validate_ident(&table)?;

        // 1. Get basic schema
        let pragma_sql = format!("PRAGMA table_info({})", quote_ident(&table, &SQLiteDialect {}));
        let rows = self.connection.fetch_json(pragma_sql.as_str(), None).await?;

        if rows.is_empty() {
            return Err(SqlError::TableNotFound(table));
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
                    "foreignKey": Value::Null,
                }),
            );
        }

        // 2. Get FK info via PRAGMA
        let fk_pragma_sql = format!("PRAGMA foreign_key_list({})", quote_ident(&table, &SQLiteDialect {}));
        let fk_rows = self.connection.fetch_json(fk_pragma_sql.as_str(), None).await?;

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
                    "foreignKey".to_string(),
                    json!({
                        "constraintName": Value::Null,
                        "referencedTable": ref_table,
                        "referencedColumn": ref_col,
                        "onUpdate": on_update,
                        "onDelete": on_delete,
                    }),
                );
            }
        }

        Ok(TableSchemaResponse {
            table,
            columns: json!(columns),
        })
    }
}
