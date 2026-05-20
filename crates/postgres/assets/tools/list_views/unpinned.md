List user-defined views in the `public` schema, optionally filtered and/or with full metadata. Materialized views and system-schema views are excluded.

<usecase>
Use when:
- Auditing views across a database (brief mode, default).
- Searching for a view by partial name (pass `search`).
- Inspecting a view's owner, comment, and full SELECT body before querying it (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `pg_views` / `pg_class`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on view names via `ILIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by bare view name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What views are in mydb?" → listViews(database="mydb")
✓ "Find the active-users view" → listViews(search="active")
✓ "What does active_users select?" → listViews(search="active_users", detailed=true)
✗ "Show me the columns of a view" → use listTables with `detailed: true` instead
✗ "List materialized views" → use listMaterializedViews
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of view-name strings, e.g. `["active_orders", "active_users"]`. View names are unique per schema, so no duplicates appear.
Detailed mode: a JSON object keyed by bare view name; each value carries `schema`, `owner`, `description` (or null when no `COMMENT ON VIEW`), and `definition` (the SELECT body verbatim from `pg_views.definition`, with no `CREATE VIEW` wrapper). The view name is the map key only — it is not repeated inside the value.

Detailed mode deliberately omits column metadata (`columns`), the `name` field (already the key), and view-level options (`security_barrier`, `security_invoker`, `WITH CHECK OPTION`). Column shape is recoverable from the `definition` text or via `listTables(detailed=true)` since Postgres exposes views in `pg_class`.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity. Brief and detailed modes share the same `viewname` row order, so a client can switch `detailed` between pages without losing position.
</pagination>