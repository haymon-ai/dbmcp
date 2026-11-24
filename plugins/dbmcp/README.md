# dbmcp plugin

[dbmcp](https://dbmcp.haymon.ai), the privacy first MCP server for SQL databases, as a Claude Code and Codex plugin. Works with MySQL, MariaDB, PostgreSQL, and SQLite.

## Install

Claude Code:

```text
/plugin marketplace add haymon-ai/marketplace
/plugin install dbmcp@haymon-ai
```

Codex:

```text
codex plugin marketplace add haymon-ai/marketplace
codex plugin add dbmcp@haymon-ai
```

Claude Code prompts for connection settings on install: fill in the quick-connect fields, or point it at a `dbmcp.yaml` and an optional env file. In Codex, the **Set up** button walks you through the connection.

## Skills

- `setup`: connects dbmcp to your database and fixes a server that fails to start.
- `privacy`: writes column rules and tunes sensitive-data redaction.

## Defaults

The server is read-only and redacts sensitive data in query results (`full` mode) unless your own config says otherwise. The plugin never changes either default.

## Requirements

- Node.js 18 or later.
- Claude Code 2.1.119 or later.
- Codex 0.122.0 or later; the post-install **Set up** button needs 0.156.0.
- The first start downloads about 16 MB once; it may time out on slow links.

## Links

- [Claude Code setup](https://dbmcp.haymon.ai/clients/claude-code)
- [Codex setup](https://dbmcp.haymon.ai/clients/codex)
- [Tools](https://dbmcp.haymon.ai/tools)

## License

MIT
