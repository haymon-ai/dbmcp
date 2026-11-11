Execute a read-only SQL query. Allowed statements: SELECT, SHOW, DESCRIBE, EXPLAIN.

<usecase>
Use when:
- Querying data from tables (SELECT with WHERE, JOIN, GROUP BY, etc.)
- Aggregations: COUNT, SUM, AVG, GROUP BY, HAVING
- Listing server variables or status (SHOW)
- Viewing table structure (DESCRIBE)
</usecase>

<when_not_to_use>
- Data changes (INSERT, UPDATE, DELETE) → use writeQuery (not available in read-only mode)
- Query performance analysis → use explainQuery
- Discovering tables or columns → use listTables (pass detailed=true for column-level metadata)
- Switching database context (USE is rejected) → the database is fixed by configuration
</when_not_to_use>

<examples>
✓ "SELECT * FROM users WHERE status = 'active'"
✓ "SELECT COUNT(*) FROM orders GROUP BY region"
✓ "SHOW TABLES" or "DESCRIBE users"
✗ "INSERT INTO users ..." → use writeQuery (not available in read-only mode)
✗ "EXPLAIN SELECT ..." → use explainQuery for structured analysis
</examples>

<what_it_returns>
A JSON array of row objects, each keyed by column name.

Long string values are cut and end in `…[truncated, N chars total]`; read the rest with `SUBSTRING`/`SUBSTR`. Rows past the response size cap are dropped: a `SELECT` page ends early and `nextCursor` resumes at the first row left out, while other statements report the count in `rowsOmitted`. When either happens the response carries `truncated: true` and a `hint`.
</what_it_returns>

<pagination>
`SELECT` results are paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page. Order by a unique key so pages neither skip nor repeat rows. `SHOW`, `DESCRIBE`, and `EXPLAIN` return a single page and ignore `cursor`.
</pagination>

<timeout>
Each call stops at the configured query timeout. On a timeout, check the plan with explainQuery before retrying: filter on indexed columns, or limit inner result sets before joining or aggregating. For a query known to be slow, pass `timeout` (seconds, at least 1) to allow more time; values above the server maximum are rejected.
</timeout>