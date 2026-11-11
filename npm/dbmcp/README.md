# @haymon-ai/dbmcp

Database MCP server for MySQL, MariaDB, PostgreSQL & SQLite, as a single native binary.

This package is a thin launcher: npm installs the prebuilt binary for your platform as an optional dependency (`@haymon-ai/dbmcp-<os>-<arch>`), and `dbmcp` runs it.

Supported platforms: Linux x64 and arm64 (glibc 2.34+ and libstdc++ from GCC 12+), macOS arm64, Windows x64.

## Usage

```json
{
  "mcpServers": {
    "dbmcp": {
      "command": "npx",
      "args": ["-y", "@haymon-ai/dbmcp"],
      "env": {
        "DBMCP_DB_BACKEND": "mysql",
        "DBMCP_DB_USER": "root"
      }
    }
  }
}
```

See the [documentation](https://dbmcp.haymon.ai/) for every setting and the [repository](https://github.com/haymon-ai/dbmcp) for source and issues.
