List user-defined triggers in the `public` schema, optionally filtered and/or with full metadata.

<usecase>
Use when:
- Auditing triggers across a database (brief mode, default).
- Searching for a trigger by partial name (pass `search`).
- Inspecting a trigger's timing, events, activation level, handler function, status, and full `CREATE TRIGGER` text before reasoning about side-effects (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `pg_trigger` / `information_schema.triggers`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on trigger names via `ILIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by trigger name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What triggers are in the mydb database?" → listTriggers(database="mydb")
✓ "Find the audit triggers" → listTriggers(search="audit")
✓ "What does orders_audit_after_iu do?" → listTriggers(search="orders_audit_after_iu", detailed=true)
✗ "Show me a trigger's body" → use detailed mode; the `definition` field carries the full `CREATE TRIGGER` text
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of trigger-name strings, e.g. `["customers_audit_after_insert", "orders_audit_after_insert"]`.
Detailed mode: a JSON object keyed by trigger name; each value carries `schema`, `table`, `status` (ENABLED/DISABLED/REPLICA/ALWAYS), `timing` (BEFORE/AFTER/INSTEAD OF), `events` (array of strings drawn from INSERT/UPDATE/DELETE/TRUNCATE in that fixed order), `activationLevel` (ROW/STATEMENT), `functionName`, and `definition` (the full `CREATE TRIGGER` text). Internal triggers (FK enforcement etc.) are excluded.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity.
</pagination>