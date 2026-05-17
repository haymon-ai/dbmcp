List tables in the connected database, optionally filtered and/or with full metadata.

<usecase>
Use when:
- Exploring a database to find relevant tables (brief mode, default).
- Searching for a table by partial name (pass `search`).
- Inspecting a table's columns, constraints, indexes, and triggers before writing a query (pass `detailed: true`). This supersedes the legacy `getTableSchema` workflow.
</usecase>

<parameters>
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on table names via `LIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by table name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What tables are in this database?" → listTables()
✓ "Find the orders table" → listTables(search="order")
✓ "What columns does orders have?" → listTables(search="orders", detailed=true)
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of table-name strings, e.g. `["customers", "orders"]`.
Detailed mode: a JSON object keyed by table name; each value carries `schema`, `kind`, `owner`, `comment`, `columns`, `constraints`, `indexes`, and `triggers` (the name is the key and is not repeated inside the value). Keys iterate in the same alphabetical order brief mode uses. Example: `{"orders": {"schema": "main", "kind": "TABLE", ...}}`.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity.
</pagination>