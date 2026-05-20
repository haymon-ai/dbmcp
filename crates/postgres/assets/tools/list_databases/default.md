List all accessible databases on the connected server. Use this tool to discover what databases are available before using other tools.

<usecase>
ALWAYS call this tool FIRST when:
- You need to explore what databases exist on the server
- You need a database name for listTables or query tools
- The user asks what data is available
</usecase>

<examples>
✓ "What databases are on this server?"
✓ "Show me what's available" → call listDatabases first
</examples>

<what_it_returns>
A sorted JSON array of database name strings.
</what_it_returns>

<pagination>
Paginated. Pass the prior response's `nextCursor` as `cursor` to fetch the next page.
</pagination>