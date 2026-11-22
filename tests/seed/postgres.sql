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
    phone VARCHAR(40),
    ssn VARCHAR(20),
    tax_id VARCHAR(20),
    credit_card VARCHAR(25),
    iban VARCHAR(40),
    city VARCHAR(80),
    employer VARCHAR(120),
    nationality VARCHAR(60),
    bio TEXT,
    notes TEXT,
    metadata JSONB,
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
INSERT INTO users (name, email, phone, ssn, tax_id, credit_card, iban, city, employer, nationality, bio, notes, metadata) VALUES
    ('Alice Johnson', 'alice@example.com', '+14155552671', '078-05-1123', NULL, '4111111111111111', 'GB82WEST12345698765432', 'San Francisco', 'Acme Corporation', 'American',
     'Alice Johnson is a software engineer from San Francisco who works at Acme Corporation. An American citizen, she frequently visits the Golden Gate Bridge. Call her at +14155552671 or email alice@example.com.',
     'Customer Alice paid with card 4111111111111111. SSN on file 078-05-1123. Refund IBAN GB82WEST12345698765432. Callback phone +14155552671.',
     '{"contact":{"name":"Alice Johnson","email":"alice@example.com","phone":"+14155552671"},"address":{"city":"San Francisco","country":"USA"}}'),
    ('Bob Smith', 'bob@example.com', '+44 20 7946 0958', NULL, NULL, '5555555555554444', 'BE62510007547061', 'London', 'Siemens', 'British',
     'Bob Smith, a British national, lives in London and is employed at Siemens. He often meets clients at the British Museum. Reach Bob at bob@example.com; phone +44 20 7946 0958.',
     'Bob Smith called from phone +44 20 7946 0958 about a refund. Card 5555555555554444. No SSN on file.',
     '{"contact":{"name":"Bob Smith","email":"bob@example.com","phone":"+44 20 7946 0958"},"address":{"city":"London","country":"United Kingdom"}}'),
    ('Charlie Brown', 'charlie@example.com', '+49 30 12345678', NULL, '12345678903', '371449635398431', 'DE89370400440532013000', 'Berlin', 'Deutsche Bank', 'German',
     'Charlie Brown grew up in Berlin and works as an analyst at Deutsche Bank. A German citizen, Charlie volunteers at the Red Cross and often walks past the Brandenburg Gate. Email charlie@example.com or call +49 30 12345678.',
     'Charlie Brown German tax ID 12345678903. Paid via Amex card 371449635398431. IBAN DE89370400440532013000.',
     '{"contact":{"name":"Charlie Brown","email":"charlie@example.com","phone":"+49 30 12345678"},"address":{"city":"Berlin","country":"Germany"},"taxId":"12345678903"}');

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

