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
