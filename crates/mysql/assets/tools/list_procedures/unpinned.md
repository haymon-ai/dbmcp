List user-defined stored procedures in a database, optionally filtered and/or with full metadata. Stored functions and loadable UDFs (`mysql.func`) are excluded.

<usecase>
Use when:
- Auditing stored procedures across a database (brief mode, default).
- Searching for a procedure by partial name (pass `search`).
- Inspecting a procedure's language, parameter list (with `IN`/`OUT`/`INOUT` modes), determinism, SQL-data-access classification, security mode, definer, comment, session context, and full reconstructed `CREATE PROCEDURE` text before reasoning about correctness or invocation safety (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `information_schema.ROUTINES` / `information_schema.PARAMETERS`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on procedure names via `LIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by procedure name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What procedures are in the mydb database?" → listProcedures(database="mydb")
✓ "Find the order archival routine" → listProcedures(search="archive")
✓ "What does archive_order do?" → listProcedures(search="archive_order", detailed=true)
✗ "List functions" → use listFunctions instead
✗ "List loadable UDFs from mysql.func" → not supported; only routines in information_schema.ROUTINES are returned
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of procedure-name strings, e.g. `["archive_order", "archive_order_history", "purge_order_archive", "touch_post"]`.
Detailed mode: a JSON object keyed by bare procedure name (MySQL/MariaDB do not allow procedure overloading, so no signature suffix is needed); each value carries `schema`, `language` (typically `"SQL"`; MariaDB external-language procedures report the external language name), `arguments` (comma-separated `MODE name type` triples from `information_schema.PARAMETERS` — `MODE` is one of `IN`, `OUT`, `INOUT`; empty string for zero-parameter procedures), `deterministic` (boolean), `sqlDataAccess` (one of `CONTAINS_SQL`, `NO_SQL`, `READS_SQL_DATA`, `MODIFIES_SQL_DATA`), `security` (`INVOKER` or `DEFINER`), `definer` (`user@host`), `description` (the `COMMENT` text or `null` when no comment was set — the empty string MySQL stores is coerced to JSON `null`), `definition` (the canonical reconstructed `CREATE PROCEDURE` text including `DEFINER=` in `` `user`@`host` `` form; no `RETURNS` clause — procedures have no return type), `sqlMode`, `characterSetClient`, `collationConnection`, and `databaseCollation`. Versus the Postgres `listProcedures` detailed payload: `volatility`, `parallelSafety`, `strict`, and `returnType` are intentionally absent (no MySQL/MariaDB analogues for the first three; procedures have no return type for the fourth — the Postgres-side payload also omits all four), `owner` is renamed to `definer` (more accurate for the MySQL `DEFINER` concept), keys are bare names rather than `name(arguments)` (no overloads possible), the four session-context fields (`sqlMode`, `characterSetClient`, `collationConnection`, `databaseCollation`) are MySQL/MariaDB-only additions, and `deterministic` plus `sqlDataAccess` are MySQL/MariaDB-only additions versus the Postgres `listProcedures` detailed payload (which omits both because Postgres procedures have no equivalent attributes).
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity.
</pagination>