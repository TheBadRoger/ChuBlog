use crate::{
    accounts::{Grant, Role},
    database::Database,
};
use anyhow::Context;
use chusql_driver::{Client, Query, QueryResult, Value};
use std::future::Future;

#[derive(Debug)]
pub enum Error {
    Database(anyhow::Error),
    Conflict,
    NotFound,
    Denied,
    Quota,
    InstallationConflict,
}

impl From<anyhow::Error> for Error {
    fn from(error: anyhow::Error) -> Self {
        Self::Database(error)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Database(_) => "账户数据库操作失败",
            Self::Conflict => "账号已存在或凭据已变更",
            Self::NotFound => "用户不存在",
            Self::Denied => "权限不足或已变更",
            Self::Quota => "用户配额已满",
            Self::InstallationConflict => "账户表已有记录，请核对安装状态或使用新的安装数据库",
        })
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}
type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Banned,
    Member,
    Author,
    Master,
}
impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Banned => "BANNED",
            Self::Member => "MEMBER",
            Self::Author => "AUTHOR",
            Self::Master => "MASTER",
        }
    }
    pub fn from_grant(enabled: bool, role: Role) -> Self {
        if !enabled {
            Self::Banned
        } else {
            match role {
                Role::Reader => Self::Member,
                Role::Publisher => Self::Author,
                Role::Admin => Self::Master,
            }
        }
    }
    pub fn grant(self) -> Grant {
        Grant {
            enabled: self != Self::Banned,
            role: match self {
                Self::Banned | Self::Member => Role::Reader,
                Self::Author => Role::Publisher,
                Self::Master => Role::Admin,
            },
        }
    }
}
#[derive(Clone)]
pub struct AuthorDisplay {
    pub nickname: String,
    pub is_admin: bool,
}

#[derive(Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub nickname: String,
    pub hash: String,
    pub created_at: String,
    pub last_login_at: Option<String>,
    pub level: Level,
}
impl User {
    pub fn same_revision(&self, other: &Self) -> bool {
        self.id == other.id
            && self.username == other.username
            && self.hash == other.hash
            && self.level == other.level
    }
    pub fn view(&self) -> Value {
        serde_json::json!({"id":self.id,"username":self.username,"nickname":self.nickname,"created_at":self.created_at,
            "last_login_at":self.last_login_at,"permission_level":self.level.as_str()})
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessSource {
    Password,
    Granted,
}
impl AccessSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "PASSWORD",
            Self::Granted => "GRANTED",
        }
    }
    fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "PASSWORD" => Ok(Self::Password),
            "GRANTED" => Ok(Self::Granted),
            _ => anyhow::bail!("未知文章授权来源"),
        }
    }
}

#[derive(Clone)]
pub struct ArticleAccess {
    pub user: User,
    pub source: AccessSource,
}

/// One row of the queryable article-policy projection. collection.json remains authoritative.
pub struct ArticlePolicyRow {
    pub article: String,
    pub visibility: String,
    pub hidden: String,
    pub review: String,
    pub blocked_hash: Option<String>,
    pub password_hash: Option<String>,
}

#[derive(Clone)]
pub struct Store {
    pub database: Database,
}
const COLUMNS: &str =
    "id, username, password_hash, created_at, last_login_at, permission_level, nickname";
