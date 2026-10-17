-- Rebuildable article indexes. Identity tables belong in accounts.sql.
CREATE TABLE blog_posts (slug TEXT PRIMARY KEY, title TEXT, description TEXT, published_at TEXT, updated_at TEXT, path TEXT, git_commit TEXT);
CREATE TABLE blog_tags (slug TEXT, tag TEXT, git_commit TEXT);
CREATE TABLE blog_articles (id TEXT PRIMARY KEY, author TEXT, article_id TEXT, title TEXT, description TEXT, status TEXT, created_at TEXT, published_at TEXT, updated_at TEXT, path TEXT, format TEXT, cover TEXT, entry TEXT, git_commit TEXT);
CREATE TABLE blog_pages (id TEXT, name TEXT, path TEXT, git_commit TEXT);
CREATE TABLE blog_article_tags (id TEXT, tag TEXT);
