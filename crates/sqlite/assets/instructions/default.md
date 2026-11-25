## Workflow

1. Call `objects` to discover tables, views, and triggers before writing SQL.
2. Use `read` for read-only SQL (SELECT).
3. Use `write` for data and schema changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).
4. Use `explain` to analyze query execution plans and diagnose slow queries.

## Constraints

- Multi-statement queries are not supported. Send one statement per request.
