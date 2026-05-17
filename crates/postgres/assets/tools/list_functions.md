List user-defined functions in the `public` schema, optionally filtered and/or with full metadata. Aggregates, window functions, and procedures are excluded.

<usecase>
Use when:
- Auditing functions across a database (brief mode, default).
- Searching for a function by partial name (pass `search`).
- Inspecting a function's language, signature, return type, volatility, strictness, security mode, parallel-safety, owner, comment, and full `CREATE FUNCTION` text before reasoning about correctness or invocation safety (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `pg_proc` / `information_schema.routines`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on function names via `ILIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by `name(arguments)` instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What functions are in mydb?" → listFunctions(database="mydb")
✓ "Find the order-total calculation" → listFunctions(search="order")
✓ "What does calc_order_total do?" → listFunctions(search="calc_order_total", detailed=true)
✗ "List stored procedures" → use listProcedures instead
✗ "List aggregates" → not supported; aggregates are excluded
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of function-name strings, e.g. `["audit_user_login", "calc_order_subtotal", "calc_order_total", "calc_order_total"]`. Overloaded functions appear as one entry per overload (duplicate name strings allowed).
Detailed mode: a JSON object keyed by function signature `name(arguments)`; each value carries `schema`, `name`, `language`, `arguments`, `returnType`, `volatility` (IMMUTABLE/STABLE/VOLATILE), `strict` (boolean), `security` (INVOKER/DEFINER), `parallelSafety` (SAFE/RESTRICTED/UNSAFE), `owner`, `description` (or null when no `COMMENT ON FUNCTION`), and `definition` (the full `CREATE OR REPLACE FUNCTION` text). Overloads occupy distinct keys (e.g. `calc_total(integer)` vs `calc_total(integer, numeric)`).
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity. Brief and detailed modes share the same `(proname, oid)` row order, so a client can switch `detailed` between pages without losing position.
</pagination>