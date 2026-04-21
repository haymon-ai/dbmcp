-- Seed schema and data for MySQL/MariaDB
-- This file is mounted to /docker-entrypoint-initdb.d/ and auto-executed on first start

-- app database

DROP DATABASE IF EXISTS `app`;
CREATE DATABASE `app`;

CREATE TABLE `app`.`users` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `name` VARCHAR(100) NOT NULL,
    `email` VARCHAR(255) NOT NULL UNIQUE,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB;

CREATE TABLE `app`.`posts` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `user_id` INT NOT NULL,
    `title` VARCHAR(255) NOT NULL,
    `body` TEXT,
    `published` TINYINT(1) DEFAULT 0,
    CONSTRAINT `fk_posts_user` FOREIGN KEY (`user_id`) REFERENCES `app`.`users`(`id`)
) ENGINE=InnoDB;

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
