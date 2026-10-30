# Database MCP

A single-binary MCP server that exposes MySQL, MariaDB, PostgreSQL, and SQLite to MCP clients.

## Configuration

**Setting**:
One configurable value, known by its YAML key path and its `DBMCP_*` env name.
_Avoid_: option, flag, parameter

**Section**:
An optional group of settings (`http`, `pii`, `ssh`) that exists in the configuration only when activated.
_Avoid_: block, group

**Activation rule**:
A rule that decides from which settings are set whether an optional section exists, or rejects a section that is set but cannot exist.
_Avoid_: toggle check

**Validation rule**:
A rule that checks the values of a fully resolved configuration, within one section or across sections.
_Avoid_: constraint, check
