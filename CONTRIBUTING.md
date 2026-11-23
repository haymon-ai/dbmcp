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
cargo clippy --workspace --tests -- -D warnings
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

### Adding a Setting

Settings have no CLI flag. Each one has a YAML key path and an env var, both declared in the `SETTINGS` table in `crates/config/src/env.rs`; the loader in `crates/config/src/loader.rs` reads and casts it.

- **YAML key**: the Rust field name in its section (`database.read_only`, `privacy.model`).
- **Env var**: hand-picked, not derived from the key path: `DBMCP_` + section prefix (`DB_`, `HTTP_`, `PRIVACY_`, `SSH_`; none for top-level settings) + a SCREAMING_SNAKE concept name. Deliberate exceptions are fine (`DBMCP_HTTP_DANGEROUSLY_ALLOW_UNAUTHENTICATED`). Never rename or alias an existing name.
- **YAML only**: a setting whose value has no sensible env form (a map, such as `privacy.rules`) is declared with `yaml_only` and has no env var; its table row shows "— (YAML only)".
- Add the new row to the all-settings table in `docs/content/docs/configuration/index.mdx`, and document it on the page that explains its feature.

### Dependencies

Adding or updating dependencies requires discussion. If your change needs a new crate, mention it in your PR description with a justification for why the dependency is necessary.

## Testing

### Running Tests

**Unit tests** run with [cargo-nextest](https://nexte.st/docs/installation/pre-built-binaries/) (config: `.config/nextest.toml`), together with the integration tests that need no database (`crates/privacy/tests`). nextest cannot run doc tests, so those stay on `cargo test`:

```bash
cargo nextest run --workspace
cargo test --doc --workspace       # doc tests
```

**Integration tests** run against real databases using Docker and need cargo-nextest. The `tests/run.sh` script manages the full matrix:

```bash
# Run the newest version per backend
./tests/run.sh

# Run every Supported version (the full matrix)
./tests/run.sh --all-versions

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

### Adding or Dropping a Database Version

The [Supported versions](https://dbmcp.haymon.ai/supported-versions) are listed by hand. Edit every list in one PR:

1. `tests/compose.yml`: one service per version.
2. `tests/run.sh`: the `MATRIX` entry, and the `newest` flag on the newest entry per backend.
3. `.github/workflows/ci.yml`: the backend job's quoted `version` list (such as `"26.10"`), and the `helm` job's PostgreSQL image when the newest PostgreSQL changes.
4. `crates/sql/src/support.rs`: the backend's Supported lines, which decide the startup warning.
5. `docs/content/docs/supported-versions.mdx`: the versions table. A `libsqlite3-sys` bump that changes the bundled SQLite line updates the SQLite entries the same way.

For a new version, check `crates/sql-lineage/src/functions/` for its new built-in functions: a function missing there makes column rules refuse the statement.

Commit messages:

- Add: `feat(<backend>): support <Backend> <version>`.
- Drop: `feat(<backend>): drop <Backend> <version> from supported versions`. The body names the upstream end-of-life date and says the version is now Best effort and logs a startup warning.
- Neither takes `!`: no setting changes and the server still runs. A later change that breaks an end-of-life version takes `!` and a `BREAKING CHANGE:` footer naming the version.

### CI Pipeline

CI automatically runs on every push and pull request:

1. Commit message validation — conventional commit format ([cocogitto](https://github.com/cocogitto/cocogitto))
2. `cargo fmt --check` — formatting
3. `cargo clippy --workspace --tests -- -D warnings` — linting
4. `cargo nextest run --workspace` — unit tests and `crates/privacy/tests` on Linux, Windows, and macOS
5. `cargo test --doc --workspace` — doc tests
6. Integration tests against every Supported version (see `ci.yml` and [Supported versions](https://dbmcp.haymon.ai/supported-versions)) and SQLite, run with nextest from a test archive built once with `cargo nextest archive`

All checks must pass before a PR can be merged.

## Commit Message Convention

This project requires [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) and uses [cocogitto](https://github.com/cocogitto/cocogitto) to enforce them locally and in CI.

### Setup (One-Time)

Install cocogitto and the commit-msg git hook:

```bash
cargo install --locked cocogitto
cog install-hook commit-msg
```

The hook validates every commit message against the conventional commit specification before allowing the commit. If your message doesn't conform, the commit is rejected with an error showing the expected format.

### Message Format

Every commit message must follow this format:

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

This project follows [Semantic Versioning 2.0.0](https://semver.org/). Releases are managed by maintainers.