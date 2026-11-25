List database objects by kind: names, or full metadata keyed by name.

<usecase>
Use before writing SQL to discover object names and shapes. Prefer this over querying the catalog through `read`.
</usecase>

<examples>
✓ "What tables are there?" → objects()
✓ "What columns does orders have?" → objects(search="orders", detail="full")
✓ "Which triggers exist?" → objects(kind="trigger")
</examples>

<what_it_returns>
`detail: "names"`: `{"objects": [...]}`, a sorted array of name strings.
`detail: "full"`: `{"objects": {name: metadata}}`; the name is the key and is not repeated. Metadata per kind:
- `table`: `schema`, `kind`, `owner`, `comment`, `columns`, `constraints`, `indexes`, `triggers`.
- `view`: names only; `search` and `detail` are rejected.
- `trigger`: `schema`, `table`, `definition` (the stored `CREATE TRIGGER` text).
</what_it_returns>

<pagination>
Keep `kind`, `search`, and `detail` the same across pages.
</pagination>
