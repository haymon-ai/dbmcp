Execute a write SQL query (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).

<usecase>
Use when:
- Inserting, updating, or deleting rows
- Creating or altering tables, indexes, views, or other schema objects
- Any data modification operation
</usecase>

<when_not_to_use>
- Read-only queries (SELECT) → use readQuery
- Query performance analysis → use explainQuery
</when_not_to_use>

<examples>
✓ "INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com')"
✓ "UPDATE orders SET status = 'shipped' WHERE id = 42"
✓ "CREATE TABLE logs (id INTEGER PRIMARY KEY, message TEXT)"
✗ "SELECT * FROM users" → use readQuery
</examples>

<what_it_returns>
An object with:
- `rows`: rows returned by a `RETURNING` clause, each keyed by column name; empty otherwise.
- `rowsAffected`: rows inserted, updated, or deleted. `0` for schema changes.
- `lastInsertId`: the rowid of the last row inserted by an `INSERT`/`REPLACE`. Absent after other statements; use `RETURNING` for `WITHOUT ROWID` tables and upserts.

No follow-up `SELECT` is needed to confirm a write.

Long string values are cut and end in `…[truncated, N chars total]`, and rows past the response size cap are dropped and counted in `rowsOmitted`; when either happens the response carries `truncated: true` and a `hint`.
</what_it_returns>