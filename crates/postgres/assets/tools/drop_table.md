Drop a table from a database. Checks for foreign key dependencies via the database engine.

<usecase>
Use when:
- Removing a table that is no longer needed
- Cleaning up test or temporary tables
</usecase>

<examples>
✓ "Drop the temp_logs table" → dropTable(database="mydb", table="temp_logs")
✓ "Force drop with dependencies" → dropTable(..., cascade=true)
✗ "Delete rows from a table" → use writeQuery with DELETE
✗ "Drop a database" → use dropDatabase instead
</examples>

<safety>
IMPORTANT: This permanently deletes the table and ALL its data. This action cannot be undone.
Set `cascade` to true to also drop dependent foreign key constraints.
Without cascade, the drop will fail if other tables reference this one.
</safety>

<what_it_returns>
A confirmation message with the dropped table name.
</what_it_returns>