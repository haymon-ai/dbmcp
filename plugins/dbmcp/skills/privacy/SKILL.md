---
name: privacy
description: "Configures dbmcp's PII redaction: writes `privacy.rules` column rules (`table.column` keys, `entity`, `operator`) and tunes `privacy.key`, `privacy.operator`, `privacy.categories`, and the NER `privacy.model`. Use when the user wants a column masked, hashed, or redacted, wants redaction tokens stable across restarts or replicas, or asks why values appear as entity tokens. Adds protection only; never removes rules, narrows categories, weakens the operator, or turns redaction off unless the user explicitly asks."
---

# Configure dbmcp privacy

Edit the dbmcp YAML (often `~/.config/dbmcp/dbmcp.yaml`). Defaults: mode `full`, operator `hash`.

## Column rules

```yaml
privacy:
  rules:
    customer-email:           # a-z 0-9 _ -
      columns: [users.email]  # [[catalog.]schema.]table.column
      entity: EMAIL_ADDRESS   # data-type name
      operator: replace       # optional
```

- Exact keys (no `*`, quotes, spaces). Check each with `listTables`; a key matching nothing is silently ignored.
- Short keys need `database.name` (PostgreSQL: any; MySQL/MariaDB: `table.column`; SQLite: never, it fills `main.main`); PostgreSQL fills `public`, not `search_path`.
- One rule per column. The entity only labels; every value is rewritten.
- YAML only; the last config file setting `privacy.rules` wins whole. Applies under `rules` and `full`, not `none`.
- Tell the user: while rules exist, uncheckable SQL is refused (`column policy: …`), ruled columns work only in the select list, and migrations run outside dbmcp.

## Tokens

`hash` → `<EMAIL_ADDRESS_3f9a…>` (equal token = equal value), `replace` → `<EMAIL_ADDRESS>`, `mask` → `*` per character, `redact` → empty. A computed ruled column (`lower(email)`) reads `<ENTITY>`.

## Tuning

- `privacy.key` (≥ 32 bytes): stable `hash` tokens across restarts and replicas. No `.env`: run setup. If `grep -q '^DBMCP_PRIVACY_KEY=.' ~/.config/dbmcp/.env` fails, add one unseen: `printf 'DBMCP_PRIVACY_KEY=%s\n' "$(openssl rand -hex 32)" >> ~/.config/dbmcp/.env`.
- `privacy.operator`: global detection and rules without `operator`.
- `privacy.categories`: kebab list (`personal`, `financial`, `government`, `contact`, `network`, `digital-identity`, `crypto`); unset = all.
- `privacy.model`: NER directory; adds names, places, organizations (`full`); a bad model stops startup.

Then reconnect (Claude Code: **Reconnect** in `/mcp`; Codex: restart).

Docs: https://dbmcp.haymon.ai/privacy/rules, https://dbmcp.haymon.ai/privacy/redaction, https://dbmcp.haymon.ai/privacy/machine-learning

## Safety

- Adding rules or categories, or a stronger operator, needs no request. Removing or narrowing a rule, fewer `categories`, a weaker operator, mode `rules`/`none`, or a new `privacy.key` needs the user's own message asking for that exact change; never as a fix for a refusal or redacted result. Before it, state in one sentence what it exposes and to whom (e.g. "Removing `customer-email` sends every `users.email` value to the model provider."), then change it.
- Never read `privacy.key` or write it into YAML.
