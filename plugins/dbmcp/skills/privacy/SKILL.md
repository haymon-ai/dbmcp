---
name: privacy
description: "Configures dbmcp's PII redaction: writes `privacy.rules` column rules (`table.column` keys, `entity`, `operator`) and tunes `privacy.key`, `privacy.operator`, `privacy.categories`, and the NER `privacy.model`. Use when the user wants a column masked, hashed, or redacted, wants redaction tokens stable across restarts or replicas, or asks why values appear as entity tokens."
---

# Configure dbmcp privacy

Edit the YAML dbmcp loads: `config_file` in `/plugin configure`, `DBMCP_CONFIG_FILE` in the environment, or (Codex) the `DBMCP_CONFIG_FILE` line of the env file `DBMCP_ENV_FILE` names, relative to that file's folder; print only that line (`grep -E '^[[:space:]]*(export[[:space:]]+)?DBMCP_CONFIG_FILE=' FILE`), never the file. None: run setup. Defaults: mode `full`, operator `hash`.

## Column rules

```yaml
privacy:
  rules:
    customer-email:           # a-z 0-9 _ -
      columns: [users.email]  # [[catalog.]schema.]table.column
      entity: EMAIL_ADDRESS   # data-type name
      operator: replace       # optional
```

- Exact keys (no `*`, quotes, spaces). Check each with `objects` (`detail: "full"`); unmatched keys are silently ignored.
- Short keys need `database.name` (PostgreSQL: any; MySQL/MariaDB: `table.column`; SQLite: never, it fills `main.main`); PostgreSQL fills `public`, not `search_path`.
- One rule per column. The entity only labels; every value is rewritten. Use a rule-only entity (`FREE_TEXT`, `MIXED_CONTENT`, `PASSWORD`, `AUTH_TOKEN`, `USER_ID`, `NATIONAL_ID`, …) where no detected type fits; never a stand-in.
- YAML only; the last config file setting `privacy.rules` wins whole. Applies under `rules` and `full`, not `none`.
- Tell the user: while rules exist, uncheckable SQL is refused, ruled columns work only in the select list, and migrations run outside dbmcp.

## Tokens

`hash` → `<EMAIL_ADDRESS_3f9a…>` (equal token = equal value), `replace` → `<EMAIL_ADDRESS>`, `mask` → `*` per character, `redact` → empty. A computed ruled column (`lower(email)`) reads `<ENTITY>`.

## Tuning

- `privacy.key` (≥ 32 bytes): stable `hash` tokens across restarts and replicas; makes tokens linkable across sessions. Add one only when the user asks for stable tokens. A key may already be set (adding one changes every token): check `privacy.key` in any YAML file dbmcp loads, `grep -Eq '^[[:space:]]*(export[[:space:]]+)?DBMCP_PRIVACY_KEY[[:space:]]*[=:][[:space:]]*[^[:space:]#]' FILE` on each env file (`env_file` or `DBMCP_ENV_FILE`; none: run setup), and `test -n "$DBMCP_PRIVACY_KEY"`. If none: add an empty `DBMCP_PRIVACY_KEY=` line to the env file (`chmod 600`) and tell the user to fill it themselves, e.g. with `openssl rand -hex 32`; never generate or write the key.
- `privacy.operator`: global detection and rules without `operator`.
- `privacy.categories`: kebab list (`personal`, `financial`, `government`, `contact`, `network`, `digital-identity`, `crypto`); unset = all.
- `privacy.model`: NER directory; adds names, places, organizations (`full`); a bad model stops startup.

Then reconnect (Claude Code: **Reconnect** in `/mcp`; Codex: restart).

Docs: https://dbmcp.haymon.ai/privacy/rules, https://dbmcp.haymon.ai/privacy/redaction, https://dbmcp.haymon.ai/privacy/machine-learning

## Safety

- Adding protection needs no request. Removing or narrowing a rule, fewer `categories`, a weaker operator, removing `privacy.model`, mode `rules`/`none`, or a new `privacy.key` needs the user's own message asking for that exact change; never as a fix for a refusal or redacted result. Before it, state in one sentence what it exposes and to whom (e.g. "Removing `customer-email` sends every `users.email` value to the model provider."), then change it.
- Never read `privacy.key` or write it into YAML.
