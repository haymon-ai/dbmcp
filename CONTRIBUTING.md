# Contributing

This guide will walk you through everything you need to know to contribute effectively.

## Table of Contents
- [How to Contribute](#how-to-contribute)
- [Coding Standards](#coding-standards)
- [Testing](#testing)
- [Commit Message Convention](#commit-message-convention)
- [Pull Request Process](#pull-request-process)
- [Versioning & Releases](#versioning--releases)

## How to Contribute

### Reporting Bugs

If you find a bug, please open a GitHub issue with:

- A clear, descriptive title
- Steps to reproduce the problem
- Expected vs actual behavior
- Your environment (OS, Rust version, database backend)

### Suggesting Features

- **Small changes** (typos, minor fixes, small improvements): Open a pull request directly.
- **Large changes** (new features, architectural changes): Open an issue first to discuss the approach. This avoids wasted effort if the change doesn't align with project goals.

## Coding Standards

### Formatting

All code must be formatted with `rustfmt`. Run before committing:

```bash
cargo fmt
```

CI runs `cargo fmt --check` and will reject unformatted code.

### Linting

All code must pass Clippy with warnings treated as errors:

```bash
cargo clippy -- -D warnings
```

### Error Handling

- Use `thiserror` for custom error types. Define error enums with `#[derive(thiserror::Error)]`.
- Use `expect("reason")` instead of bare `unwrap()`. Every `expect` call must include a message explaining why the value should be present.
- Propagate errors with `?` rather than panicking.

### Documentation

This project follows the [Microsoft Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/agents/all.txt):

- **Module docs**: Every module must have a `//!` doc comment explaining its purpose.
- **Public items**: Every public function, struct, enum, and trait must have a `///` doc comment.
- **First sentence**: Keep the summary sentence under 15 words.
- **Error/Panic sections**: Include `# Errors` and `# Panics` sections in doc comments when applicable.

### Dependencies

Adding or updating dependencies requires discussion. If your change needs a new crate, mention it in your PR description with a justification for why the dependency is necessary.

## Testing

### Running Tests

**Unit tests**:

```bash
cargo test --lib
```

**Integration tests** run against real databases using Docker. The `tests/run.sh` script manages the full matrix:

```bash
# Run the full test matrix (MariaDB 12, MySQL 9, PostgreSQL 18, SQLite)
./tests/run.sh

# Filter to a specific backend
./tests/run.sh --filter mariadb
./tests/run.sh --filter postgres
./tests/run.sh --filter sqlite
```

The script handles container lifecycle, port assignment, seeding, and cleanup automatically. Docker must be installed and running.

### Writing Tests

- Write tests for every new feature and bug fix.
- Unit tests go alongside the code they test (in `#[cfg(test)]` modules).
- Integration tests go in the `tests/` directory, organized by database backend.
- Tests should be deterministic and not depend on external state beyond their declared setup.

### CI Pipeline

CI automatically runs on every push and pull request:

1. `cargo fmt --check` — formatting
2. `cargo clippy -- -D warnings` — linting
3. `cargo test --lib` — unit tests
4. Integration tests against MariaDB 12, MySQL 9, PostgreSQL 18, and SQLite

All checks must pass before a PR can be merged.

## Commit Message Convention

This project requires [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/). Every commit message must follow this format:

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

### Commit Types

| Type       | Description                                          |
|------------|------------------------------------------------------|
| `feat`     | A new feature                                        |
| `fix`      | A bug fix                                            |
| `docs`     | Documentation only changes                           |
| `style`    | Formatting, no code meaning change                   |
| `refactor` | Code change that neither fixes a bug nor adds a feature |
| `perf`     | A performance improvement                            |
| `test`     | Adding or correcting tests                           |
| `build`    | Changes to the build system or dependencies          |
| `ci`       | Changes to CI configuration files and scripts        |
| `chore`    | Other changes that don't modify src or test files    |
| `revert`   | Reverts a previous commit                            |

### Scopes

Scopes are optional and freeform. Common scopes in this project include:

- `config` — configuration and CLI parsing
- `db` — database backend logic
- `cli` — command-line interface
- `http` — HTTP transport layer

### Examples

```
feat(db): add connection pool timeout configuration

fix: resolve panic when SSL cert path is missing

docs(cli): update --help text for http subcommand

refactor(config): flatten nested SSL configuration struct

test(db): add integration tests for PostgreSQL backend
```

### Breaking Changes

Indicate breaking changes by adding `!` after the type/scope or by including a `BREAKING CHANGE:` footer:

```
feat(db)!: remove deprecated connection_url parameter

The connection_url parameter has been replaced by individual
DB_HOST, DB_PORT, DB_USER, and DB_PASSWORD environment variables.

BREAKING CHANGE: connection_url is no longer accepted.
Migrate to individual DB_* environment variables.
```

## Pull Request Process

### Before Submitting

1. Create a new branch from `master` for your changes.
2. Keep your PR focused on a single change. Don't mix unrelated fixes or features.
3. Run all checks locally before pushing.
4. Write or update tests for your changes.

### PR Guidelines

- **Title**: Use the conventional commit format (e.g., `fix(db): resolve connection timeout`). The PR title becomes the squash commit message.
- **Description**: Explain what the change does and why. Link to related issues with `Closes #123` or `Fixes #123`.
- **Scope**: One PR = one logical change. If you find yourself writing "and" in your PR title, consider splitting it.
- **Draft PRs**: Open a draft PR early if you want feedback on your approach before completing the work.

### Review Process

- All PRs require at least one review before merging.
- PRs are squash-merged to keep the main branch history clean.
- Address review feedback by pushing new commits (don't force-push during review).
- Once approved, the maintainer will merge your PR.

## Versioning & Releases

This project follows [Semantic Versioning 2.0.0](https://semver.org/):

- **MAJOR** (X.0.0): Breaking changes — incompatible API or behavior changes
- **MINOR** (0.X.0): New features — backwards-compatible additions
- **PATCH** (0.0.X): Bug fixes — backwards-compatible fixes

### Release Process

Releases are triggered by pushing a version tag (e.g., `v0.2.0`). The CI pipeline automatically builds release binaries for all supported platforms and creates a GitHub release.