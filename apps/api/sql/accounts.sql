-- Durable identity data. Never delete or rebuild this table with article indexes.
-- UTC timestamps use RFC 3339 strings; NULL means the user has never signed in.
-- permission_level: 0=BANNED, 1=MEMBER, 2=AUTHOR, 3=MASTER.
-- Replace INT with a SQL ENUM in a later, explicit data migration.
CREATE TABLE blog_users (
    id INT AUTO_INCREMENT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_login_at TEXT,
    permission_level INT NOT NULL
);

-- Records the one-time import of legacy JSON accounts, including an empty import.
CREATE TABLE blog_account_migrations (
    name TEXT PRIMARY KEY,
    completed_at TEXT NOT NULL
);
