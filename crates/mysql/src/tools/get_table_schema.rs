//! MCP tool: `get_table_schema`.

use std::borrow::Cow;
use std::collections::HashMap;

use database_mcp_server::AppError;
use database_mcp_server::types::{GetTableSchemaRequest, TableSchemaResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::identifier::validate_identifier;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::{Value, json};

use crate::MysqlHandler;

/// Marker type for the `get_table_schema` MCP tool.
pub(crate) struct GetTableSchemaTool;

impl GetTableSchemaTool {
    const NAME: &'static str = "get_table_schema";
    const TITLE: &'static str = "Get Table Schema";
    const DESCRIPTION: &'static str = r#"Get column definitions and foreign key relationships for a table. Requires `database_name` and `table_name` — call `list_databases` and `list_tables` first.

<usecase>
ALWAYS call this before writing queries to understand:
- Column names and data types
- Which columns are nullable, primary keys, or have defaults
- Foreign key relationships for writing JOINs
</usecase>

<examples>
✓ "What columns does the orders table have?" → get_table_schema(database_name="mydb", table_name="orders")
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

impl AsyncTool<MysqlHandler> for GetTableSchemaTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.get_table_schema(&params).await?)
    }
}

impl MysqlHandler {
    /// Returns column definitions with foreign key relationships.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if validation fails or the query errors.
    pub async fn get_table_schema(&self, request: &GetTableSchemaRequest) -> Result<TableSchemaResponse, AppError> {
        let database = &request.database_name;
        let table = &request.table_name;
        validate_identifier(database)?;
        validate_identifier(table)?;

        // 1. Get basic schema
        let describe_sql = format!(
            "DESCRIBE {}.{}",
            self.connection.quote_identifier(database),
            self.connection.quote_identifier(table)
        );
        let schema_rows = self.connection.fetch(describe_sql.as_str(), None).await?;

        if schema_rows.is_empty() {
            return Err(AppError::TableNotFound(format!("{database}.{table}")));
        }

        let mut columns: HashMap<String, Value> = HashMap::new();
        for row in &schema_rows {
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
        let fk_sql = format!(
            "SELECT
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
            WHERE kcu.TABLE_SCHEMA = {}
              AND kcu.TABLE_NAME = {}
              AND kcu.REFERENCED_TABLE_NAME IS NOT NULL
            ORDER BY kcu.CONSTRAINT_NAME, kcu.ORDINAL_POSITION",
            self.connection.quote_string(database),
            self.connection.quote_string(table),
        );

        let fk_rows = self.connection.fetch(fk_sql.as_str(), None).await?;

        for fk_row in &fk_rows {
            if let Some(col_name) = fk_row.get("column_name").and_then(|v| v.as_str())
                && let Some(col_info) = columns.get_mut(col_name)
                && let Some(obj) = col_info.as_object_mut()
            {
                obj.insert(
                    "foreign_key".to_string(),
                    json!({
                        "constraint_name": fk_row.get("constraint_name"),
                        "referenced_table": fk_row.get("referenced_table"),
                        "referenced_column": fk_row.get("referenced_column"),
                        "on_update": fk_row.get("on_update"),
                        "on_delete": fk_row.get("on_delete"),
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
