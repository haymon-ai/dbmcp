Drop an existing database from the connected server.

<usecase>
Use when:
- Removing a database that is no longer needed
- Cleaning up test or temporary databases
</usecase>

<examples>
✓ "Drop the test_db database" → dropDatabase(database="test_db")
✗ "Drop a table" → use dropTable instead
</examples>

<safety>
IMPORTANT: This permanently deletes the database and ALL its data. This action cannot be undone.
Cannot drop the database you are currently connected to.
</safety>

<what_it_returns>
A confirmation message with the dropped database name.
</what_it_returns>