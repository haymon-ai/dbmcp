## Workflow

1. Call `listDatabases` to discover available databases.
2. Call `listTables` to see tables. Pass `search` to filter by name (case-insensitive substring). Pass `detailed: true` to get columns, constraints, indexes, and triggers in the same call — this supersedes the legacy `getTableSchema` workflow.
3. Call `listViews` to see views.
4. Call `listTriggers` to see triggers.
5. Call `listFunctions` to see stored functions.
6. Call `listProcedures` to see stored procedures.
7. Use `readQuery` for read-only SQL (SELECT, SHOW, DESCRIBE, USE, EXPLAIN).
8. Use `explainQuery` to analyze query execution plans and diagnose slow queries.

Per-database tools default to the active database; pass `database` to target another.

## Constraints

- The server runs in read-only mode. Data and schema changes are not possible.
- Multi-statement queries are not supported. Send one statement per request.
