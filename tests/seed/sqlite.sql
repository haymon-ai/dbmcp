-- Seed schema and data for SQLite

CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
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
    metadata TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS posts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    title VARCHAR(255) NOT NULL,
    body TEXT,
    published INTEGER DEFAULT 0,
    CONSTRAINT fk_posts_user FOREIGN KEY (user_id) REFERENCES users(id)
);

CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name VARCHAR(50) NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS post_tags (
    post_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (post_id, tag_id),
    CONSTRAINT fk_post_tags_post FOREIGN KEY (post_id) REFERENCES posts(id),
    CONSTRAINT fk_post_tags_tag FOREIGN KEY (tag_id) REFERENCES tags(id)
);

-- Secondary indexes on posts for detailed-mode coverage (spec 046 US2).
CREATE UNIQUE INDEX IF NOT EXISTS posts_user_title_uidx ON posts (user_id, title);
CREATE INDEX IF NOT EXISTS posts_published_idx ON posts (published, id);

-- WITHOUT ROWID table — spec 046 edge case.
CREATE TABLE IF NOT EXISTS lookup_codes (
    code TEXT PRIMARY KEY,
    label TEXT
) WITHOUT ROWID;

-- FTS5 virtual table — spec 046 US2 virtual-table kind coverage.
CREATE VIRTUAL TABLE IF NOT EXISTS posts_fts USING fts5(title, body);

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
    (1, 'Getting Started with SQL', 'An introduction to SQL databases.', 1),
    (1, 'Advanced Queries', 'Deep dive into complex SQL queries.', 1),
    (2, 'Database Design', 'Best practices for schema design.', 1),
    (2, 'Draft Post', 'This is still a work in progress.', 0),
    (3, 'My First Post', 'Hello world from Charlie!', 0);

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

CREATE TABLE IF NOT EXISTS temporal (
    id INTEGER PRIMARY KEY,
    date DATE NOT NULL,
    time TIME NOT NULL,
    timestamp TIMESTAMP NOT NULL
);

-- Sample data: 1 temporal row
INSERT INTO temporal (id, date, time, timestamp) VALUES
    (1, '2026-04-20', '14:30:00', '2026-04-20 14:30:00');

-- Views

CREATE VIEW IF NOT EXISTS active_users AS
    SELECT id, name, email FROM users;

CREATE VIEW IF NOT EXISTS published_posts AS
    SELECT id, user_id, title FROM posts WHERE published = 1;

-- Column-rules fixture: rules target `users` and `orders.customer_email`;
-- `audit` stays unruled. `posts_fts` above is the FTS5 table.
CREATE TABLE IF NOT EXISTS audit (
    id INTEGER PRIMARY KEY,
    user_id INTEGER,
    action TEXT,
    note TEXT
);

INSERT INTO audit (id, user_id, action, note) VALUES
    (1, 1, 'login', 'ok'),
    (2, 2, 'logout', NULL);

CREATE TABLE IF NOT EXISTS orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER,
    customer_email TEXT
);

INSERT INTO orders (id, user_id, customer_email) VALUES
    (1, 1, 'alice@example.com'),
    (2, 2, 'bob@example.com');

CREATE VIEW IF NOT EXISTS users_with_ssn AS
    SELECT id, email FROM users WHERE ssn IS NOT NULL;

-- Trigger-free write targets whose `email` is hashed like `users.email`.
CREATE TABLE IF NOT EXISTS policy_hashed (
    id INTEGER PRIMARY KEY,
    email TEXT
);

INSERT INTO policy_hashed (id, email) VALUES
    (1, 'alice@example.com'),
    (2, 'bob@example.com');

CREATE TABLE IF NOT EXISTS policy_hashed_copy (
    id INTEGER PRIMARY KEY,
    email TEXT
);

-- Trigger-free write targets whose `phone` and `note` are masked like `users.phone`.
CREATE TABLE IF NOT EXISTS policy_masked (
    id INTEGER PRIMARY KEY,
    phone TEXT,
    note TEXT
);

INSERT INTO policy_masked (id, phone, note) VALUES
    (1, '+14155552671', 'first note'),
    (2, '+44 20 7946 0958', 'second note');

