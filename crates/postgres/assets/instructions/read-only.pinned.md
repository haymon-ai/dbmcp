## Workflow

1. Call `objects` to discover tables, views, materialized views, triggers, functions, and procedures (in the `public` schema) before writing SQL. Pass `kind` to pick the object kind (default `table`), `search` to filter by name, and `detail: "full"` for columns, constraints, indexes, and definitions.
2. Use `read` for read-only SQL (SELECT, SHOW, EXPLAIN).
3. Use `explain` to analyze query execution plans and diagnose slow queries.

Tools default to the active database.

## Constraints

- This server is scoped to a single configured database; `objects` does not offer `kind: "database"`.
- The server runs in read-only mode. Data and schema changes are not possible.
- Multi-statement queries are not supported. Send one statement per request.