-- Numeric round-trip fixtures — exercises spec 092 (issue #141). NUMERIC and
-- MONEY columns must come back through readQuery as real values with exact
-- precision. Includes a value beyond f64 precision to verify the
-- value-driven JSON shape rule (number when safe, string when out of range),
-- a MONEY value at i64::MAX cents (always string), and an explicit-NULL row.
CREATE TABLE numeric_samples (
    id         SERIAL PRIMARY KEY,
    label      TEXT NOT NULL,
    n_small    NUMERIC(12, 2),
    n_int      NUMERIC(10, 0),
    n_overflow NUMERIC(38, 10),
    f4         REAL,
    f8         DOUBLE PRECISION,
    m_small    MONEY,
    m_overflow MONEY
);

INSERT INTO numeric_samples (label, n_small, n_int, n_overflow, f4, f8, m_small, m_overflow) VALUES
    ('basic',         123.45,  42,  1.5,                              1.5::real,  2.5,     '$123.45',                  '$92233720368547758.07'),
    ('trailing_zero', 1.20,    10,  0.1,                              0.5::real,  1.0,     '$0.10',                    '$92233720368547758.07'),
    ('negative',     -99.99,  -7,  -123.45,                          -1.5::real, -2.5,     '-$99.99',                  '-$92233720368547758.08'),
    ('overflow',      0.01,    1,   12345678901234567890.1234567890,  1.5::real,  1e100,   '$1.00',                    '$92233720368547758.07'),
    ('all_null',      NULL,    NULL, NULL,                             NULL,       NULL,    NULL,                       NULL);

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

-- Additional fixtures for listProcedures search + detailed mode (spec 061):
-- exercises every metadata field (security/owner/description) and the
-- overload-disambiguation contract for procedures (prokind='p').
-- `app_user` role already created above by spec 057's block.

CREATE OR REPLACE PROCEDURE archive_order(order_id integer)
LANGUAGE plpgsql
AS $$ BEGIN END; $$;
COMMENT ON PROCEDURE archive_order(integer) IS 'Moves an order into the archive table';
ALTER PROCEDURE archive_order(integer) OWNER TO app_user;

-- Overload pair: same name, different parameter signatures.
CREATE OR REPLACE PROCEDURE archive_order_history(order_id integer)
LANGUAGE plpgsql AS $$ BEGIN END; $$;
ALTER PROCEDURE archive_order_history(integer) OWNER TO app_user;

CREATE OR REPLACE PROCEDURE archive_order_history(order_id integer, soft_delete boolean)
LANGUAGE plpgsql AS $$ BEGIN END; $$;
ALTER PROCEDURE archive_order_history(integer, boolean) OWNER TO app_user;

-- SECURITY DEFINER procedure (must report security: "DEFINER").
-- Distinct name from the homonymous function `elevate_user(bigint)`.
CREATE OR REPLACE PROCEDURE elevate_user_proc(uid bigint)
LANGUAGE plpgsql
SECURITY DEFINER
AS $$ BEGIN END; $$;
COMMENT ON PROCEDURE elevate_user_proc(bigint) IS 'Privileged helper - runs as definer.';

-- Zero-arg procedure with no comment (description must be null in detailed mode;
-- detailed-map key must be `tmp_cleanup()`, not `tmp_cleanup`).
CREATE OR REPLACE PROCEDURE tmp_cleanup()
LANGUAGE plpgsql
AS $$ BEGIN END; $$;
ALTER PROCEDURE tmp_cleanup() OWNER TO app_user;

-- Multi-arg with defaults + IN / OUT / INOUT / VARIADIC.
-- PostgreSQL forbids OUT parameters after a defaulted IN parameter on
-- procedures, so `since_days` is non-defaulted here.
CREATE OR REPLACE PROCEDURE summarise_orders(
    IN tenant_id integer,
    IN since_days integer,
    OUT total integer,
    INOUT cursor_name text,
    VARIADIC tags text[]
)
LANGUAGE plpgsql
AS $$ BEGIN total := 0; END; $$;
ALTER PROCEDURE summarise_orders(integer, integer, text, text[]) OWNER TO app_user;

-- Additional fixtures for listViews search + detailed mode (spec 063):
-- exercises owner, comment-vs-no-comment, multi-line definition, single-quote
-- pass-through, and the materialized-view exclusion contract.
-- `app_user` role already created above by spec 057's block.

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'reporting_role') THEN
        CREATE ROLE reporting_role;
    END IF;
END $$;

-- Comment + non-default owner on the existing `active_users` view (defined
-- earlier in this file). The view itself is reused; this just attaches the
-- COMMENT ON VIEW that the detailed-mode tests expect.
COMMENT ON VIEW active_users IS 'Currently-active user accounts';
ALTER VIEW active_users OWNER TO app_user;

-- Plain view, no comment, app_user owner. Lets us exercise
-- `description: null` in detailed mode.
CREATE VIEW active_orders AS
    SELECT id, user_id, title FROM posts WHERE published = TRUE;
ALTER VIEW active_orders OWNER TO app_user;

-- View owned by a non-default role to verify per-view owner reporting.
CREATE VIEW archived_orders AS
    SELECT id, user_id, title FROM posts WHERE published = FALSE;
ALTER VIEW archived_orders OWNER TO reporting_role;

-- Multi-line / JOIN / CTE view to verify verbatim pg_views.definition pass-through.
CREATE VIEW user_order_summary AS
    WITH recent AS (
        SELECT user_id, count(*) AS n FROM posts GROUP BY user_id
    )
    SELECT u.id, u.name, COALESCE(r.n, 0) AS recent_orders
    FROM users u LEFT JOIN recent r ON r.user_id = u.id;
COMMENT ON VIEW user_order_summary IS 'Per-user aggregate of recent orders';
ALTER VIEW user_order_summary OWNER TO app_user;

-- View with a single-quote literal in its body to exercise the JSON
-- round-trip of pg_views.definition (no SQL escaping at the JSON layer).
CREATE VIEW audit_log AS
    SELECT id, name FROM users WHERE name = 'admin';
ALTER VIEW audit_log OWNER TO app_user;