CREATE TABLE IF NOT EXISTS policy_masked_copy (
    id INTEGER PRIMARY KEY,
    phone TEXT
);

CREATE TABLE IF NOT EXISTS policy_hashed_default (
    id INTEGER PRIMARY KEY,
    email TEXT DEFAULT 'x@example.com'
);

CREATE TABLE IF NOT EXISTS policy_hashed_generated (
    id INTEGER PRIMARY KEY,
    name TEXT,
    email TEXT GENERATED ALWAYS AS (name || '@example.com')
);

-- `ON UPDATE CASCADE` copies `policy_cascade_parent.k` (unruled) into the
-- hashed `policy_cascade_child.k`; updating `policy_cascade_parent.j` fills
-- the hashed `policy_cascade_default.k` with its default (`ON UPDATE SET
-- DEFAULT`); deleting a parent row deletes rows of `policy_hashed_trigger`,
-- which has a trigger.
CREATE TABLE IF NOT EXISTS policy_cascade_parent (
    k TEXT PRIMARY KEY,
    j TEXT UNIQUE
);

INSERT INTO policy_cascade_parent (k, j) VALUES ('alice@example.com', 'a'), ('x@example.com', 'x@example.com');

CREATE TABLE IF NOT EXISTS policy_cascade_child (
    id INTEGER PRIMARY KEY,
    k TEXT REFERENCES policy_cascade_parent (k) ON UPDATE CASCADE
);

INSERT INTO policy_cascade_child (id, k) VALUES (1, 'alice@example.com');

CREATE TABLE IF NOT EXISTS policy_cascade_default (
    id INTEGER PRIMARY KEY,
    k TEXT DEFAULT 'x@example.com' REFERENCES policy_cascade_parent (j) ON UPDATE SET DEFAULT
);

INSERT INTO policy_cascade_default (id, k) VALUES (1, 'a');

-- A hashed `email` on a table with a trigger, below `policy_cascade_parent`.
CREATE TABLE IF NOT EXISTS policy_hashed_trigger (
    id INTEGER PRIMARY KEY,
    email TEXT,
    k TEXT REFERENCES policy_cascade_parent (k) ON DELETE CASCADE
);

CREATE TRIGGER IF NOT EXISTS policy_hashed_trigger_fill
    AFTER INSERT ON policy_hashed_trigger
BEGIN
    SELECT 1;
END;

-- Triggers

CREATE TRIGGER IF NOT EXISTS users_before_insert
    BEFORE INSERT ON users
    BEGIN
        SELECT CASE WHEN NEW.name IS NULL THEN RAISE(ABORT, 'name required') END;
    END;

CREATE TRIGGER IF NOT EXISTS posts_before_update
    BEFORE UPDATE ON posts
    BEGIN
        SELECT CASE WHEN NEW.title IS NULL THEN RAISE(ABORT, 'title required') END;
    END;

-- *_audit_* triggers — exercise FR-001 (search) and detailed mode.
-- The first one carries a literal newline + quote in its body so the detailed-mode
-- `definition` field can be asserted to round-trip multi-line bodies (spec edge case).
-- Bodies are intentionally side-effect-free SELECT statements so that the existing
-- write_query tests against `users` / `posts` keep their pre-feature semantics.
CREATE TRIGGER IF NOT EXISTS users_audit_after_insert
    AFTER INSERT ON users
    BEGIN
        SELECT 'a note
spans two lines';
    END;

CREATE TRIGGER IF NOT EXISTS users_audit_after_update
    AFTER UPDATE ON users
    BEGIN
        SELECT NEW.id;
    END;

CREATE TRIGGER IF NOT EXISTS users_audit_after_delete
    AFTER DELETE ON users
    BEGIN
        SELECT OLD.id;
    END;

CREATE TRIGGER IF NOT EXISTS posts_audit_after_insert
    AFTER INSERT ON posts
    BEGIN
        SELECT NEW.id;
    END;

-- INSTEAD OF trigger on a view — exercises spec edge case "trigger attached to a
-- view rather than a table"; `tbl_name` reports the view name in detailed mode.
CREATE TRIGGER IF NOT EXISTS published_posts_instead_of_delete
    INSTEAD OF DELETE ON published_posts
    BEGIN
        SELECT OLD.id;
    END;
