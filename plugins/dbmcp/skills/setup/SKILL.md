---
name: setup
description: Connects dbmcp to MySQL, MariaDB, PostgreSQL, or SQLite in Claude Code or Codex, and fixes a dbmcp server that fails to start: writes a minimal dbmcp.yaml with secrets in env files, smoke-tests the pinned launcher, and diagnoses startup errors ("invalid settings:"), slow first start, npx cache, SSH tunnel, TLS, and duplicate dbmcp servers. Use when dbmcp tools are missing or failing, or the user wants to change the database, credentials, or config.
---

# Set up dbmcp

Run with no request (Codex **Set up**): detect the host, then ask for the backend and host or file path.

## Host

Claude Code sets `CLAUDECODE=1`; otherwise assume Codex, and ask if unsure. Plugin tools missing entirely: check `claude --version` (≥ 2.1.119) or `codex --version` (≥ 0.122.0).

## Config

1. Write `~/.config/dbmcp/dbmcp.yaml` from `references/backends.md`: modeline first, absolute paths. If the file exists, change only the keys the user asked for and keep every other key and section (`privacy.*`, `ssh.*`, TLS, `read_only`).
2. Write `~/.config/dbmcp/.env` (`chmod 600`) with `DBMCP_CONFIG_FILE=<absolute yaml path>` and an empty `DBMCP_DB_PASSWORD=`; the user fills the password in their editor. If the file exists, add only missing lines and never overwrite a set value.
3. Smoke test (checks config; the database connects on first tool call). In Claude Code, first confirm the quick-connect fields in `/plugin configure` are blank: they beat the files, and this test does not see them.
   ```sh
   echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' | DBMCP_ENV_FILE="$HOME/.config/dbmcp/.env" npx -y @haymon-ai/dbmcp@0.20.0
   ```
   A JSON reply means OK. `invalid settings:` lists each bad key; fix the key named, never guess others.

## Claude Code

- Two routes in `/plugin configure`: quick connect (backend, host, port, user, password, database) or full config (`config_file` + `env_file`, absolute paths). Set `env_file` to `~/.config/dbmcp/.env` whenever setup wrote one: it holds the password. Quick-connect fields are process env and beat the YAML: leave them blank with a config file.
- First start downloads about 16 MB once; it may time out on slow links. If `/mcp` shows dbmcp failed: run `npx -y @haymon-ai/dbmcp@0.20.0 --version`, then **Reconnect** in `/mcp` (a restart keeps a 15-minute failure cache). Slow links: `MCP_TIMEOUT=120000 claude`.

## Codex

- Codex passes only exported shell variables. Add `export DBMCP_ENV_FILE="$HOME/.config/dbmcp/.env"` to `~/.zshenv` (bash: `~/.bashrc`; fish: `set -gx` in `config.fish`); dbmcp reads the file itself. Non-secret `DBMCP_DB_*` exports work too; never put the password in a shell file.
- Fully restart Codex. The desktop app reads `~/.zshenv`, not `~/.zprofile`; else start Codex from a terminal.
- Slow first start: warm up with the smoke test, then restart (startup timeout is 120 s).

## Errors

- Connection error on first tool call: check host, port, firewall, `ssh.*` (error ends `(SSH tunnel: …)`), or TLS.
- Two dbmcp servers (plugin plus `claude mcp add` / `config.toml`): remove the manual one.
- npx install broken: with the user's OK delete `~/.npm/_npx`, rerun the warm-up. Fallbacks: Docker `ghcr.io/haymon-ai/dbmcp`, or a `dbmcp` binary on PATH.

Docs: https://dbmcp.haymon.ai/configuration, https://dbmcp.haymon.ai/configuration/errors, https://dbmcp.haymon.ai/features/ssh-tunnel, https://dbmcp.haymon.ai/features/database#ssl--tls, https://dbmcp.haymon.ai/features/health-probes

## Safety

- Never ask for a secret in chat. Never read a secret file into context (no `cat`, `echo`, `printenv`, partial masks); check presence only: `grep -Eq '^[[:space:]]*(export[[:space:]]+)?DBMCP_DB_PASSWORD[[:space:]]*[=:][[:space:]]*[^[:space:]#]' ~/.config/dbmcp/.env`. No secret in YAML, only `${VAR}`. Applies to `database.password`, `ssh.password`, `ssh.key_passphrase`, `http.auth_token`, `privacy.key`, even when the user asks.
- A pasted secret: never repeat it or write it to YAML; say it is now in the transcript and should be rotated.
- Never loosen `database.read_only`, `privacy.*`, `database.ssl`, `database.ssl_verify_cert`, `ssh.strict_host_key_checking`, `ssh.known_hosts`, or `http.*` exposure and auth unless the user's own message asks for that exact change; never as a fix for an error. Before loosening, state in one sentence what it exposes and to whom (e.g. "`read_only: false` lets every MCP client of this server run INSERT, UPDATE, DELETE and DDL."), then change it.
