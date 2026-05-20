List materialized views in the `public` schema, optionally filtered and/or with full metadata. Unlike regular views, materialized views store their results physically and must be refreshed explicitly. Regular views and system-schema matviews are excluded.

<usecase>
Use when:
- Auditing materialized views across a database (brief mode, default).
- Searching for a matview by partial name (pass `search`).
- Inspecting a matview's owner, comment, full SELECT body, populated state, and index presence before querying or refreshing it (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `pg_matviews` / `pg_class`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on matview names via `ILIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by bare matview name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What materialized views are in mydb?" → listMaterializedViews(database="mydb")
✓ "Find the recent-orders matview" → listMaterializedViews(search="orders")
✓ "What does mv_orders_by_region compute?" → listMaterializedViews(search="mv_orders_by_region", detailed=true)
✓ "Has the cache matview ever been refreshed?" → listMaterializedViews(search="cache", detailed=true) — read `populated`
✓ "Which matviews can I refresh concurrently?" → listMaterializedViews(detailed=true) — read `indexed` (CONCURRENTLY additionally needs a unique index)
✗ "List regular views" → use listViews instead
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of matview-name strings, e.g. `["mv_archived_orders", "mv_recent_orders"]`. Matview names are unique per schema, so no duplicates appear.
Detailed mode: a JSON object keyed by bare matview name; each value carries:
- `schema` — schema name (always `"public"` in this build).
- `owner` — owning role's name from `pg_matviews.matviewowner`.
- `description` — `COMMENT ON MATERIALIZED VIEW` text, or `null` when no comment.
- `definition` — the SELECT body verbatim from `pg_matviews.definition`, with no `CREATE MATERIALIZED VIEW` wrapper.
- `populated` — `true` once the matview has been refreshed at least once. `false` for matviews created `WITH NO DATA` and never refreshed; querying such a matview returns zero rows until `REFRESH MATERIALIZED VIEW` runs.
- `indexed` — `true` when at least one index exists on the matview. `REFRESH MATERIALIZED VIEW CONCURRENTLY` additionally requires a unique index; this tool reports the broader has-any-index signal.

The matview name is the map key only — it is not repeated inside the value. Detailed mode deliberately omits column metadata (`columns`), `tablespace`, storage parameters, and unique-index detection. Column shape is recoverable from the `definition` text or via `listTables(detailed=true)` since Postgres exposes matviews in `pg_class`.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity. Brief and detailed modes share the same `matviewname` row order, so a client can switch `detailed` between pages without losing position.
</pagination>