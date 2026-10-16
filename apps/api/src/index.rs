use crate::content::{Snapshot, Status};
use anyhow::{Context, Result, ensure};
use chusql_driver::{Client, Options, Query};
use std::time::Duration;

async fn connect() -> Result<Client> {
    let database = std::env::var("CHUSQL_DATABASE").context("未配置 ChuSQL")?;
    ensure!(
        !database.is_empty()
            && !database.starts_with(|c: char| c.is_ascii_digit())
            && database
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "数据库名称无效"
    );
    let db = Client::connect_with_options(
        std::env::var("CHUSQL_ADDRESS").unwrap_or_else(|_| "127.0.0.1:7777".into()),
        &std::env::var("CHUSQL_USER")?,
        &std::env::var("CHUSQL_PASSWORD")?,
        Options {
            timeout: Duration::from_secs(3),
            max_response_bytes: 1024 * 1024,
        },
    )
    .await?;
    db.query(&format!("USE {database}")).await?;
    Ok(db)
}

pub async fn rebuild(snapshot: &Snapshot) -> Result<()> {
    let db = connect().await?;
    // Probe before creating: ChuSQL does not promise IF NOT EXISTS syntax.
    if db
        .query("SELECT slug FROM blog_posts LIMIT 1")
        .await
        .is_err()
    {
        db.query("CREATE TABLE blog_posts (slug TEXT PRIMARY KEY, title TEXT, description TEXT, published_at TEXT, updated_at TEXT, path TEXT, git_commit TEXT)").await?;
    }
    if db
        .query("SELECT slug FROM blog_tags LIMIT 1")
        .await
        .is_err()
    {
        db.query("CREATE TABLE blog_tags (slug TEXT, tag TEXT, git_commit TEXT)")
            .await?;
    }
    if db
        .query("SELECT id FROM blog_articles LIMIT 1")
        .await
        .is_err()
    {
        db.query("CREATE TABLE blog_articles (id TEXT PRIMARY KEY, author TEXT, article_id TEXT, title TEXT, description TEXT, status TEXT, published_at TEXT, updated_at TEXT, path TEXT, format TEXT, git_commit TEXT)").await?;
    }
    db.query("BEGIN").await?;
    db.query("DELETE FROM blog_articles").await?;
    for post in snapshot.posts.values() {
        let meta = &post.meta;
        let status = match meta.status {
            Status::Draft => "draft",
            Status::Published => "published",
        };
        let q = Query::new("INSERT INTO blog_articles (id, author, article_id, title, description, status, published_at, updated_at, path, format, git_commit) VALUES (")
            .bind(meta.slug.as_str()).push(", ").bind(post.author.as_str()).push(", ")
            .bind(post.article_id.as_str()).push(", ").bind(meta.title.as_str()).push(", ")
            .bind(meta.description.as_str()).push(", ").bind(status).push(", ")
            .bind(meta.date.as_str()).push(", ").bind(meta.updated.as_str()).push(", ")
            .bind(post.path.as_str()).push(", ").bind(post.format.as_str()).push(", ")
            .bind(post.git_commit.as_str()).push(")");
        db.query(q.as_str()).await?;
    }
    db.query("DELETE FROM blog_tags").await?;
    db.query("DELETE FROM blog_posts").await?;
    for post in snapshot
        .posts
        .values()
        .filter(|p| p.meta.status == Status::Published)
    {
        let meta = &post.meta;
        let q = Query::new("INSERT INTO blog_posts (slug, title, description, published_at, updated_at, path, git_commit) VALUES (")
            .bind(meta.slug.as_str()).push(", ").bind(meta.title.as_str()).push(", ")
            .bind(meta.description.as_str()).push(", ").bind(meta.date.as_str()).push(", ")
            .bind(meta.updated.as_str()).push(", ").bind(post.path.as_str())
            .push(", ").bind(snapshot.commit.as_str()).push(")");
        db.query(q.as_str()).await?;
        for tag in &meta.tags {
            let q = Query::new("INSERT INTO blog_tags (slug, tag, git_commit) VALUES (")
                .bind(meta.slug.as_str())
                .push(", ")
                .bind(tag.as_str())
                .push(", ")
                .bind(snapshot.commit.as_str())
                .push(")");
            db.query(q.as_str()).await?;
        }
    }
    db.query("COMMIT").await?;
    db.close().await;
    Ok(())
}

pub async fn provision() -> Result<()> {
    let db = Client::connect(
        std::env::var("CHUSQL_ADDRESS").unwrap_or_else(|_| "127.0.0.1:7777".into()),
        "root",
        &std::env::var("CHUSQL_ROOT_PASSWORD")?,
    )
    .await?;
    if db.query("USE chublog").await.is_err() {
        db.query("CREATE DATABASE chublog").await?;
    }
    let roles = db.query("SHOW ROLES").await?;
    ensure!(!roles.truncated, "身份列表被截断，请人工检查数据库初始化");
    if !roles
        .rows
        .iter()
        .flatten()
        .any(|v| v.as_str() == Some("chublog_index"))
    {
        let password = std::env::var("CHUSQL_PASSWORD")?;
        ensure!(password.len() >= 16, "数据库口令至少 16 字节");
        let q = Query::new("CREATE USER chublog_index IDENTIFIED BY ").bind(password.as_str());
        db.query(q.as_str())
            .await
            .map_err(|_| anyhow::anyhow!("创建索引用户失败"))?;
    }
    db.query("REVOKE CONNECT ON DATABASE chublog FROM PUBLIC")
        .await?;
    db.query("GRANT CONNECT, CREATE ON DATABASE chublog TO chublog_index")
        .await?;
    db.close().await;
    Ok(())
}

// Detect a lost/recreated index even if no new article was pushed.
pub async fn consistent(snapshot: &Snapshot) -> Result<bool> {
    let db = connect().await?;
    let result = db
        .query("SELECT slug, git_commit FROM blog_posts LIMIT 1000")
        .await?;
    let expected: std::collections::BTreeSet<_> = snapshot
        .posts
        .values()
        .filter(|p| p.meta.status == Status::Published)
        .map(|p| (p.meta.slug.as_str(), snapshot.commit.as_str()))
        .collect();
    let actual: std::collections::BTreeSet<_> = result
        .rows
        .iter()
        .filter_map(|row| Some((row.first()?.as_str()?, row.get(1)?.as_str()?)))
        .collect();
    let count = db.query("SELECT count(*) FROM blog_tags").await?;
    let tags = snapshot
        .posts
        .values()
        .filter(|p| p.meta.status == Status::Published)
        .map(|p| p.meta.tags.len())
        .sum::<usize>() as u64;
    let articles = db
        .query("SELECT id, git_commit FROM blog_articles LIMIT 1000")
        .await?;
    let expected_articles: std::collections::BTreeSet<_> = snapshot
        .posts
        .values()
        .map(|p| (p.meta.slug.as_str(), p.git_commit.as_str()))
        .collect();
    let actual_articles: std::collections::BTreeSet<_> = articles
        .rows
        .iter()
        .filter_map(|row| Some((row.first()?.as_str()?, row.get(1)?.as_str()?)))
        .collect();
    let matches = !articles.truncated
        && articles.rows.len() == expected_articles.len()
        && actual_articles == expected_articles
        && !result.truncated
        && result.rows.len() == expected.len()
        && actual == expected
        && count
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(|v| v.as_u64())
            == Some(tags);
    db.close().await;
    Ok(matches)
}
