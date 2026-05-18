## Workflow

1. Call `listDatabases` to discover available databases.
2. Call `listTables` to see tables. Pass `search` to filter by name (case-insensitive substring). Pass `detailed: true` to get columns, constraints, indexes, and triggers in the same call — this supersedes the legacy `getTableSchema` workflow.
3. Call `listViews` to see views.
4. Call `listTriggers` to see triggers.
5. Call `listFunctions` to see stored functions.
6. Call `listProcedures` to see stored procedures.
7. Use `readQuery` for read-only SQL (SELECT, SHOW, DESCRIBE, USE, EXPLAIN).
8. Use `writeQuery` for data changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
9. Use `explainQuery` to analyze query execution plans and diagnose slow queries.
10. Use `createDatabase` to create a new database.
11. Use `dropDatabase` to drop an existing database.
12. Use `dropTable` to remove a table from a database.

Per-database tools default to the active database; pass `database` to target another.

## Constraints

- The `writeQuery`, `createDatabase`, `dropDatabase`, and `dropTable` tools are hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.