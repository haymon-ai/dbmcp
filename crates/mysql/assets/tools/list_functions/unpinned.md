List user-defined SQL functions in a database, optionally filtered and/or with full metadata. Loadable UDFs (`mysql.func`) and stored procedures are excluded.

<usecase>
Use when:
- Auditing stored functions across a database (brief mode, default).
- Searching for a function by partial name (pass `search`).
- Inspecting a function's language, signature, return type, determinism, SQL-data-access classification, security mode, definer, comment, session context, and full reconstructed `CREATE FUNCTION` text before reasoning about correctness or invocation safety (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `information_schema.ROUTINES` / `information_schema.PARAMETERS`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on function names via `LIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by function name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What functions are in the mydb database?" → listFunctions(database="mydb")
✓ "Find the order-total calculation" → listFunctions(search="order")
✓ "What does calc_order_total do?" → listFunctions(search="calc_order_total", detailed=true)
✗ "List stored procedures" → use listProcedures instead
✗ "List loadable UDFs from mysql.func" → not supported; only routines in information_schema.ROUTINES are returned
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of function-name strings, e.g. `["calc_order_subtotal", "calc_order_total", "double_it"]`.
Detailed mode: a JSON object keyed by bare function name (MySQL/MariaDB do not allow function overloading, so no signature suffix is needed); each value carries `schema`, `language` (typically `"SQL"`; MariaDB external-language functions report the external language name), `arguments` (comma-separated `name type` pairs from `information_schema.PARAMETERS`, empty string for zero-parameter functions), `returnType` (full `DTD_IDENTIFIER` including length/precision/unsigned/enum/set members), `deterministic` (boolean), `sqlDataAccess` (one of `CONTAINS_SQL`, `NO_SQL`, `READS_SQL_DATA`, `MODIFIES_SQL_DATA`), `security` (`INVOKER` or `DEFINER`), `definer` (`user@host`), `description` (the `COMMENT` text or `null` when no comment was set — the empty string MySQL stores is coerced to JSON `null`), `definition` (the canonical reconstructed `CREATE FUNCTION` text including `DEFINER=` in `` `user`@`host` `` form), `sqlMode`, `characterSetClient`, `collationConnection`, and `databaseCollation`. Versus the Postgres detailed payload: `volatility`, `parallelSafety`, and `strict` are intentionally absent (no MySQL/MariaDB analogues), `owner` is renamed to `definer` (more accurate for the MySQL `DEFINER` concept), keys are bare names rather than `name(arguments)` (no overloads possible), and the four session-context fields (`sqlMode`, `characterSetClient`, `collationConnection`, `databaseCollation`) are MySQL/MariaDB-only additions.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity.
</pagination>