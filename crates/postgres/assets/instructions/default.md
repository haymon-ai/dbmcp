## Workflow

1. Call `objects` to discover tables, views, materialized views, triggers, functions, procedures (in the `public` schema), and databases before writing SQL.
2. Use `read` for read-only SQL (SELECT, SHOW, EXPLAIN).
3. Use `write` for data and schema changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP, CREATE DATABASE, DROP DATABASE).
4. Use `explain` to analyze query execution plans and diagnose slow queries.

Tools default to the active database; pass `database` to target another.

## Constraints

- While column rules (`privacy.rules`) exist, `write` refuses `CREATE DATABASE`, `DROP DATABASE`, and `DROP … CASCADE`.
- Multi-statement queries are not supported. Send one statement per request.
