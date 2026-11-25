## Workflow

1. Call `objects` to discover tables, views, triggers, functions, procedures, and databases before writing SQL.
2. Use `read` for read-only SQL (SELECT, SHOW, DESCRIBE, EXPLAIN).
3. Use `write` for data and schema changes (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP, CREATE DATABASE, DROP DATABASE).
4. Use `explain` to analyze query execution plans and diagnose slow queries.

Tools default to the active database; pass `database` to target another.

## Constraints

- Multi-statement queries are not supported. Send one statement per request.
