List database objects by kind: names, or full metadata keyed by name.

<usecase>
Use before writing SQL to discover object names and shapes. Prefer this over querying the catalog through `read`.
</usecase>

<examples>
✓ "What tables are there?" → objects()
✓ "What columns does orders have?" → objects(search="orders", detail="full")
✓ "Which triggers exist?" → objects(kind="trigger")
✓ "Which databases exist?" → objects(kind="database")
</examples>

<what_it_returns>
`detail: "names"`: `{"objects": [...]}`, a sorted array of name strings.
`detail: "full"`: `{"objects": {name: metadata}}`; the name is the key and is not repeated. Metadata per kind:
- `table`: `schema`, `kind`, `owner`, `comment`, `columns`, `constraints`, `indexes`, `triggers`.
- `view`: `schema`, `definer`, `security`, `checkOption`, `updatable`, `characterSetClient`, `collationConnection`, `definition` (the `SELECT` body).
- `trigger`: `schema`, `table`, `timing`, `events`, `activationLevel`, `definition`, `sqlMode`, `characterSetClient`, `collationConnection`, `databaseCollation`.
- `function`: `schema`, `language`, `arguments`, `returnType`, `deterministic`, `sqlDataAccess`, `security`, `definer`, `description`, `definition`, `sqlMode`, `characterSetClient`, `collationConnection`, `databaseCollation`.
- `procedure`: `schema`, `language`, `arguments`, `deterministic`, `sqlDataAccess`, `security`, `definer`, `description`, `definition`, `sqlMode`, `characterSetClient`, `collationConnection`, `databaseCollation`.
- `database`: names only; `search` and `detail` are rejected.
</what_it_returns>

<pagination>
Keep `kind`, `search`, and `detail` the same across pages.
</pagination>
