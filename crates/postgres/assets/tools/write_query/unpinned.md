Execute a write SQL query (INSERT, UPDATE, DELETE, CREATE, ALTER, DROP).

<usecase>
Use when:
- Inserting, updating, or deleting rows
- Creating or altering tables, indexes, views, or other schema objects
- Any data modification operation
</usecase>

<when_not_to_use>
- Read-only queries (SELECT, SHOW) → use readQuery
- Query performance analysis → use explainQuery
- Creating/dropping entire databases → use createDatabase or dropDatabase
</when_not_to_use>

<examples>
✓ "INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com')"
✓ "UPDATE orders SET status = 'shipped' WHERE id = 42"
✓ "CREATE TABLE logs (id SERIAL PRIMARY KEY, message TEXT)"
✗ "SELECT * FROM users" → use readQuery
</examples>

<what_it_returns>
An object with:
- `rows`: rows returned by a `RETURNING` clause, each keyed by column name; empty otherwise.
- `rowsAffected`: rows inserted, updated, or deleted. `0` for schema changes.

There is no `lastInsertId`: add `RETURNING id` to get generated keys. No follow-up `SELECT` is needed to confirm a write.

Long string values are cut and end in `…[truncated, N chars total]`; when that happens the response carries `truncated: true` and a `hint`.
</what_it_returns>