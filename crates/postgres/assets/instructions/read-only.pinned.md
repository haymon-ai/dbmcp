## Workflow

1. Call `listTables` to see tables. Pass `search` to filter by name (case-insensitive substring). Pass `detailed: true` to get columns, constraints, indexes, and triggers in the same call — this supersedes the legacy `getTableSchema` workflow.
2. Call `listViews` to see views in the `public` schema.
3. Call `listTriggers` to see user-defined triggers in the `public` schema.
4. Call `listFunctions` to see user-defined functions in the `public` schema.
5. Call `listProcedures` to see user-defined procedures in the `public` schema.
6. Call `listMaterializedViews` to see materialized views in the `public` schema.
7. Use `readQuery` for read-only SQL (SELECT, SHOW, EXPLAIN).
8. Use `explainQuery` to analyze query execution plans and diagnose slow queries.

Per-database tools default to the active database; pass `database` to target another.

## Constraints

- This server is scoped to a single configured database; database management tools are not available.
- The server runs in read-only mode. Data and schema changes are not possible.
- Multi-statement queries are not supported. Send one statement per request.
