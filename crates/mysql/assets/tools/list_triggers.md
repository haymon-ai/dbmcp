List user-defined triggers in a database, optionally filtered and/or with full metadata.

<usecase>
Use when:
- Auditing trigger coverage across a database (brief mode, default).
- Searching for a trigger by partial name (pass `search`).
- Inspecting a trigger's timing, event, activation level, full `CREATE TRIGGER` text, and the session context active at trigger-creation time before reasoning about side-effects (pass `detailed: true`). Detailed mode supersedes ad-hoc `readQuery` against `information_schema.TRIGGERS`.
</usecase>

<parameters>
- `database` — Database to target. Defaults to the active database.
- `cursor` — Opaque pagination cursor; echo the prior response's `nextCursor`.
- `search` — Case-insensitive filter on trigger names via `LIKE`. `%` matches any sequence; `_` matches a single character.
- `detailed` — When `true`, returns full metadata objects keyed by trigger name instead of bare name strings. Default `false`.
</parameters>

<examples>
✓ "What triggers are in the mydb database?" → listTriggers(database="mydb")
✓ "Find the audit triggers" → listTriggers(search="audit")
✓ "What does orders_audit_after_insert do?" → listTriggers(search="orders_audit_after_insert", detailed=true)
✗ "Show me a trigger's body" → use detailed mode; the `definition` field carries the full `CREATE TRIGGER` text
</examples>

<what_it_returns>
Brief mode (default): a sorted JSON array of trigger-name strings, e.g. `["customers_audit_after_insert", "orders_audit_after_insert"]`.
Detailed mode: a JSON object keyed by trigger name; each value carries `schema`, `table`, `timing` (BEFORE/AFTER), `events` (single-element array — `INSERT`, `UPDATE`, or `DELETE`), `activationLevel` (always `ROW` on MySQL/MariaDB), `definition` (the canonical `CREATE TRIGGER` text including `DEFINER=` in `` `user`@`host` `` form), `sqlMode`, `characterSetClient`, `collationConnection`, and `databaseCollation`. The Postgres-only `status` and `functionName` fields are intentionally absent (no per-trigger enabled/disabled flag and no separate handler-function reference on MySQL/MariaDB); the four session-context fields are MySQL/MariaDB-only additions versus the Postgres detailed payload.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. The `search` filter must stay the same across pages for cursor continuity.
</pagination>