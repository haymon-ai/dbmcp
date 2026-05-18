## Workflow

1. Call `listTables` to see tables. Pass `search` to filter by name (case-insensitive substring). Pass `detailed: true` to get columns, constraints, indexes, and triggers in the same call — this supersedes the legacy `getTableSchema` workflow.
2. Call `listViews` to see views.
3. Call `listTriggers` to see triggers.
4. Call `listFunctions` to see stored functions.
5. Call `listProcedures` to see stored procedures.
6. Use `readQuery` for read-only SQL (SELECT, SHOW, DESCRIBE, USE, EXPLAIN).
7. Use `writeQuery` for data changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
8. Use `explainQuery` to analyze query execution plans and diagnose slow queries.
9. Use `dropTable` to remove a table from a database.

Per-database tools default to the active database; pass `database` to target another.

## Constraints

- This server is scoped to a single configured database; database management tools are not available.
- The `writeQuery` and `dropTable` tools are hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.
