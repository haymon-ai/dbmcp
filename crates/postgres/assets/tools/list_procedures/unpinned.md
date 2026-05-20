List user-defined procedures in the `public` schema, optionally filtered and/or with full metadata. Functions, aggregates, and window functions are excluded.

<usecase>
Use when:
- Auditing procedures across a database (brief mode, default).
- Searching for a procedure by partial name (pass `search`).
- Inspecting a procedure's language, signature, security mode, owner, comment, and full `CREATE PROCEDURE` text before reasoning about correctness or invocation safety (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `pg_proc` / `information_schema.routines`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on procedure names via `ILIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by `name(arguments)` instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What procedures are in mydb?" → listProcedures(database="mydb")
✓ "Find the order archival routine" → listProcedures(search="archive")
✓ "What does archive_order do?" → listProcedures(search="archive_order", detailed=true)
✗ "List functions" → use listFunctions instead
✗ "List aggregates" → not supported; aggregates are excluded
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of procedure-name strings, e.g. `["archive_order", "archive_order_history", "archive_order_history"]`. Overloaded procedures appear as one entry per overload (duplicate name strings allowed).
Detailed mode: a JSON object keyed by procedure signature `name(arguments)`; each value carries `schema`, `name`, `language`, `arguments`, `security` (INVOKER/DEFINER), `owner`, `description` (or null when no `COMMENT ON PROCEDURE`), and `definition` (the full `CREATE OR REPLACE PROCEDURE` text). Overloads occupy distinct keys (e.g. `archive_order_history(integer)` vs `archive_order_history(integer, boolean)`). Zero-arg procedures key as `name()` — the parens are always present so the key shape stays uniform.

Detailed mode deliberately omits the `listFunctions`-only fields `returnType`, `volatility`, `strict`, and `parallelSafety`: procedures don't return a value, `pg_proc.provolatile` / `proisstrict` are not user-settable for procedures, and `proparallel` carries no procedure-level guarantee in PostgreSQL.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity. Brief and detailed modes share the same `(proname, oid)` row order, so a client can switch `detailed` between pages without losing position.
</pagination>