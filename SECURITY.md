# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in Database MCP, please report it responsibly. **Do not open a public issue.**

### How to Report

- **Email**: [security@haymon.ai](mailto:security@haymon.ai)
- **GitHub**: Use [Private Vulnerability Reporting](https://github.com/nicosalm/mcp/security/advisories/new) via the repository's Security tab

### What to Include

- A description of the vulnerability
- Steps to reproduce the issue
- Affected versions (if known)
- Any potential impact assessment

### Response Timeline

- **Acknowledgment**: Within 48 hours of receipt
- **Detailed response**: Within 1 week, including an assessment and planned next steps

## Supported Versions

Only the **latest release** receives security patches. If you are running an older version, please upgrade to the latest release.

| Version | Supported |
|---------|-----------|
| Latest  | Yes       |
| Older   | No        |

## Scope

Database MCP mediates between an AI assistant (via the Model Context Protocol) and a database. The following trust model defines what constitutes a valid security vulnerability.

### Trusted

These are assumed to be under the user's control and are **not** considered attack vectors:

- The host operating system and file system
- Database credentials provided by the user
- The MCP client (Claude Desktop, Cursor, etc.)
- Configuration provided via CLI flags or environment variables

### Untrusted

Exploits via these vectors **are** valid security vulnerabilities:

- **SQL input from the AI assistant** — the primary attack surface; the server must enforce read-only restrictions and prevent injection regardless of what the AI sends
- **Network traffic in HTTP transport mode** — wire-level attacks between client and server
- **Database server responses** — defence-in-depth; malformed responses should not cause crashes or information leaks

### Not Vulnerabilities

The following are expected behavior and **not** security issues:

- Read-only mode blocking write operations (this is a security feature)
- Identifier validation rejecting special characters in database or table names
- Connection failures due to incorrect credentials or unreachable databases

## Disclosure Policy

We follow a **90-day disclosure timeline**:

1. **Report received** — a handler is assigned and acknowledges receipt
2. **Confirmed** — the vulnerability is verified and affected versions identified
3. **Fix developed** — a patch is prepared privately
4. **Release** — the fix is published in a new release
5. **Public disclosure** — the vulnerability is disclosed publicly

Vulnerabilities are made public when the fix is released **or** 90 days after the initial report, whichever comes first. We may request a short extension if a fix is near completion at the 90-day mark.
