use crate::content::{Snapshot, Status};
use crate::database::to_timestamp;
use anyhow::{Context, Result, ensure};
use chusql_driver::{Client, Param, Query};

async fn connect() -> Result<Client> {
    crate::database::Database::from_env()?.connect().await
}

pub async fn rebuild(snapshot: &Snapshot) -> Result<()> {
    rebuild_with_db(snapshot, connect().await?).await
}

pub(crate) async fn rebuild_with_db(snapshot: &Snapshot, db: Client) -> Result<()> {
    crate::database::ensure_schema(&db, crate::database::ARTICLES_SQL).await?;
    db.query("BEGIN").await?;
    db.query("DELETE FROM blog_pages").await?;
    db.query("DELETE FROM blog_article_tags").await?;
    db.query("DELETE FROM blog_articles").await?;
    for post in snapshot.posts.values() {
        let meta = &post.meta;
        let created = to_timestamp(&meta.created_at)?;
        let updated = to_timestamp(&meta.updated)?;
        let published = if meta.date.is_empty() {
            None
        } else {
            Some(to_timestamp(&meta.date)?)
        };
        let status = match meta.status {
            Status::Draft => "draft",
            Status::Published => "published",
        };
        let q = Query::new("INSERT INTO blog_articles (id, author, article_id, title, description, status, created_at, published_at, updated_at, path, format, cover, entry, git_commit) VALUES (")
            .bind(meta.slug.as_str()).push(", ").bind(post.author.as_str()).push(", ")
            .bind(post.article_id.as_str()).push(", ").bind(meta.title.as_str()).push(", ")
            .bind(meta.description.as_str()).push(", ").bind(status).push(", ")
            .bind(created.as_str()).push(", ").bind(published.as_deref().map(Param::Text).unwrap_or(Param::Null)).push(", ").bind(updated.as_str()).push(", ")
            .bind(post.path.as_str()).push(", ").bind(post.format.as_str()).push(", ")
            .bind(meta.cover.as_deref().map(Param::Text).unwrap_or(Param::Null)).push(", ").bind(post.entry.as_str()).push(", ")
            .bind(post.git_commit.as_str()).push(")");
        db.query(q.as_str()).await?;
        let folder = post.path.rsplit_once('/').context("正文路径无效")?.0;
        for name in post.pages.keys() {
            let q = Query::new("INSERT INTO blog_pages (id, name, path, git_commit) VALUES (")
                .bind(meta.slug.as_str())
                .push(", ")
                .bind(name.as_str())
                .push(", ")
                .bind(format!("{folder}/{name}").as_str())
                .push(", ")
                .bind(post.git_commit.as_str())
                .push(")");
            db.query(q.as_str()).await?;
        }
        for tag in &meta.tags {
            let q = Query::new("INSERT INTO blog_article_tags (id, tag) VALUES (")
                .bind(meta.slug.as_str())
                .push(", ")
                .bind(tag.as_str())
                .push(")");
            db.query(q.as_str()).await?;
        }
    }
    db.query("DELETE FROM blog_tags").await?;
    db.query("DELETE FROM blog_posts").await?;
    for post in snapshot
        .posts
        .values()
        .filter(|p| p.meta.status == Status::Published)
    {
        let meta = &post.meta;
        let published = to_timestamp(&meta.date)?;
        let updated = to_timestamp(&meta.updated)?;
        let q = Query::new("INSERT INTO blog_posts (slug, title, description, published_at, updated_at, path, git_commit) VALUES (")
            .bind(meta.slug.as_str()).push(", ").bind(meta.title.as_str()).push(", ")
            .bind(meta.description.as_str()).push(", ").bind(published.as_str()).push(", ")
            .bind(updated.as_str()).push(", ").bind(post.path.as_str())
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
        crate::database::execute_script(&db, crate::database::CREATE_DATABASE_SQL).await?;
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
    let db = connect().await?;
    crate::database::ensure_schema(&db, crate::database::ACCOUNTS_SQL).await?;
    crate::database::ensure_schema(&db, crate::database::ARTICLES_SQL).await?;
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
    let page_count = db.query("SELECT count(*) FROM blog_pages").await?;
    let all_tag_count = db.query("SELECT count(*) FROM blog_article_tags").await?;
    let expected_pages = snapshot
        .posts
        .values()
        .map(|p| p.pages.len())
        .sum::<usize>() as u64;
    let expected_all_tags = snapshot
        .posts
        .values()
        .map(|p| p.meta.tags.len())
        .sum::<usize>() as u64;
    let count_value = |result: &chusql_driver::QueryResult| {
        result
            .rows
            .first()
            .and_then(|r| r.first())
            .and_then(|v| v.as_u64())
    };
    let matches = count_value(&page_count) == Some(expected_pages)
        && count_value(&all_tag_count) == Some(expected_all_tags)
        && !articles.truncated
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
