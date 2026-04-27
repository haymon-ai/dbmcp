-- Seed schema and data for MySQL/MariaDB
-- This file is mounted to /docker-entrypoint-initdb.d/ and auto-executed on first start

-- app database

DROP DATABASE IF EXISTS `app`;
CREATE DATABASE `app`;

CREATE TABLE `app`.`users` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `name` VARCHAR(100) NOT NULL COMMENT 'Display name; trimmed by trigger on insert.',
    `email` VARCHAR(255) NOT NULL UNIQUE,
    `display_name` VARCHAR(400) GENERATED ALWAYS AS (CONCAT(`name`, ' <', `email`, '>')) STORED,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB;

CREATE TABLE `app`.`posts` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `user_id` INT NOT NULL,
    `title` VARCHAR(255) NOT NULL,
    `body` TEXT COMMENT 'Markdown-encoded post body.',
    `published` TINYINT(1) DEFAULT 0,
    CONSTRAINT `fk_posts_user` FOREIGN KEY (`user_id`) REFERENCES `app`.`users`(`id`),
    CONSTRAINT `posts_user_id_positive` CHECK (`user_id` > 0),
    UNIQUE KEY `posts_user_title_uidx` (`user_id`, `title`),
    KEY `posts_published_idx` (`published`, `id`),
    FULLTEXT KEY `posts_body_fts` (`body`)
) ENGINE=InnoDB COMMENT='Blog post entries.';

CREATE TABLE `app`.`tags` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `name` VARCHAR(50) NOT NULL UNIQUE
) ENGINE=InnoDB;

CREATE TABLE `app`.`post_tags` (
    `post_id` INT NOT NULL,
    `tag_id` INT NOT NULL,
    PRIMARY KEY (`post_id`, `tag_id`),
    CONSTRAINT `fk_post_tags_post` FOREIGN KEY (`post_id`) REFERENCES `app`.`posts`(`id`),
    CONSTRAINT `fk_post_tags_tag` FOREIGN KEY (`tag_id`) REFERENCES `app`.`tags`(`id`)
) ENGINE=InnoDB;

-- App sample data

INSERT INTO `app`.`users` (`name`, `email`) VALUES
    ('Alice Johnson', 'alice@example.com'),
    ('Bob Smith', 'bob@example.com'),
    ('Charlie Brown', 'charlie@example.com');

INSERT INTO `app`.`posts` (`user_id`, `title`, `body`, `published`) VALUES
    (1, 'Getting Started with SQL', 'An introduction to SQL databases.', 1),
    (1, 'Advanced Queries', 'Deep dive into complex SQL queries.', 1),
    (2, 'Database Design', 'Best practices for schema design.', 1),
    (2, 'Draft Post', 'This is still a work in progress.', 0),
    (3, 'My First Post', 'Hello world from Charlie!', 0);

INSERT INTO `app`.`tags` (`name`) VALUES
    ('sql'),
    ('tutorial'),
    ('design'),
    ('beginner');

INSERT INTO `app`.`post_tags` (`post_id`, `tag_id`) VALUES
    (1, 1),
    (1, 2),
    (1, 4),
    (2, 1),
    (3, 3),
    (3, 1);

CREATE TABLE `app`.`temporal` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `date` DATE NOT NULL,
    `time` TIME NOT NULL,
    `datetime` DATETIME NOT NULL,
    `timestamp` TIMESTAMP NOT NULL
) ENGINE=InnoDB;

-- Audit log written to by the posts_after_insert trigger.
CREATE TABLE `app`.`posts_audit` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `post_id` INT NOT NULL,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB;

-- Partitioned table — exercises the kind: "PARTITIONED_TABLE" detection path.
-- The primary key includes `year` because MySQL requires every UNIQUE key to
-- include the partitioning column.
CREATE TABLE `app`.`events_by_year` (
    `id` BIGINT NOT NULL AUTO_INCREMENT,
    `year` SMALLINT NOT NULL,
    `payload` TEXT,
    PRIMARY KEY (`id`, `year`)
) ENGINE=InnoDB
PARTITION BY RANGE (`year`) (
    PARTITION `p_pre_2025` VALUES LESS THAN (2025),
    PARTITION `p_future` VALUES LESS THAN MAXVALUE
);

