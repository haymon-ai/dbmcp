Return the execution plan for a SQL query to diagnose performance. Use this tool instead of running EXPLAIN directly through readQuery — it provides structured JSON output.

<usecase>
Use when:
- A query runs slowly and you need to understand why
- Investigating performance bottlenecks
- Planning index creation to optimize queries
- Analyzing join methods, table scan strategies, and sort operations
</usecase>

<when_not_to_use>
- Running actual queries → use readQuery or writeQuery
- Checking table structure → use listTables(detailed=true)
</when_not_to_use>

<examples>
✓ "Why is my SELECT on orders slow?" → explainQuery(query="SELECT ...")
✓ "Should I add an index?" → explainQuery with analyze=true
✗ "Run this SELECT" → use readQuery
</examples>

<safety>
Set `analyze` to true for actual execution statistics (EXPLAIN ANALYZE).
IMPORTANT: EXPLAIN ANALYZE actually executes the query! In read-only mode, only read-only statements are allowed with analyze.
When analyze is false, returns EXPLAIN (FORMAT JSON) output without executing.
</safety>

<what_it_returns>
A JSON array of execution plan rows showing access methods, join types, row estimates, and costs.
</what_it_returns>