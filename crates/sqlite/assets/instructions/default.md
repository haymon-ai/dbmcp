## Workflow

1. Call `objects` to discover tables, views, and triggers before writing SQL. Pass `kind` to pick the object kind (default `table`), `search` to filter by name, and `detail: "full"` for columns, constraints, indexes, and definitions.
2. Use `read` for read-only SQL (SELECT).
3. Use `write` for data and schema changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
4. Use `explain` to analyze query execution plans and diagnose slow queries.

## Constraints

- The `write` tool is hidden when read-only mode is active.
- Multi-statement queries are not supported. Send one statement per request.