fn require(condition: bool, message: &'static str) -> anyhow::Result<()> {
    if !condition {
        anyhow::bail!(message);
    }
    Ok(())
}
fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
async fn sql(db: &Client, query: &str) -> Result<QueryResult> {
    let operation = match query {
        q if q.starts_with("INSERT INTO blog_users ") => "create_account",
        q if q.starts_with("INSERT INTO blog_site ") => "record_installation",
        q if q.starts_with("SELECT count(*) FROM blog_users") => "count_accounts",
        "BEGIN" => "begin_transaction",
        "COMMIT" => "commit_transaction",
        "ROLLBACK" => "rollback_transaction",
        q if q.starts_with("SELECT ") => "read_accounts_or_site",
        q if q.starts_with("UPDATE ") => "update_account_or_site",
        _ => "account_query",
    };
    db.query(query)
        .await
        .map_err(|error| Error::Database(anyhow::Error::new(error).context(operation)))
}
fn decode(row: &[Value]) -> anyhow::Result<User> {
    require(row.len() == 7, "账户记录字段数量无效")?;
    let id = row[0]
        .as_i64()
        .filter(|id| *id > 0)
        .context("用户 ID 无效")?;
    let text = |index: usize| {
        row[index]
            .as_str()
            .map(str::to_owned)
            .context("账户文本字段无效")
    };
    let username = text(1)?;
    crate::accounts::username(&username)?;
    let nickname = text(6)?;
    crate::accounts::nickname(&nickname)?;
    let hash = text(2)?;
    let parsed = argon2::PasswordHash::new(&hash).map_err(|_| anyhow::anyhow!("密码哈希无效"))?;
    require(
        parsed.algorithm.as_str() == "argon2id",
        "密码必须使用 Argon2id 哈希",
    )?;
    let created_at = crate::database::from_timestamp(&text(3)?)?;
    let last_login_at = if row[4].is_null() {
        None
    } else {
        Some(crate::database::from_timestamp(&text(4)?)?)
    };
    let level = match row[5].as_str() {
        Some("BANNED") => Level::Banned,
        Some("MEMBER") => Level::Member,
        Some("AUTHOR") => Level::Author,
        Some("MASTER") => Level::Master,
        _ => anyhow::bail!("未知账户权限等级，拒绝授权"),
    };
    Ok(User {
        id,
        username,
        nickname,
        hash,
        created_at,
        last_login_at,
        level,
    })
}
async fn get(db: &Client, name: &str) -> Result<Option<User>> {
    let query = Query::new(&format!(
        "SELECT {COLUMNS} FROM blog_users WHERE username = "
    ))
    .bind(name)
    .push(" LIMIT 2");
    let result = sql(db, query.as_str()).await?;
    require(
        !result.truncated && result.rows.len() <= 1,
        "账户查询结果不唯一或被截断",
    )?;
    result
        .rows
        .first()
        .map(|row| decode(row).map_err(Error::from))
        .transpose()
}
impl Store {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database: Database::from_env()?,
        })
    }
    async fn transaction<T, F, Fut>(&self, action: F) -> Result<T>
    where
        F: FnOnce(Client) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let db = self.database.connect().await?;
        sql(&db, "BEGIN").await?;
        match action(db.clone()).await {
            Ok(value) => {
                sql(&db, "COMMIT").await?;
                db.close().await;
                Ok(value)
            }
            Err(error) => {
                let _ = db.query("ROLLBACK").await;
                db.close().await;
                Err(error)
            }
        }
    }
    pub async fn user(&self, name: &str) -> Result<Option<User>> {
        let db = self.database.connect().await?;
        let result = get(&db, name).await;
        db.close().await;
        result
    }
    pub async fn users(&self) -> Result<Vec<User>> {
        let db = self.database.connect().await?;
        let result = sql(&db, &format!("SELECT {COLUMNS} FROM blog_users LIMIT 1001")).await?;
        db.close().await;
        require(
            !result.truncated && result.rows.len() <= 1000,
            "账户列表超过配额或被截断",
        )?;
        result
            .rows
            .iter()
            .map(|row| decode(row).map_err(Error::from))
            .collect()
    }
    pub async fn display_authors(
        &self,
    ) -> Result<std::collections::BTreeMap<String, AuthorDisplay>> {
        let db = self.database.connect().await?;
        let result = sql(
            &db,
            "SELECT username, nickname, permission_level FROM blog_users LIMIT 1001",
        )
        .await?;
        db.close().await;
        require(
            !result.truncated && result.rows.len() <= 1000,
            "账户列表超过配额或被截断",
        )?;
        result
            .rows
            .iter()
            .map(|row| {
                let name = row
                    .first()
                    .and_then(Value::as_str)
                    .context("账户名称无效")?;
                let nickname = row.get(1).and_then(Value::as_str).context("账户昵称无效")?;
                let is_admin = match row.get(2).and_then(Value::as_str) {
                    Some("MASTER") => true,
                    Some("BANNED" | "MEMBER" | "AUTHOR") => false,
                    _ => return Err(Error::Database(anyhow::anyhow!("未知账户权限等级"))),
                };
                Ok((
                    name.to_owned(),
                    AuthorDisplay {
                        nickname: nickname.to_owned(),
                        is_admin,
                    },
                ))
            })
            .collect()
    }
    pub async fn site(&self) -> Result<Option<crate::setup::Site>> {
        let db = self.database.connect().await?;
        let result = sql(&db, "SELECT name, description FROM blog_site WHERE id = 1").await?;
        db.close().await;
        require(
            !result.truncated && result.rows.len() <= 1,
            "网站配置记录无效",
        )?;
        result
            .rows
            .first()
            .map(|row| {
                Ok(crate::setup::Site {
                    name: row
                        .first()
                        .and_then(Value::as_str)
                        .context("网站名称无效")?
                        .into(),
                    description: row
                        .get(1)
                        .and_then(Value::as_str)
                        .context("网站简介无效")?
                        .into(),
                })
            })
            .transpose()
    }
    pub async fn owner(&self) -> Result<Option<User>> {
        let db = self.database.connect().await?;
        let result = sql(&db, &format!("SELECT {COLUMNS} FROM blog_users WHERE id IN (SELECT master_user_id FROM blog_site WHERE id = 1)")).await?;
        db.close().await;
        require(
            !result.truncated && result.rows.len() <= 1,
            "管理员账户记录无效",
        )?;
        let user = result.rows.first().map(|row| decode(row)).transpose()?;
        if let Some(user) = &user {
            require(user.level == Level::Master, "管理员权限无效")?;
        }
        Ok(user)
    }
    pub async fn starters_pending(&self) -> Result<bool> {
        let db = self.database.connect().await?;
        let result = sql(&db, "SELECT starter_commit FROM blog_site WHERE id = 1").await?;
        db.close().await;
        Ok(result
            .rows
            .first()
            .is_some_and(|row| row.first().is_some_and(Value::is_null)))
    }
    pub async fn mark_starters(&self, owner: &User, commit: &str) -> Result<()> {
        let db = self.database.connect().await?;
        let query = Query::new("UPDATE blog_site SET starter_commit = ")
            .bind(commit)
            .push(" WHERE id = 1 AND master_user_id = ")
            .bind(owner.id)
            .push(" AND starter_commit IS NULL");
        sql(&db, query.as_str()).await?;
        db.close().await;
        Ok(())
    }
    pub async fn install(
        &self,
        site: &crate::setup::Site,
        name: &str,
        nickname: &str,
        hash: &str,
    ) -> Result<()> {
        self.transaction(|db| async move {
            let installed = sql(&db, "SELECT id FROM blog_site WHERE id = 1").await?;
            if !installed.rows.is_empty() { return Err(Error::Conflict); }
            let count = sql(&db, "SELECT count(*) FROM blog_users").await?;
            let count = count.rows.first().and_then(|row| row.first()).and_then(Value::as_u64).context("安装账户数量查询无效")?;
            if count != 0 { return Err(Error::InstallationConflict); }
            let query = Query::new("INSERT INTO blog_users (username, nickname, password_hash, created_at, permission_level) VALUES (")
                .bind(name).push(", ").bind(nickname).push(", ").bind(hash).push(", ").bind(now().as_str()).push(", 'MASTER')");
            sql(&db, query.as_str()).await?;
            let master = get(&db, name).await?.ok_or(Error::NotFound)?;
            let query = Query::new("INSERT INTO blog_site (id, name, description, master_user_id, installed_at) VALUES (1, ")
                .bind(site.name.as_str()).push(", ").bind(site.description.as_str()).push(", ").bind(master.id)
                .push(", ").bind(now().as_str()).push(")");
            sql(&db, query.as_str()).await?;
            Ok(())
        }).await
    }
    pub async fn register(&self, name: &str, nickname: &str, hash: &str) -> Result<User> {
        self.transaction(|db| async move {
            if sql(&db, "SELECT id FROM blog_site WHERE id = 1").await?.rows.is_empty() { return Err(Error::Denied); }
            if get(&db, name).await?.is_some() { return Err(Error::Conflict); }
            let count = sql(&db, "SELECT count(*) FROM blog_users").await?;
            let count = count.rows.first().and_then(|row| row.first()).and_then(Value::as_u64)
                .context("用户数量查询无效")?;
            if count >= 1000 { return Err(Error::Quota); }
            let query = Query::new("INSERT INTO blog_users (username, nickname, password_hash, created_at, last_login_at, permission_level) VALUES (")
                .bind(name).push(", ").bind(nickname).push(", ").bind(hash).push(", ").bind(now().as_str()).push(", NULL, ").bind(Level::Member.as_str()).push(")");
            sql(&db, query.as_str()).await?;
            get(&db, name).await?.ok_or(Error::NotFound)
        }).await
    }
    pub async fn logged_in(&self, expected: &User) -> Result<User> {
        self.transaction(|db| async move {
            let current = get(&db, &expected.username).await?.ok_or(Error::NotFound)?;
            if !current.same_revision(expected) || current.level == Level::Banned {
                return Err(Error::Conflict);
            }
            let query = Query::new("UPDATE blog_users SET last_login_at = ")
                .bind(now().as_str())
                .push(" WHERE id = ")
                .bind(current.id);
            sql(&db, query.as_str()).await?;
            get(&db, &current.username).await?.ok_or(Error::NotFound)
        })
        .await
    }
    pub async fn change_password(&self, expected: &User, hash: &str) -> Result<()> {
        self.transaction(|db| async move {
            let current = get(&db, &expected.username).await?.ok_or(Error::NotFound)?;
            if !current.same_revision(expected) || current.level == Level::Banned {
                return Err(Error::Conflict);
            }
            let query = Query::new("UPDATE blog_users SET password_hash = ")
                .bind(hash)
                .push(" WHERE id = ")
                .bind(current.id);
            sql(&db, query.as_str()).await?;
            Ok(())
        })
        .await
    }

    pub async fn article_access_set(
        &self,
        user_id: i64,
    ) -> Result<std::collections::BTreeSet<String>> {
        let db = self.database.connect().await?;
        let query = Query::new("SELECT article FROM blog_article_access WHERE user_id = ")
            .bind(user_id)
            .push(" AND source = ")
            .bind(AccessSource::Granted.as_str())
            .push(" LIMIT 1001");
        let result = sql(&db, query.as_str()).await?;
        db.close().await;
        require(
            !result.truncated && result.rows.len() <= 1000,
            "文章授权记录超过配额或被截断",
        )?;
        result
            .rows
            .iter()
            .map(|row| {
                row.first()
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .context("文章授权记录无效")
            })
            .collect::<anyhow::Result<std::collections::BTreeSet<_>>>()
            .map_err(Error::from)
    }

    pub async fn replace_article_policies(&self, rows: Vec<ArticlePolicyRow>) -> Result<()> {
        require(rows.len() <= 1000, "文章策略记录超过配额")?;
        self.transaction(|db| async move {
            sql(&db, "DELETE FROM blog_article_policy").await?;
            for row in rows {
                let mut query = Query::new(
                    "INSERT INTO blog_article_policy (article, visibility, hidden, review, blocked_hash, password_hash) VALUES (",
                )
                .bind(row.article.as_str())
                .push(", ")
                .bind(row.visibility.as_str())
                .push(", ")
                .bind(row.hidden.as_str())
                .push(", ")
                .bind(row.review.as_str())
                .push(", ");
                query = match row.blocked_hash {
                    Some(hash) => query.bind(hash.as_str()),
                    None => query.push("NULL"),
                };
                query = query.push(", ");
                query = match row.password_hash {
                    Some(hash) => query.bind(hash.as_str()),
                    None => query.push("NULL"),
                };
                sql(&db, query.push(")").as_str()).await?;
            }
            Ok(())
        })
        .await
    }

    pub async fn has_article_access(
        &self,
        user_id: i64,
        article: &str,
        source: AccessSource,
    ) -> Result<bool> {
        let db = self.database.connect().await?;
        let query = Query::new("SELECT user_id FROM blog_article_access WHERE user_id = ")
            .bind(user_id)
            .push(" AND article = ")
            .bind(article)
            .push(" AND source = ")
            .bind(source.as_str())
            .push(" LIMIT 2");
        let result = sql(&db, query.as_str()).await?;
        db.close().await;
        require(
            !result.truncated && result.rows.len() <= 1,
            "文章授权记录不唯一或被截断",
        )?;
        Ok(!result.rows.is_empty())
    }

    pub async fn grant_article_access(
        &self,
        user_id: i64,
        article: &str,
        source: AccessSource,
    ) -> Result<()> {
        self.transaction(|db| async move {
            let delete = Query::new("DELETE FROM blog_article_access WHERE user_id = ")
                .bind(user_id)
                .push(" AND article = ")
                .bind(article);
            sql(&db, delete.as_str()).await?;
            let insert = Query::new(
                "INSERT INTO blog_article_access (id, user_id, article, source, created_at) VALUES (",
            )
            .bind(format!("{user_id}~{article}").as_str())
            .push(", ")
            .bind(user_id)
            .push(", ")
            .bind(article)
            .push(", ")
            .bind(source.as_str())
            .push(", ")
            .bind(now().as_str())
            .push(")");
            sql(&db, insert.as_str()).await?;
            Ok(())
        })
        .await
    }

    /// Password rotation invalidates every PASSWORD grant for that article; explicit TOP grants remain.
    pub async fn revoke_password_access(&self, article: &str) -> Result<()> {
        self.revoke_access(article, AccessSource::Password).await
    }

    pub async fn revoke_granted_access(&self, article: &str) -> Result<()> {
        self.revoke_access(article, AccessSource::Granted).await
    }

    async fn revoke_access(&self, article: &str, source: AccessSource) -> Result<()> {
        let db = self.database.connect().await?;
        let query = Query::new("DELETE FROM blog_article_access WHERE article = ")
            .bind(article)
            .push(" AND source = ")
            .bind(source.as_str());
        sql(&db, query.as_str()).await?;
        db.close().await;
        Ok(())
    }

    pub async fn revoke_article_access(&self, user_id: i64, article: &str) -> Result<()> {
        let db = self.database.connect().await?;
        let query = Query::new("DELETE FROM blog_article_access WHERE user_id = ")
            .bind(user_id)
            .push(" AND article = ")
            .bind(article);
        sql(&db, query.as_str()).await?;
        db.close().await;
        Ok(())
    }

    pub async fn article_access_users(&self, article: &str) -> Result<Vec<ArticleAccess>> {
        let db = self.database.connect().await?;
        let query = Query::new("SELECT user_id, source FROM blog_article_access WHERE article = ")
            .bind(article)
            .push(" LIMIT 1001");
        let access = sql(&db, query.as_str()).await?;
        let users = sql(&db, &format!("SELECT {COLUMNS} FROM blog_users LIMIT 1001")).await?;
        db.close().await;
        require(
            !access.truncated
                && access.rows.len() <= 1000
                && !users.truncated
                && users.rows.len() <= 1000,
            "文章授权列表超过配额或被截断",
        )?;
        let users = users
            .rows
            .iter()
            .map(|row| decode(row).map_err(Error::from))
            .collect::<Result<Vec<_>>>()?;
        access
            .rows
            .iter()
            .map(|row| {
                let user_id = row
                    .first()
                    .and_then(Value::as_i64)
                    .context("文章授权用户 ID 无效")?;
                let source = AccessSource::parse(
                    row.get(1)
                        .and_then(Value::as_str)
                        .context("文章授权来源无效")?,
                )?;
                let user = users
                    .iter()
                    .find(|user| user.id == user_id)
                    .cloned()
                    .context("文章授权用户不存在")?;
                Ok(ArticleAccess { user, source })
            })
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(Error::from)
    }

    pub async fn grant(&self, expected_actor: &User, name: &str, level: Level) -> Result<()> {
        self.transaction(|db| async move {
            let actor = get(&db, &expected_actor.username)
                .await?
                .ok_or(Error::Denied)?;
            if !actor.same_revision(expected_actor)
                || actor.level != Level::Master
                || (actor.username == name && level != Level::Master)
                || (actor.username != name && level == Level::Master)
            {
                return Err(Error::Denied);
            }
            let target = get(&db, name).await?.ok_or(Error::NotFound)?;
            let query = Query::new("UPDATE blog_users SET permission_level = ")
                .bind(level.as_str())
                .push(" WHERE id = ")
                .bind(target.id);
            sql(&db, query.as_str()).await?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
#[path = "../../../test/api/accounts_db.rs"]
mod tests;
