-- Seed schema and data for MySQL/MariaDB
-- This file is mounted to /docker-entrypoint-initdb.d/ and auto-executed on first start

-- Allow non-deterministic stored functions to be created without each function
-- explicitly declaring DETERMINISTIC / NO SQL / READS SQL DATA. The seed
-- intentionally exercises a NOT DETERMINISTIC + MODIFIES SQL DATA function
-- (`recalc_order_total_v2`) to cover spec FR-004 acceptance values.
SET GLOBAL log_bin_trust_function_creators = 1;

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

-- Additional views — exercise FR-001 search semantics and FR-004 detailed-mode
-- field coverage. Five `*active*` views in total (counting the existing
-- `active_users` above) so search="active" returns exactly five hits.
-- Bodies use only standard SQL constructs that work on both MySQL 9 and
-- MariaDB 12. No `ALGORITHM=` clause (MariaDB-only column not surfaced
-- per FR-006).

-- Non-updatable due to expression column (UPPER) — exercises updatable=false.
CREATE VIEW `app`.`active_users_v2` AS
    SELECT `id`, UPPER(`name`) AS `name` FROM `app`.`users`;

-- Non-updatable due to GROUP BY + aggregate — exercises updatable=false
-- (Acceptance Scenario 4). MySQL's `IS_UPDATABLE` reports `YES` for many
-- simple JOINs (any single underlying table is still updatable through
-- the view), but a `GROUP BY` aggregate view is unconditionally
-- non-updatable on both MySQL 9 and MariaDB 12.
CREATE VIEW `app`.`active_orders` AS
    SELECT `p`.`user_id`, COUNT(*) AS `post_count`
    FROM `app`.`posts` `p`
    WHERE `p`.`published` = 1
    GROUP BY `p`.`user_id`;

-- Updatable view with `WITH CASCADED CHECK OPTION` — exercises checkOption="CASCADED".
CREATE VIEW `app`.`active_users_with_check_cascaded` AS
    SELECT `id`, `name`, `email` FROM `app`.`users` WHERE `id` > 0
    WITH CASCADED CHECK OPTION;

-- Updatable view with `WITH LOCAL CHECK OPTION` — exercises checkOption="LOCAL".
CREATE VIEW `app`.`active_users_with_check_local` AS
    SELECT `id`, `name`, `email` FROM `app`.`users` WHERE `id` > 0
    WITH LOCAL CHECK OPTION;

-- SQL SECURITY DEFINER view — exercises security="DEFINER".
CREATE SQL SECURITY DEFINER VIEW `app`.`archived_users` AS
    SELECT `id`, `name`, `email` FROM `app`.`users`;

-- SQL SECURITY INVOKER view — exercises security="INVOKER" (the engine default
-- is DEFINER, so this requires an explicit clause).
CREATE SQL SECURITY INVOKER VIEW `app`.`archived_users_invoker` AS
    SELECT `id`, `name`, `email` FROM `app`.`users`;

-- Multi-line / CTE-style body — exercises the spec Edge Case "view definitions
-- can be very large; faithful pass-through of VIEW_DEFINITION is required".
-- MySQL/MariaDB normalise CTE syntax to derived-table form in VIEW_DEFINITION,
-- so the round-trip assertion checks for the engine-canonicalised body, not
-- the literal source text.
CREATE VIEW `app`.`user_metrics_cte` AS
    SELECT `u`.`id`,
           `u`.`name`,
           (SELECT COUNT(*) FROM `app`.`posts` `p`
            WHERE `p`.`user_id` = `u`.`id`) AS `post_count`
    FROM `app`.`users` `u`;

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

-- *order* search-target functions — exercise FR-001 search semantics + FR-004
-- detailed-mode `sqlDataAccess` / `deterministic` / `security` / `description`
-- coverage. Single-statement bodies (no DELIMITER required).

CREATE FUNCTION `app`.`calc_order_total`(order_id INT) RETURNS DECIMAL(12,2)
    DETERMINISTIC
    READS SQL DATA
    SQL SECURITY INVOKER
    COMMENT 'Sums line items for an order'
    RETURN (SELECT COALESCE(MAX(`id`), 0) FROM `app`.`posts` WHERE `id` = order_id);

