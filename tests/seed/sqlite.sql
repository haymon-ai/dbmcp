-- Seed schema and data for SQLite

CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name VARCHAR(100) NOT NULL,
    email VARCHAR(255) NOT NULL UNIQUE,
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
INSERT INTO users (name, email) VALUES
    ('Alice Johnson', 'alice@example.com'),
    ('Bob Smith', 'bob@example.com'),
    ('Charlie Brown', 'charlie@example.com');

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
