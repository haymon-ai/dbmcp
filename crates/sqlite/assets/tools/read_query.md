Execute a read-only SQL query. Allowed statements: SELECT, EXPLAIN.

<usecase>
Use when:
- Querying data from tables (SELECT with WHERE, JOIN, GROUP BY, etc.)
- Aggregations: COUNT, SUM, AVG, GROUP BY, HAVING
- Checking data existence or counts
</usecase>

<when_not_to_use>
- Data changes (INSERT, UPDATE, DELETE) → use writeQuery (not available in read-only mode)
- Query performance analysis → use explainQuery
- Discovering tables or columns → use listTables (pass `detailed: true` for columns)
</when_not_to_use>

<examples>
✓ "SELECT * FROM users WHERE status = 'active'"
✓ "SELECT COUNT(*) FROM orders GROUP BY region"
✗ "INSERT INTO users ..." → use writeQuery (not available in read-only mode)
✗ "EXPLAIN SELECT ..." → use explainQuery for structured analysis
</examples>

<what_it_returns>
A JSON array of row objects, each keyed by column name.

Long string values are cut and end in `…[truncated, N chars total]`; read the rest with `SUBSTRING`/`SUBSTR`. Rows past the response size cap are dropped: a `SELECT` page ends early and `nextCursor` resumes at the first row left out, while other statements report the count in `rowsOmitted`. When either happens the response carries `truncated: true` and a `hint`.
</what_it_returns>

<pagination>
`SELECT` results are paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. Order by a unique key so pages neither skip nor repeat rows. `EXPLAIN` returns a single page and ignores `cursor`.
</pagination>