## Workflow

1. Call `listTables` to discover tables in the connected database. Pass `search` to filter by name (case-insensitive substring). Pass `detailed: true` to get columns, constraints, indexes, and triggers in the same call — this supersedes the legacy `getTableSchema` workflow.
2. Call `listViews` to discover views in the connected database.
3. Call `listTriggers` to discover triggers in the connected database.
4. Use `readQuery` for read-only SQL (SELECT).
5. Use `explainQuery` to analyze query execution plans and diagnose slow queries.

## Constraints

- The server runs in read-only mode. Data and schema changes are not possible.
- Multi-statement queries are not supported. Send one statement per request.
