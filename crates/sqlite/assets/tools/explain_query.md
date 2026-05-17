Return the execution plan for a SQL query to diagnose performance. Use this tool instead of running EXPLAIN directly through readQuery — it provides structured output via EXPLAIN QUERY PLAN.

<usecase>
Use when:
- A query runs slowly and you need to understand why
- Understanding how SQLite will scan tables and use indexes
- Deciding whether to add an index
</usecase>

<when_not_to_use>
- Running actual queries → use readQuery or writeQuery
- Checking table structure → use listTables with `detailed: true`
</when_not_to_use>

<examples>
✓ "Why is my SELECT on orders slow?" → explainQuery(query="SELECT ...")
✓ "How will SQLite execute this join?" → explainQuery
✗ "Run this SELECT" → use readQuery
</examples>

<what_it_returns>
A JSON array of EXPLAIN QUERY PLAN rows showing how SQLite will scan tables, use indexes, and order operations.
</what_it_returns>