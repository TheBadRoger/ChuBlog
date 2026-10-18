-- Durable identity and installation data; never rebuilt with article indexes.
-- TIMESTAMP values are UTC YYYY-MM-DD HH:MM:SS. The API emits UTC RFC 3339.
CREATE TYPE blog_permission AS ENUM ('BANNED', 'MEMBER', 'AUTHOR', 'MASTER');

CREATE TABLE blog_users (
    id INT AUTO_INCREMENT PRIMARY KEY,
    username VARCHAR(32) NOT NULL UNIQUE,
    nickname VARCHAR(128) NOT NULL,
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

CREATE TYPE blog_visibility AS ENUM ('DEFAULT', 'SECRET', 'CONFIDENTIAL', 'TOP');
CREATE TYPE blog_hidden AS ENUM ('SEMI', 'FULL');
CREATE TYPE blog_review AS ENUM ('OPEN', 'BLOCKED');

-- Queryable projection of collection.json article policy. collection.json stays authoritative.
CREATE TABLE blog_article_policy (
    article VARCHAR(113) PRIMARY KEY,
    visibility blog_visibility NOT NULL DEFAULT 'DEFAULT',
    hidden blog_hidden NOT NULL DEFAULT 'FULL',
    review blog_review NOT NULL DEFAULT 'OPEN',
    blocked_hash CHAR(64),
    password_hash VARCHAR(255)
);

CREATE TYPE blog_access_source AS ENUM ('PASSWORD', 'GRANTED');

-- Durable reader grants: password unlocks and administrator/author TOP grants.
CREATE TABLE blog_article_access (
    id VARCHAR(134) PRIMARY KEY,
    user_id INT NOT NULL,
    article VARCHAR(113) NOT NULL,
    source blog_access_source NOT NULL,
    created_at TIMESTAMP NOT NULL
);
