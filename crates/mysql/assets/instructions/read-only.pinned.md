## Workflow

1. Call `objects` to discover tables, views, triggers, functions, and procedures before writing SQL.
2. Use `read` for read-only SQL (SELECT, SHOW, DESCRIBE, EXPLAIN).
3. Use `explain` to analyze query execution plans and diagnose slow queries.

## Constraints

- This server is scoped to a single configured database; `objects` does not offer `kind: "database"`.
- The server runs in read-only mode. Data and schema changes are not possible.
- Multi-statement queries are not supported. Send one statement per request.
