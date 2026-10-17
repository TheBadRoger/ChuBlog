-- Durable identity and installation data; never rebuilt with article indexes.
-- TIMESTAMP values are UTC YYYY-MM-DD HH:MM:SS. The API emits UTC RFC 3339.
CREATE TYPE blog_permission AS ENUM ('BANNED', 'MEMBER', 'AUTHOR', 'MASTER');

CREATE TABLE blog_users (
    id INT AUTO_INCREMENT PRIMARY KEY,
    username VARCHAR(32) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    created_at TIMESTAMP NOT NULL,
    last_login_at TIMESTAMP,
    permission_level blog_permission NOT NULL DEFAULT 'MEMBER'
);

CREATE TABLE blog_site (
    id INT PRIMARY KEY,
    name VARCHAR(120) NOT NULL,
    description VARCHAR(1000) NOT NULL,
    master_user_id INT NOT NULL UNIQUE,
    installed_at TIMESTAMP NOT NULL,
    starter_commit CHAR(40)
);