-- Additional fixtures for listMaterializedViews search + detailed mode (spec 067):
-- exercises owner, comment-vs-no-comment, multi-line / CTE / single-quote
-- definition, populated-vs-WITH-NO-DATA, indexed-vs-no-index, regular-view
-- exclusion contract, and the *orders* search-filter set.

CREATE MATERIALIZED VIEW mv_orders_by_region AS
    WITH paid_orders AS (
        SELECT id, customer_id, total FROM orders WHERE status = 'paid'
    )
    SELECT customer_id AS region,
           count(*) AS order_count,
           sum(total) AS gross
    FROM paid_orders
    GROUP BY customer_id;
COMMENT ON MATERIALIZED VIEW mv_orders_by_region IS 'Orders rolled up by region for the BI dashboard.';
ALTER MATERIALIZED VIEW mv_orders_by_region OWNER TO app_user;
CREATE UNIQUE INDEX mv_orders_by_region_region_uniq ON mv_orders_by_region (region);

-- No comment, no index, owned by a distinct role.
CREATE MATERIALIZED VIEW mv_archived_orders AS
    SELECT id, customer_id, total FROM orders WHERE status = 'shipped';
ALTER MATERIALIZED VIEW mv_archived_orders OWNER TO reporting_role;

-- Created `WITH NO DATA` so detailed mode reports populated=false until
-- REFRESH MATERIALIZED VIEW runs. Owner stays as the seed-loading role so the
-- subsequent REFRESH the test issues has SELECT privilege on `orders`.
CREATE MATERIALIZED VIEW mv_pending_data AS
    SELECT id, customer_id, total FROM orders WHERE status = 'new'
WITH NO DATA;

-- Column-rules fixture: rules target `users` and `invoices.billing_email`;
-- `audit` stays unruled.
CREATE TABLE audit (
    id SERIAL PRIMARY KEY,
    user_id INT,
    action TEXT,
    note TEXT
);

INSERT INTO audit (user_id, action, note) VALUES
    (1, 'login', 'ok'),
    (2, 'logout', NULL);

CREATE TABLE invoices (
    id SERIAL PRIMARY KEY,
    user_id INT,
    billing_email TEXT
);

INSERT INTO invoices (user_id, billing_email) VALUES
    (1, 'alice@example.com'),
    (2, 'bob@example.com');

-- Trigger-free write targets whose `email` is hashed like `users.email`.
CREATE TABLE policy_hashed (
    id INT PRIMARY KEY,
    email TEXT
);

INSERT INTO policy_hashed (id, email) VALUES
    (1, 'alice@example.com'),
    (2, 'bob@example.com');

CREATE TABLE policy_hashed_copy (
    id INT PRIMARY KEY,
    email TEXT
);

CREATE TABLE policy_hashed_default (
    id INT PRIMARY KEY,
    email TEXT DEFAULT 'x@example.com'
);

CREATE TABLE policy_hashed_generated (
    id INT PRIMARY KEY,
    name TEXT,
    email TEXT GENERATED ALWAYS AS (name || '@example.com') STORED
);

-- `ON UPDATE CASCADE` copies `policy_cascade_parent.k` (unruled) into the
-- hashed `policy_cascade_child.k`; updating `policy_cascade_parent.j` fills
-- the hashed `policy_cascade_default.k` with its default (`ON UPDATE SET
-- DEFAULT`); deleting a parent row deletes rows of `policy_hashed_trigger`,
-- which has a trigger.
CREATE TABLE policy_cascade_parent (
    k TEXT PRIMARY KEY,
    j TEXT UNIQUE
);

INSERT INTO policy_cascade_parent (k, j) VALUES ('alice@example.com', 'a'), ('x@example.com', 'x@example.com');

CREATE TABLE policy_cascade_child (
    id INT PRIMARY KEY,
    k TEXT REFERENCES policy_cascade_parent (k) ON UPDATE CASCADE
);

INSERT INTO policy_cascade_child (id, k) VALUES (1, 'alice@example.com');

CREATE TABLE policy_cascade_default (
    id INT PRIMARY KEY,
    k TEXT DEFAULT 'x@example.com' REFERENCES policy_cascade_parent (j) ON UPDATE SET DEFAULT
);

INSERT INTO policy_cascade_default (id, k) VALUES (1, 'a');

-- A hashed `email` on a table with a trigger, below `policy_cascade_parent`.
CREATE TABLE policy_hashed_trigger (
    id INT PRIMARY KEY,
    email TEXT,
    k TEXT REFERENCES policy_cascade_parent (k) ON DELETE CASCADE
);

