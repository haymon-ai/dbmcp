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
- `view`: `schema`, `owner`, `description`, `definition` (the `SELECT` body).
- `materialized_view`: `schema`, `owner`, `description`, `definition`, `populated`, `indexed`.
- `trigger`: `schema`, `table`, `status`, `timing`, `events`, `activationLevel`, `functionName`, `definition`; internal triggers excluded.
- `function`: keyed by `name(arguments)` (one key per overload): `schema`, `name`, `language`, `arguments`, `returnType`, `volatility`, `strict`, `security`, `parallelSafety`, `owner`, `description`, `definition`.
- `procedure`: keyed by `name(arguments)`: `schema`, `name`, `language`, `arguments`, `security`, `owner`, `description`, `definition`.
</what_it_returns>

<pagination>
Keep `kind`, `search`, and `detail` the same across pages.
</pagination>
