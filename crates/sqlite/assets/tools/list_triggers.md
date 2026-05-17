List triggers in the connected SQLite database, optionally filtered and/or with full metadata.

<usecase>
Use when:
- Auditing trigger coverage across a database (brief mode, default).
- Searching for a trigger by partial name (pass `search`).
- Inspecting a trigger's table and full `CREATE TRIGGER` text before reasoning about side-effects (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `sqlite_schema`.
</usecase>

<parameters>
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on trigger names via `LIKE` (SQLite's `LIKE` is ASCII-case-insensitive by default). `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by trigger name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What triggers are in this database?" → listTriggers()
✓ "Find the audit triggers" → listTriggers(search="audit")
✓ "What does orders_audit_after_insert do?" → listTriggers(search="orders_audit_after_insert", detailed=true)
✗ "Show me a trigger's body" → use detailed mode; the `definition` field carries the full `CREATE TRIGGER` text
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of trigger-name strings, e.g. `["customers_audit_after_insert", "orders_audit_after_insert"]`.
Detailed mode: a JSON object keyed by trigger name; each value carries exactly three fields — `schema` (always `"main"`), `table` (`sqlite_schema.tbl_name` — may be a view name for `INSTEAD OF` triggers), and `definition` (the original `CREATE TRIGGER` text from `sqlite_schema.sql`, byte-for-byte). Internal `sqlite_*` triggers are excluded.
The detailed payload deliberately diverges from the Postgres and MySQL/MariaDB `listTriggers` detailed payloads — `timing`, `events`, `activationLevel`, `status`, `functionName`, `sqlMode`, `characterSetClient`, `collationConnection`, `databaseCollation`, and `created` are absent. SQLite's catalogue does not expose those concepts as columns, and this tool deliberately avoids parsing the stored DDL to derive them; clients that need the timing or event keyword can read it off the prefix of `definition`.
Triggers whose stored `sqlite_schema.sql` is `NULL` (rare; produced by extension-generated rows or hand-edited catalogues) are silently omitted from detailed mode but still listed by name in brief mode.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity.
</pagination>