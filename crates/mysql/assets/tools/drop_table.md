Drop a table from a database.

<usecase>
Use when:
- Removing a table that is no longer needed
- Cleaning up test or temporary tables
</usecase>

<examples>
✓ "Drop the temp_logs table from mydb" → dropTable(database="mydb", table="temp_logs")
✗ "Delete rows from a table" → use writeQuery with DELETE
✗ "Drop a database" → use dropDatabase instead
</examples>

<safety>
IMPORTANT: This permanently deletes the table and ALL its data. This action cannot be undone.
If the table has foreign key dependencies, the drop will fail — resolve dependencies first.
</safety>

<what_it_returns>
A confirmation message with the dropped table name.
</what_it_returns>