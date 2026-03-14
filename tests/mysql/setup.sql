-- Seed schema and data for MySQL/MariaDB
-- Executed via: docker exec -i <container> mariadb -u root -ptestroot mcp < seed_mysql.sql

CREATE TABLE IF NOT EXISTS `users` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `name` VARCHAR(100) NOT NULL,
    `email` VARCHAR(255) NOT NULL UNIQUE,
    `created_at` TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB;

CREATE TABLE IF NOT EXISTS `posts` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `user_id` INT NOT NULL,
    `title` VARCHAR(255) NOT NULL,
    `body` TEXT,
    `published` TINYINT(1) DEFAULT 0,
    CONSTRAINT `fk_posts_user` FOREIGN KEY (`user_id`) REFERENCES `users`(`id`)
) ENGINE=InnoDB;

CREATE TABLE IF NOT EXISTS `tags` (
    `id` INT AUTO_INCREMENT PRIMARY KEY,
    `name` VARCHAR(50) NOT NULL UNIQUE
) ENGINE=InnoDB;

CREATE TABLE IF NOT EXISTS `post_tags` (
    `post_id` INT NOT NULL,
    `tag_id` INT NOT NULL,
    PRIMARY KEY (`post_id`, `tag_id`),
    CONSTRAINT `fk_post_tags_post` FOREIGN KEY (`post_id`) REFERENCES `posts`(`id`),
    CONSTRAINT `fk_post_tags_tag` FOREIGN KEY (`tag_id`) REFERENCES `tags`(`id`)
) ENGINE=InnoDB;

-- Sample data: 3 users
INSERT INTO `users` (`name`, `email`) VALUES
    ('Alice Johnson', 'alice@example.com'),
    ('Bob Smith', 'bob@example.com'),
    ('Charlie Brown', 'charlie@example.com');

-- Sample data: 5 posts
INSERT INTO `posts` (`user_id`, `title`, `body`, `published`) VALUES
    (1, 'Getting Started with SQL', 'An introduction to SQL databases.', 1),
    (1, 'Advanced Queries', 'Deep dive into complex SQL queries.', 1),
    (2, 'Database Design', 'Best practices for schema design.', 1),
    (2, 'Draft Post', 'This is still a work in progress.', 0),
    (3, 'My First Post', 'Hello world from Charlie!', 0);

-- Sample data: 4 tags
INSERT INTO `tags` (`name`) VALUES
    ('sql'),
    ('tutorial'),
    ('design'),
    ('beginner');

-- Sample data: 6 post_tag associations
INSERT INTO `post_tags` (`post_id`, `tag_id`) VALUES
    (1, 1),
    (1, 2),
    (1, 4),
    (2, 1),
    (3, 3),
    (3, 1);
