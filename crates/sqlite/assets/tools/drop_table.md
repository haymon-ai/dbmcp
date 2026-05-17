Drop a table from the database.

<usecase>
Use when:
- Removing a table that is no longer needed
- Cleaning up test or temporary tables
</usecase>

<examples>
✓ "Drop the temp_logs table" → dropTable(table="temp_logs")
✗ "Delete rows from a table" → use writeQuery with DELETE
</examples>

<safety>
IMPORTANT: This permanently deletes the table and ALL its data. This action cannot be undone.
</safety>

<what_it_returns>
A confirmation message with the dropped table name.
</what_it_returns>