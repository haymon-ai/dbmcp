## Workflow

1. Call `objects` to discover tables, views, materialized views, triggers, functions, procedures (in the `public` schema), and databases before writing SQL.
2. Use `read` for read-only SQL (SELECT, SHOW, EXPLAIN).
3. Use `explain` to analyze query execution plans and diagnose slow queries.

Tools default to the active database; pass `database` to target another.

## Constraints

- The server runs in read-only mode. Data and schema changes are not possible.
- Multi-statement queries are not supported. Send one statement per request.
