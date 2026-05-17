## Workflow

1. Call `listTables` to discover tables in the connected database. Pass `search` to filter by name (case-insensitive substring). Pass `detailed: true` to get columns, constraints, indexes, and triggers in the same call — this supersedes the legacy `getTableSchema` workflow.
2. Call `listViews` to discover views in the connected database.
3. Call `listTriggers` to discover triggers in the connected database.
4. Use `readQuery` for read-only SQL (SELECT).
5. Use `writeQuery` for data changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
6. Use `explainQuery` to analyze query execution plans and diagnose slow queries.
7. Use `dropTable` to remove a table from the database.

## Constraints

- The `writeQuery` and `dropTable` tools are hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.