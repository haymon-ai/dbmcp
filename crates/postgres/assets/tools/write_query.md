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
A JSON array of affected/returning row objects, each keyed by column name.
</what_it_returns>