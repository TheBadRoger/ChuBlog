-- Rebuildable article indexes. Sizes follow the validated Git manifest limits.
CREATE TYPE blog_article_status AS ENUM ('draft', 'published');
CREATE TYPE blog_document_format AS ENUM ('markdown', 'text');

CREATE TABLE blog_posts (
    slug VARCHAR(73) PRIMARY KEY,
    title VARCHAR(240) NOT NULL,
    description VARCHAR(1000) NOT NULL,
    published_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    path VARCHAR(192) NOT NULL,
    git_commit CHAR(40) NOT NULL
);
CREATE TABLE blog_tags (
    slug VARCHAR(73) NOT NULL,
    tag VARCHAR(80) NOT NULL,
    git_commit CHAR(40) NOT NULL
);
CREATE TABLE blog_articles (
    id VARCHAR(73) PRIMARY KEY,
    author VARCHAR(32) NOT NULL,
    article_id VARCHAR(40) NOT NULL,
    title VARCHAR(240) NOT NULL,
    description VARCHAR(1000) NOT NULL,
    status blog_article_status NOT NULL,
    created_at TIMESTAMP NOT NULL,
    published_at TIMESTAMP,
    updated_at TIMESTAMP NOT NULL,
    path VARCHAR(192) NOT NULL,
    format blog_document_format NOT NULL,
    cover VARCHAR(100),
    entry VARCHAR(100) NOT NULL,
    git_commit CHAR(40) NOT NULL
);
CREATE TABLE blog_pages (
    id VARCHAR(73) NOT NULL,
    name VARCHAR(100) NOT NULL,
    path VARCHAR(192) NOT NULL,
    git_commit CHAR(40) NOT NULL
);
CREATE TABLE blog_article_tags (
    id VARCHAR(73) NOT NULL,
    tag VARCHAR(80) NOT NULL
);
