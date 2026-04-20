//! MCP tool: `getTableSchema`.

use std::borrow::Cow;
use std::collections::HashMap;

use database_mcp_server::types::{GetTableSchemaRequest, TableSchemaResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::SqlError;
use database_mcp_sql::sanitize::{quote_literal, validate_ident};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::{Value, json};

use crate::PostgresHandler;

/// Marker type for the `getTableSchema` MCP tool.
pub(crate) struct GetTableSchemaTool;

impl GetTableSchemaTool {
    const NAME: &'static str = "getTableSchema";
    const TITLE: &'static str = "Get Table Schema";
    const DESCRIPTION: &'static str = r#"Get column definitions and foreign key relationships for a table. Requires `database` and `table` — call `listDatabases` and `listTables` first.

<usecase>
ALWAYS call this before writing queries to understand:
- Column names and data types
- Which columns are nullable, primary keys, or have defaults
- Foreign key relationships for writing JOINs
</usecase>

<examples>
✓ "What columns does the orders table have?" → getTableSchema(database="mydb", table="orders")
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

impl AsyncTool<PostgresHandler> for GetTableSchemaTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.get_table_schema(params).await?)
    }
}

impl PostgresHandler {
    /// Returns column definitions with foreign key relationships.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if validation fails or the query errors.
    #[allow(clippy::too_many_lines)]
    pub async fn get_table_schema(
        &self,
        GetTableSchemaRequest { database, table }: GetTableSchemaRequest,
    ) -> Result<TableSchemaResponse, SqlError> {
        validate_ident(&table)?;
        let db = Some(database.trim()).filter(|s| !s.is_empty());
        if let Some(name) = &db {
            validate_ident(name)?;
        }

        // 1. Get basic schema
        let schema_sql = format!(
            r"
            SELECT column_name, data_type, is_nullable, column_default,
                   character_maximum_length
            FROM information_schema.columns
            WHERE table_schema = 'public' AND table_name = {}
            ORDER BY ordinal_position",
            quote_literal(&table),
        );
        let rows = self.connection.fetch_json(&schema_sql, db).await?;

        if rows.is_empty() {
            return Err(SqlError::TableNotFound(table));
        }

        let mut columns: HashMap<String, Value> = HashMap::new();
        for row in &rows {
            let col_name = row
                .get("column_name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let data_type = row
                .get("data_type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let nullable = row
                .get("is_nullable")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let default = row.get("column_default").and_then(Value::as_str).map(str::to_owned);
            columns.insert(
                col_name,
                json!({
                    "type": data_type,
                    "nullable": nullable.to_uppercase() == "YES",
                    "key": Value::Null,
                    "default": default,
                    "extra": Value::Null,
                    "foreignKey": Value::Null,
                }),
            );
        }

        // 2. Get FK relationships
        let fk_sql = format!(
            r"
            SELECT
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
                AND tc.table_name = {}
                AND tc.table_schema = 'public'",
            quote_literal(&table),
        );
        let fk_rows = self.connection.fetch_json(&fk_sql, db).await?;

        for fk_row in &fk_rows {
            let col_name = fk_row
                .get("column_name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if let Some(col_info) = columns.get_mut(&col_name)
                && let Some(obj) = col_info.as_object_mut()
            {
                obj.insert(
                    "foreignKey".to_string(),
                    json!({
                        "constraintName": fk_row.get("constraint_name").and_then(Value::as_str),
                        "referencedTable": fk_row.get("referenced_table").and_then(Value::as_str),
                        "referencedColumn": fk_row.get("referenced_column").and_then(Value::as_str),
                        "onUpdate": fk_row.get("on_update").and_then(Value::as_str),
                        "onDelete": fk_row.get("on_delete").and_then(Value::as_str),
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
