---
name: setup
description: Connects dbmcp to MySQL, MariaDB, PostgreSQL, or SQLite in Claude Code or Codex, and fixes a dbmcp server that fails to start: writes a minimal dbmcp.yaml with secrets in env files, smoke-tests the pinned launcher, and diagnoses startup errors ("invalid settings:"), slow first start, npx cache, SSH tunnel, TLS, and duplicate dbmcp servers. Use when dbmcp tools are missing or failing, or the user wants to change the database, credentials, or config.
---

# Set up dbmcp

No request (Codex **Set up**): detect host, ask backend and host or file path.

## Host

Claude Code sets `CLAUDECODE=1`; else Codex (ask if unsure). No plugin tools: need `claude --version` ≥ 2.1.119 or `codex --version` ≥ 0.122.0.

## Config file

Absolute paths; if it exists, change only the asked keys, keep the rest (`privacy.*`, `ssh.*`, TLS, `read_only`); print the path.

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/haymon-ai/dbmcp/master/crates/config/assets/dbmcp.schema.json
database:
  backend: postgres   # mysql | mariadb | postgres | sqlite
  host: localhost
  port: 5432          # mysql/mariadb: 3306
  user: app_reader
  name: shop          # omit = all databases; sqlite: absolute file path, no host/port/user
```

SSH: `ssh: { host: bastion, user: deploy, key: ~/.ssh/id_ed25519 }` (bastion in `~/.ssh/known_hosts`; `database.host` as it sees it). TLS: `ssl: true`, `ssl_ca: /abs/ca.pem` under `database:`.

Smoke test (config only; `<vars>` names the files):
```sh
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' | <vars> npx -y @haymon-ai/dbmcp@0.20.0
```
JSON reply = OK; on `invalid settings:` fix only the keys named.

## Claude Code

- Default, no file: user fills quick connect + Password in `/plugin configure`.
- More settings (`privacy.rules`, SSH, TLS): `mkdir -p "${CLAUDE_PLUGIN_DATA}"`, write `${CLAUDE_PLUGIN_DATA}/dbmcp.yaml`, smoke test (`DBMCP_CONFIG_FILE=`). User sets it as `config_file`, clears quick-connect fields but Password (they beat the file).
- Other secrets: `${CLAUDE_PLUGIN_DATA}/.env` (`chmod 600`), empty `DBMCP_SSH_KEY_PASSPHRASE=` / `DBMCP_PRIVACY_KEY=` for the user to fill; set as `env_file`.
- Slow first start (~16 MB): `npx -y @haymon-ai/dbmcp@0.20.0 --version`, then **Reconnect** in `/mcp`, not restart; or `MCP_TIMEOUT=120000 claude`.

## Codex

- `mkdir -p` `${CODEX_HOME:-$HOME/.codex}/plugins/data/dbmcp-haymon-ai`. Its `.env` (`chmod 600`): `DBMCP_CONFIG_FILE=dbmcp.yaml` (same folder) and an empty `DBMCP_DB_PASSWORD=` for the user to fill; add only missing lines. Smoke test with `DBMCP_ENV_FILE=<.env>`.
- Add `export DBMCP_ENV_FILE=<.env>` to `~/.zshenv` (or bash/fish equivalent); never the password.
- Restart Codex fully (desktop reads `~/.zshenv`, not `~/.zprofile`). Slow start: smoke test, restart.

## Errors

- Connection error: host, port, firewall, `ssh.*`, TLS.
- Duplicate dbmcp (plugin + manual): remove the manual one.
- Broken npx install: with the user's OK delete `~/.npm/_npx`, retry. Fallbacks: Docker `ghcr.io/haymon-ai/dbmcp`, `dbmcp` on PATH.

Docs: https://dbmcp.haymon.ai/configuration/errors

## Safety

- Never ask for a secret in chat or read a secret file into context (no `cat`/`echo`/`printenv`/masks); presence only: `grep -Eq '^[[:space:]]*(export[[:space:]]+)?NAME[[:space:]]*[=:][[:space:]]*[^[:space:]#]' <.env>`. YAML holds no secret, only `${VAR:?}`. Covers `database.password`, `ssh.password`, `ssh.key_passphrase`, `http.auth_token`, `privacy.key`, even when asked.
- Pasted secret: never repeat or store it; say it is in the transcript; rotate it.
- Never loosen `database.read_only`, `privacy.*`, `database.ssl`, `database.ssl_verify_cert`, `ssh.strict_host_key_checking`, `ssh.known_hosts`, or `http.*` exposure and auth unless the user's own message asks for that exact change; never as an error fix. First state in one sentence what it exposes and to whom (e.g. "`read_only: false` lets every MCP client write and run DDL.").
