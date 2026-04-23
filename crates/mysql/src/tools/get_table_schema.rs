//! MCP tool: `getTableSchema`.

use std::borrow::Cow;
use std::collections::HashMap;

use dbmcp_server::types::{GetTableSchemaRequest, TableSchemaResponse};
use dbmcp_sql::Connection as _;
use dbmcp_sql::SqlError;
use dbmcp_sql::sanitize::{quote_ident, quote_literal, validate_ident};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::{Value, json};
use sqlparser::dialect::MySqlDialect;

use crate::MysqlHandler;

/// Marker type for the `getTableSchema` MCP tool.
pub(crate) struct GetTableSchemaTool;

impl GetTableSchemaTool {
    const NAME: &'static str = "getTableSchema";
    const TITLE: &'static str = "Get Table Schema";
    const DESCRIPTION: &'static str = r#"Get column definitions and foreign key relationships for a table.

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

impl AsyncTool<MysqlHandler> for GetTableSchemaTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.get_table_schema(params).await?)
    }
}

impl MysqlHandler {
    /// Returns column definitions with foreign key relationships.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if validation fails or the query errors.
    pub async fn get_table_schema(
        &self,
        GetTableSchemaRequest { database, table }: GetTableSchemaRequest,
    ) -> Result<TableSchemaResponse, SqlError> {
        let database = database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map_or_else(|| self.connection.default_database_name().to_owned(), str::to_owned);

        validate_ident(&database)?;
        validate_ident(&table)?;

        // 1. Get basic schema
        let describe_sql = format!(
            "DESCRIBE {}.{}",
            quote_ident(&database, &MySqlDialect {}),
            quote_ident(&table, &MySqlDialect {}),
        );
        let schema_rows = self.connection.fetch_json(describe_sql.as_str(), None).await?;

        if schema_rows.is_empty() {
            return Err(SqlError::TableNotFound(format!("{database}.{table}")));
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
                        "foreignKey": Value::Null,
                    }),
                );
            }
        }

        // 2. Get FK relationships
        let fk_sql = format!(
            r"
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
            WHERE kcu.TABLE_SCHEMA = {}
              AND kcu.TABLE_NAME = {}
              AND kcu.REFERENCED_TABLE_NAME IS NOT NULL
            ORDER BY kcu.CONSTRAINT_NAME, kcu.ORDINAL_POSITION",
            quote_literal(&database),
            quote_literal(&table),
        );

        let fk_rows = self.connection.fetch_json(fk_sql.as_str(), None).await?;

        for fk_row in &fk_rows {
            if let Some(col_name) = fk_row.get("column_name").and_then(|v| v.as_str())
                && let Some(col_info) = columns.get_mut(col_name)
                && let Some(obj) = col_info.as_object_mut()
            {
                obj.insert(
                    "foreignKey".to_string(),
                    json!({
                        "constraintName": fk_row.get("constraint_name"),
                        "referencedTable": fk_row.get("referenced_table"),
                        "referencedColumn": fk_row.get("referenced_column"),
                        "onUpdate": fk_row.get("on_update"),
                        "onDelete": fk_row.get("on_delete"),
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
