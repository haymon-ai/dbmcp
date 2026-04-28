-- Seed schema and data for PostgreSQL
-- This file is mounted to /docker-entrypoint-initdb.d/ and auto-executed on first start

-- app database

DROP DATABASE IF EXISTS app;
CREATE DATABASE app;
\c app

CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    email VARCHAR(255) NOT NULL UNIQUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE posts (
    id SERIAL PRIMARY KEY,
    user_id INT NOT NULL,
    title VARCHAR(255) NOT NULL,
    body TEXT,
    published BOOLEAN DEFAULT FALSE,
    CONSTRAINT fk_posts_user FOREIGN KEY (user_id) REFERENCES users(id)
);

CREATE TABLE tags (
    id SERIAL PRIMARY KEY,
    name VARCHAR(50) NOT NULL UNIQUE
);

CREATE TABLE post_tags (
    post_id INT NOT NULL,
    tag_id INT NOT NULL,
    PRIMARY KEY (post_id, tag_id),
    CONSTRAINT fk_post_tags_post FOREIGN KEY (post_id) REFERENCES posts(id),
    CONSTRAINT fk_post_tags_tag FOREIGN KEY (tag_id) REFERENCES tags(id)
);

-- Sample data: 3 users
INSERT INTO users (name, email) VALUES
    ('Alice Johnson', 'alice@example.com'),
    ('Bob Smith', 'bob@example.com'),
    ('Charlie Brown', 'charlie@example.com');

-- Sample data: 5 posts
INSERT INTO posts (user_id, title, body, published) VALUES
    (1, 'Getting Started with SQL', 'An introduction to SQL databases.', TRUE),
    (1, 'Advanced Queries', 'Deep dive into complex SQL queries.', TRUE),
    (2, 'Database Design', 'Best practices for schema design.', TRUE),
    (2, 'Draft Post', 'This is still a work in progress.', FALSE),
    (3, 'My First Post', 'Hello world from Charlie!', FALSE);

-- Sample data: 4 tags
INSERT INTO tags (name) VALUES
    ('sql'),
    ('tutorial'),
    ('design'),
    ('beginner');

-- Sample data: 6 post_tag associations
INSERT INTO post_tags (post_id, tag_id) VALUES
    (1, 1),
    (1, 2),
    (1, 4),
    (2, 1),
    (3, 3),
    (3, 1);

CREATE TABLE temporal (
    id SERIAL PRIMARY KEY,
    "date" DATE NOT NULL,
    "time" TIME NOT NULL,
    "timestamp" TIMESTAMP NOT NULL,
    "timestamptz" TIMESTAMPTZ NOT NULL
);

-- Sample data: 1 temporal row
INSERT INTO temporal (id, "date", "time", "timestamp", "timestamptz") VALUES
    (1, '2026-04-20', '14:30:00', '2026-04-20 14:30:00', '2026-04-20 14:30:00+02:00');

-- Views (regular views only; materialized views are seeded below in US4)

CREATE VIEW active_users AS
    SELECT id, name, email FROM users;

CREATE VIEW published_posts AS
    SELECT id, user_id, title FROM posts WHERE published = TRUE;

-- Triggers

CREATE OR REPLACE FUNCTION users_before_insert_fn() RETURNS trigger AS $$
BEGIN
    NEW.name := trim(NEW.name);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER users_before_insert
    BEFORE INSERT ON users
    FOR EACH ROW EXECUTE FUNCTION users_before_insert_fn();

CREATE OR REPLACE FUNCTION posts_before_update_fn() RETURNS trigger AS $$
BEGIN
    NEW.title := trim(NEW.title);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER posts_before_update
    BEFORE UPDATE ON posts
    FOR EACH ROW EXECUTE FUNCTION posts_before_update_fn();

-- Stored functions (prokind='f') — distinct from trigger functions above, though
-- listFunctions enumerates all user-defined functions in `public` including the
-- trigger functions (which is fine — they are, in fact, user-defined functions).

CREATE OR REPLACE FUNCTION calc_total(n INT) RETURNS INT AS $$ SELECT n * 2 $$ LANGUAGE SQL;

CREATE OR REPLACE FUNCTION double_it(n INT) RETURNS INT AS $$ SELECT n + n $$ LANGUAGE SQL;

-- Stored procedures (prokind='p')

CREATE OR REPLACE PROCEDURE archive_user(uid INT) AS $$
    UPDATE users SET name = name || ' (archived)' WHERE id = uid;
$$ LANGUAGE SQL;

CREATE OR REPLACE PROCEDURE touch_post(pid INT) AS $$
    UPDATE posts SET title = title WHERE id = pid;
$$ LANGUAGE SQL;

-- Materialized views

CREATE MATERIALIZED VIEW mv_recent_orders AS
    SELECT id, title, published FROM posts WHERE published;

CREATE MATERIALIZED VIEW mv_user_cohort AS
    SELECT id, name FROM users;

-- Additional fixtures for listTables search + detailed mode (spec 043)
--
-- Exercises case-insensitive substring filter, literal-wildcard safety, and the
-- full detailed payload (PK, FK, UNIQUE, CHECK, multiple indexes, trigger,
-- comments, partitioned-table kind).

CREATE TABLE customers (
    id           BIGSERIAL PRIMARY KEY,
    email        TEXT NOT NULL UNIQUE,
    display_name TEXT
);
COMMENT ON TABLE customers IS 'End customers of the shop';
COMMENT ON COLUMN customers.email IS 'Login and contact email';

CREATE TABLE orders (
    id           BIGSERIAL PRIMARY KEY,
    customer_id  BIGINT NOT NULL REFERENCES customers(id),
    total        NUMERIC(12, 2) NOT NULL,
    status       TEXT NOT NULL DEFAULT 'new',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT orders_status_check CHECK (status IN ('new', 'paid', 'shipped'))
);
CREATE INDEX orders_customer_created_idx ON orders (customer_id, created_at);