CREATE FUNCTION `app`.`calc_order_subtotal`(order_id INT, exclude_title VARCHAR(64)) RETURNS DECIMAL(12,2)
    DETERMINISTIC
    READS SQL DATA
    SQL SECURITY INVOKER
    COMMENT 'Sums line items for an order, excluding one title'
    RETURN (SELECT COALESCE(MAX(`id`), 0) FROM `app`.`posts` WHERE `id` = order_id AND `title` <> exclude_title);

CREATE FUNCTION `app`.`recalc_order_total_v2`(order_id INT) RETURNS DECIMAL(12,2)
    NOT DETERMINISTIC
    MODIFIES SQL DATA
    SQL SECURITY DEFINER
    COMMENT 'Recomputes and writes back the orders.total cache (v2)'
    RETURN order_id * 2;

-- Zero-parameter function with NO_SQL access flavour, no COMMENT (exercises
-- the `description === null` coercion in FR-004 acceptance #6).
CREATE FUNCTION `app`.`current_pricing_version`() RETURNS INT
    DETERMINISTIC
    NO SQL
    SQL SECURITY INVOKER
    RETURN 7;

-- Body containing literal newline + escaped single quote — exercises spec
-- Edge Case "function body containing literal newlines, quote characters".
CREATE FUNCTION `app`.`format_audit_note`(order_id INT) RETURNS VARCHAR(512)
    DETERMINISTIC
    NO SQL
    SQL SECURITY INVOKER
    COMMENT 'Returns a multi-line note for the audit log'
    RETURN CONCAT('order ', order_id, '\nnote: contains a quote '' and a newline');

CREATE PROCEDURE `app`.`archive_user`(IN uid INT)
    UPDATE `app`.`users` SET `name` = CONCAT(`name`, ' (archived)') WHERE `id` = uid;

CREATE PROCEDURE `app`.`touch_post`(IN pid INT)
    UPDATE `app`.`posts` SET `title` = `title` WHERE `id` = pid;

-- *archive* / *order* search-target procedures — exercise FR-001 search
-- semantics and FR-004 detailed-mode coverage (parameter modes, deterministic
-- flag, sqlDataAccess classification, security mode, comment handling).
-- Single-statement bodies (no DELIMITER required).

CREATE PROCEDURE `app`.`archive_order_history`(IN order_id INT)
    DETERMINISTIC
    CONTAINS SQL
    SQL SECURITY INVOKER
    COMMENT 'Archives an order history row'
    UPDATE `app`.`posts` SET `title` = `title` WHERE `id` = order_id;

CREATE PROCEDURE `app`.`archive_order`(IN n INT, OUT archived_count INT)
    DETERMINISTIC
    MODIFIES SQL DATA
    SQL SECURITY INVOKER
    COMMENT 'Archives an order and returns the count'
    SET archived_count = n * 2;

CREATE PROCEDURE `app`.`purge_order_archive`(INOUT counter INT)
    NOT DETERMINISTIC
    READS SQL DATA
    SQL SECURITY INVOKER
    SET counter = (SELECT COALESCE(MAX(`id`), 0) FROM `app`.`posts`);

CREATE PROCEDURE `app`.`compute_user_metrics`(
    IN uid INT,
    OUT metric_total DECIMAL(10,2) UNSIGNED,
    INOUT metric_avg DECIMAL(10,2)
)
    DETERMINISTIC
    NO SQL
    SQL SECURITY DEFINER
    COMMENT 'Multi-mode parameter fixture'
    SET metric_total = uid * 1.5;

-- Zero-parameter procedure with body containing a literal newline + escaped
-- single quote — exercises spec Edge Case "procedure body containing literal
-- newlines, quote characters".
CREATE PROCEDURE `app`.`no_arg_proc`()
    DETERMINISTIC
    NO SQL
    SQL SECURITY INVOKER
    COMMENT 'Zero-parameter round-trip fixture'
    SELECT 'first line
second line with quote ''here''' AS note;

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
