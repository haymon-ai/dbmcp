# Minimal dbmcp.yaml per backend

Every file starts with the schema modeline. Secrets never go in YAML: `DBMCP_DB_PASSWORD=` in the env file sets the password directly, or a value reads any variable with `${VAR:?}` (the `:?` stops startup when it is missing). Omit `name` on MySQL/PostgreSQL to see every database.

## MySQL

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/haymon-ai/dbmcp/master/crates/config/assets/dbmcp.schema.json
database:
  backend: mysql
  host: localhost
  port: 3306
  user: app_reader
  name: shop
```

## MariaDB

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/haymon-ai/dbmcp/master/crates/config/assets/dbmcp.schema.json
database:
  backend: mariadb
  host: localhost
  port: 3306
  user: app_reader
  name: shop
```

## PostgreSQL

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/haymon-ai/dbmcp/master/crates/config/assets/dbmcp.schema.json
database:
  backend: postgres
  host: localhost
  port: 5432
  user: app_reader
  name: shop
```

## SQLite

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/haymon-ai/dbmcp/master/crates/config/assets/dbmcp.schema.json
database:
  backend: sqlite
  name: /absolute/path/to/app.db
```

No host, port, user, or password. A relative path resolves against the YAML file's directory.

## SSH tunnel (MySQL, MariaDB, PostgreSQL)

`database.host` is then the host as the bastion sees it. Unknown host keys are rejected; add the bastion to `~/.ssh/known_hosts` first.

```yaml
ssh: { host: bastion.example.com, user: deploy, key: ~/.ssh/id_ed25519 }
```

Key passphrase: `DBMCP_SSH_KEY_PASSPHRASE=` in the env file, filled by the user.

## TLS (MySQL, MariaDB, PostgreSQL)

```yaml
database: { ssl: true, ssl_ca: /absolute/path/to/ca.pem }
```

Merge into the existing `database:` section. Client certificates: `ssl_cert` and `ssl_key`.
