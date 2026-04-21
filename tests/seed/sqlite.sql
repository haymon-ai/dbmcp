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
