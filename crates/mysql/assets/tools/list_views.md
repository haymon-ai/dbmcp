List user-defined views in a database, optionally filtered and/or with full metadata. Base tables and system-schema views are excluded.

<usecase>
Use when:
- Auditing views across a database (brief mode, default).
- Searching for a view by partial name (pass `search`).
- Inspecting a view's definer, security mode, check-option level, updatable flag, session character set/collation, and full SELECT body before querying it (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `information_schema.VIEWS`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on view names via `LIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by view name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What views are in the mydb database?" → listViews(database="mydb")
✓ "Find the active-users view" → listViews(search="active")
✓ "What does active_users select?" → listViews(search="active_users", detailed=true)
✗ "Show me the columns of a view" → use listTables with `detailed: true` instead
✗ "List materialized views" → MySQL/MariaDB have no materialized-view concept
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of view-name strings, e.g. `["active_orders", "active_users", "published_posts"]`.
Detailed mode: a JSON object keyed by bare view name; each value carries `schema`, `definer` (`user@host`), `security` (`INVOKER` or `DEFINER`), `checkOption` (`NONE`, `CASCADED`, or `LOCAL`), `updatable` (boolean), `characterSetClient`, `collationConnection`, and `definition` (the SELECT body verbatim from `information_schema.VIEWS.VIEW_DEFINITION`, with no `CREATE VIEW` wrapper). The view name is the map key only — it is not repeated inside the value.

Versus the Postgres `listViews` detailed payload: `description` is intentionally absent (neither MySQL nor MariaDB exposes a user-comment column for views — `CREATE VIEW` syntax has no `COMMENT` clause), `algorithm` is intentionally absent (MariaDB-only column on `information_schema.VIEWS`), `owner` is renamed to `definer` (more accurate for the MySQL `DEFINER` concept), and the five MySQL/MariaDB-only structured fields (`security`, `checkOption`, `updatable`, `characterSetClient`, `collationConnection`) are added. The `definition` field shape is byte-identical to Postgres — raw SELECT body verbatim, no DDL wrapper. When the connected role lacks the `SHOW VIEW` privilege on a particular view, the engine redacts `VIEW_DEFINITION` to the empty string; the row remains in the response with `definition` reflecting that empty value.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity. Brief and detailed modes share the same `TABLE_NAME` row order, so a client can switch `detailed` between pages without losing position.
</pagination>