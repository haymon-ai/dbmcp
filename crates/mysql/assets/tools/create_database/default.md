Create a new database on the connected server.

<usecase>
Use when:
- Setting up a new database for a project or application
- The user asks to create a database
</usecase>

<examples>
✓ "Create a database called analytics" → createDatabase(database="analytics")
✗ "Create a table" → use writeQuery with CREATE TABLE
</examples>

<important>
Database name must be non-empty; backend reserved-character rules apply.
If the database already exists, returns a message indicating so without error.
</important>

<what_it_returns>
A confirmation message with the created database name.
</what_it_returns>