-- Sample data: 1 temporal row
INSERT INTO `app`.`temporal` (`id`, `date`, `time`, `datetime`, `timestamp`) VALUES
    (1, '2026-04-20', '14:30:00', '2026-04-20 14:30:00', '2026-04-20 14:30:00');

-- Views

CREATE VIEW `app`.`active_users` AS
    SELECT `id`, `name`, `email` FROM `app`.`users`;

CREATE VIEW `app`.`published_posts` AS
    SELECT `id`, `user_id`, `title` FROM `app`.`posts` WHERE `published` = 1;

-- Triggers

CREATE TRIGGER `app`.`users_before_insert` BEFORE INSERT ON `app`.`users`
    FOR EACH ROW SET NEW.`name` = TRIM(NEW.`name`);

CREATE TRIGGER `app`.`posts_before_update` BEFORE UPDATE ON `app`.`posts`
    FOR EACH ROW SET NEW.`title` = TRIM(NEW.`title`);

CREATE TRIGGER `app`.`posts_after_insert` AFTER INSERT ON `app`.`posts`
    FOR EACH ROW INSERT INTO `app`.`posts_audit`(`post_id`) VALUES (NEW.`id`);

-- Audit-named triggers — exercise FR-001 search semantics. One per event
-- (INSERT, UPDATE, DELETE) on `posts` plus one on `users` for cross-table search.
CREATE TRIGGER `app`.`posts_audit_after_insert` AFTER INSERT ON `app`.`posts`
    FOR EACH ROW INSERT INTO `app`.`posts_audit`(`post_id`) VALUES (NEW.`id`);

CREATE TRIGGER `app`.`posts_audit_after_update` AFTER UPDATE ON `app`.`posts`
    FOR EACH ROW INSERT INTO `app`.`posts_audit`(`post_id`) VALUES (NEW.`id`);

CREATE TRIGGER `app`.`posts_audit_after_delete` AFTER DELETE ON `app`.`posts`
    FOR EACH ROW INSERT INTO `app`.`posts_audit`(`post_id`) VALUES (OLD.`id`);

-- Single-statement body with a literal newline + single quote inside a
-- string literal — exercises the spec edge case "trigger body contains
-- literal newlines or quote characters" without needing DELIMITER directives.
CREATE TRIGGER `app`.`users_audit_after_insert` AFTER INSERT ON `app`.`users`
    FOR EACH ROW INSERT INTO `app`.`posts_audit`(`post_id`)
    SELECT NEW.`id` FROM DUAL WHERE 'a note
spans two lines' IS NOT NULL;

-- Note: MySQL/MariaDB enforce per-schema trigger-name uniqueness via the
-- `(TRIGGER_SCHEMA, TRIGGER_NAME)` primary key on `information_schema.TRIGGERS`,
-- so `ORDER BY TRIGGER_NAME` alone is a total order for a single-schema
-- listing — no tiebreaker columns are needed and none are emitted. The
-- integration test asserts deterministic name-ordering across consecutive
-- calls as a regression guard.

-- Stored functions & procedures (single-statement bodies so no DELIMITER needed)

CREATE FUNCTION `app`.`calc_total`(n INT) RETURNS INT DETERMINISTIC RETURN n * 2;

CREATE FUNCTION `app`.`double_it`(n INT) RETURNS INT DETERMINISTIC RETURN n + n;

CREATE PROCEDURE `app`.`archive_user`(IN uid INT)
    UPDATE `app`.`users` SET `name` = CONCAT(`name`, ' (archived)') WHERE `id` = uid;

CREATE PROCEDURE `app`.`touch_post`(IN pid INT)
    UPDATE `app`.`posts` SET `title` = `title` WHERE `id` = pid;

-- analytics database

DROP DATABASE IF EXISTS `analytics`;
CREATE DATABASE `analytics`;

CREATE TABLE `analytics`.`events` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `name` VARCHAR(100) NOT NULL,
    `payload` TEXT,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB;

INSERT INTO `analytics`.`events` (`name`, `payload`) VALUES
    ('signup', '{"user": "alice"}'),
    ('login', '{"user": "bob"}');

-- canary database (used by drop_database tests)

DROP DATABASE IF EXISTS `canary`;
CREATE DATABASE `canary`;
