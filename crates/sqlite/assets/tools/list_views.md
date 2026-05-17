List all views in the connected SQLite database.

<usecase>
Use when:
- Exploring what views exist in the database alongside tables
- Verifying a view exists before querying it
- The user asks what views are available
</usecase>

<examples>
✓ "What views are in this database?"
✓ "Does an active_users view exist?" → listViews to check
✗ "Show me the columns of a view" → use listTables with `detailed: true` instead
</examples>

<what_it_returns>
A sorted JSON array of view name strings.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page.
</pagination>