CREATE FUNCTION policy_hashed_trigger_fill() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RETURN NEW;
END $$;

CREATE TRIGGER policy_hashed_trigger_fill
    BEFORE INSERT ON policy_hashed_trigger
    FOR EACH ROW EXECUTE FUNCTION policy_hashed_trigger_fill();

-- `policy_deferred` fails every insert at COMMIT with a message quoting the e-mail.
CREATE TABLE policy_deferred (
    id SERIAL PRIMARY KEY,
    email TEXT
);

CREATE FUNCTION policy_deferred_reject() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'rejected %', NEW.email;
END $$;

CREATE CONSTRAINT TRIGGER policy_deferred_reject
    AFTER INSERT ON policy_deferred
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW EXECUTE FUNCTION policy_deferred_reject();

CREATE VIEW users_with_ssn AS
    SELECT id, email FROM users WHERE ssn IS NOT NULL;

CREATE VIEW user_emails AS
    SELECT id, email FROM users;

-- Inheritance fixture: legacy `inh_parent` ← `inh_child` (which also
-- inherits `inh_other`) ← `inh_grandchild`; partitioned `ptab` ← `ptab_1`,
-- `ptab_2`. `ptab_2` drops a column before it is attached, so its attnums
-- differ from `ptab`'s.
CREATE TABLE inh_parent (
    id INT,
    email TEXT,
    note TEXT
);
CREATE TABLE inh_other (
    email TEXT
);
CREATE TABLE inh_child (
    extra TEXT
) INHERITS (inh_parent, inh_other);
CREATE TABLE inh_grandchild () INHERITS (inh_child);

INSERT INTO inh_parent (id, email, note) VALUES (1, 'inh-parent@example.com', 'parent');
INSERT INTO inh_child (id, email, note, extra) VALUES (2, 'inh-child@example.com', 'child', 'x');
INSERT INTO inh_grandchild (id, email, note, extra) VALUES (3, 'inh-grandchild@example.com', 'grandchild', 'y');
INSERT INTO inh_other (email) VALUES ('inh-other@example.com');

CREATE TABLE ptab (
    id INT,
    email TEXT,
    note TEXT
) PARTITION BY RANGE (id);
CREATE TABLE ptab_1 PARTITION OF ptab FOR VALUES FROM (0) TO (100);
CREATE TABLE ptab_2 (
    gone INT,
    note TEXT,
    email TEXT,
    id INT
);
ALTER TABLE ptab_2 DROP COLUMN gone;
ALTER TABLE ptab ATTACH PARTITION ptab_2 FOR VALUES FROM (100) TO (200);

INSERT INTO ptab (id, email, note) VALUES
    (1, 'ptab-one@example.com', 'one'),
    (2, 'ptab-two@example.com', 'two'),
    (101, 'ptab-hundred@example.com', 'hundred');

CREATE VIEW inh_parent_view AS
    SELECT id, email FROM inh_parent;

CREATE VIEW ptab_view AS
    SELECT id, email FROM ptab;

-- Search-path hijack: `shadow.users` precedes `public.users` for
-- `policy_shadow`, so a bare `users` must bind to the shadow table.
CREATE SCHEMA shadow;
CREATE TABLE shadow.users (
    id INT PRIMARY KEY,
    email TEXT,
    ssn TEXT,
    bio TEXT
);
INSERT INTO shadow.users (id, email, ssn, bio) VALUES
    (1, 'shadow@example.com', '999-99-9999', NULL);

-- `policy_lexical` connects with `standard_conforming_strings = off`;
-- `policy_temp` lists `pg_temp` first in its `search_path`.
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'policy_shadow') THEN
        CREATE ROLE policy_shadow LOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'policy_lexical') THEN
        CREATE ROLE policy_lexical LOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'policy_temp') THEN
        CREATE ROLE policy_temp LOGIN;
    END IF;
END $$;
ALTER ROLE policy_shadow SET search_path = shadow, public;
ALTER ROLE policy_lexical SET standard_conforming_strings = off;
ALTER ROLE policy_temp SET search_path = pg_temp, public;
GRANT USAGE ON SCHEMA shadow TO policy_shadow, policy_lexical, policy_temp;
GRANT SELECT ON ALL TABLES IN SCHEMA public, shadow TO policy_shadow, policy_lexical, policy_temp;
GRANT CREATE ON SCHEMA public TO policy_temp;

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