CREATE TABLE erp_orders (
    id           BIGSERIAL PRIMARY KEY,
    external_ref TEXT
);

CREATE TABLE order_items (
    id       BIGSERIAL PRIMARY KEY,
    order_id BIGINT NOT NULL REFERENCES orders(id),
    sku      TEXT NOT NULL,
    qty      INTEGER NOT NULL
);

CREATE TABLE inventory (
    id  BIGSERIAL PRIMARY KEY,
    sku TEXT UNIQUE
);

CREATE OR REPLACE FUNCTION orders_audit_fn() RETURNS trigger AS $$
BEGIN
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER orders_audit_trigger
    AFTER INSERT OR UPDATE ON orders
    FOR EACH ROW EXECUTE FUNCTION orders_audit_fn();

-- Additional fixtures for listTriggers detailed mode (spec 052):
-- statement-level + disabled trigger, partitioned-parent trigger.

CREATE OR REPLACE FUNCTION block_inventory_delete_fn() RETURNS trigger AS $$
BEGIN
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER block_inventory_delete
    BEFORE DELETE ON inventory
    FOR EACH STATEMENT EXECUTE FUNCTION block_inventory_delete_fn();
ALTER TABLE inventory DISABLE TRIGGER block_inventory_delete;

-- Partitioned table exercises the `kind` field in the detailed payload.
CREATE TABLE logs (
    id        BIGSERIAL,
    logged_at TIMESTAMPTZ NOT NULL,
    payload   TEXT
) PARTITION BY RANGE (logged_at);

CREATE OR REPLACE FUNCTION logs_redact_fn() RETURNS trigger AS $$
BEGIN
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER logs_redact_before_insert
    BEFORE INSERT ON logs
    FOR EACH ROW EXECUTE FUNCTION logs_redact_fn();

-- Additional fixtures for listFunctions search + detailed mode (spec 057):
-- exercises every metadata field (volatility/strict/security/parallelSafety/
-- description) and the overload-disambiguation contract.

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'app_user') THEN
        CREATE ROLE app_user;
    END IF;
END $$;

CREATE OR REPLACE FUNCTION calc_order_subtotal(order_id integer)
RETURNS numeric LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
AS $$ SELECT 0::numeric $$;
COMMENT ON FUNCTION calc_order_subtotal(integer) IS 'Sums line items minus discounts';
ALTER FUNCTION calc_order_subtotal(integer) OWNER TO app_user;

CREATE OR REPLACE FUNCTION calc_order_total(order_id integer)
RETURNS numeric LANGUAGE sql IMMUTABLE STRICT
AS $$ SELECT 0::numeric $$;
COMMENT ON FUNCTION calc_order_total(integer) IS 'Sums line items for an order';
ALTER FUNCTION calc_order_total(integer) OWNER TO app_user;

CREATE OR REPLACE FUNCTION calc_order_total(order_id integer, tax_rate numeric)
RETURNS numeric LANGUAGE sql IMMUTABLE STRICT
AS $$ SELECT 0::numeric $$;
ALTER FUNCTION calc_order_total(integer, numeric) OWNER TO app_user;

CREATE OR REPLACE FUNCTION audit_user_login(uid bigint)
RETURNS void LANGUAGE plpgsql VOLATILE
AS $$ BEGIN END; $$;
ALTER FUNCTION audit_user_login(bigint) OWNER TO app_user;

CREATE OR REPLACE FUNCTION elevate_user(uid bigint)
RETURNS void LANGUAGE plpgsql VOLATILE
SECURITY DEFINER
AS $$ BEGIN END; $$;
COMMENT ON FUNCTION elevate_user(bigint) IS 'Privileged helper - runs as definer.';

CREATE OR REPLACE FUNCTION ratelimit_check(key text)
RETURNS boolean LANGUAGE sql STABLE STRICT PARALLEL SAFE
AS $$ SELECT true $$;
ALTER FUNCTION ratelimit_check(text) OWNER TO app_user;

CREATE OR REPLACE FUNCTION tmp_helper()
RETURNS integer LANGUAGE plpgsql
AS $$ BEGIN RETURN 42; END; $$;
ALTER FUNCTION tmp_helper() OWNER TO app_user;

CREATE OR REPLACE FUNCTION multi_arg_demo(
    a integer,
    b integer DEFAULT 0,
    OUT total integer,
    VARIADIC tags text[] DEFAULT ARRAY[]::text[]
)
LANGUAGE plpgsql
AS $$ BEGIN total := a + b; END; $$;
ALTER FUNCTION multi_arg_demo(integer, integer, text[]) OWNER TO app_user;

-- Aggregate + procedure: must NOT appear in listFunctions output (FR-010).
CREATE OR REPLACE FUNCTION sum_state(state numeric, val numeric)
RETURNS numeric LANGUAGE sql IMMUTABLE
AS $$ SELECT state + val $$;
CREATE AGGREGATE sum_demo(numeric) (SFUNC = sum_state, STYPE = numeric);

CREATE OR REPLACE PROCEDURE noop_proc()
LANGUAGE plpgsql
AS $$ BEGIN END; $$;

-- analytics database

DROP DATABASE IF EXISTS analytics;
CREATE DATABASE analytics;
\c analytics

CREATE TABLE events (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    payload TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO events (name, payload) VALUES
    ('signup', '{"user": "alice"}'),
    ('login', '{"user": "bob"}');

-- canary database (used by drop_database tests)

\c postgres
DROP DATABASE IF EXISTS canary;
CREATE DATABASE canary;
