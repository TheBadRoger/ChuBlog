use crate::{
    accounts::{Grant, Role},
    database::{self, Database},
    git_store,
};
use anyhow::Context;
use chusql_driver::{Client, Query, QueryResult, Value};
use serde::Deserialize;
use std::{collections::BTreeMap, future::Future, path::Path};

#[derive(Debug)]
pub enum Error {
    Database(anyhow::Error),
    Conflict,
    NotFound,
    Denied,
    Quota,
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
#[repr(i64)]
pub enum Level {
    Banned = 0,
    Member = 1,
    Author = 2,
    Master = 3,
}
impl Level {
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
            prefixes: vec![],
            _legacy_ssh_key: None,
        }
    }
}
#[derive(Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
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
        serde_json::json!({"id":self.id,"username":self.username,"created_at":self.created_at,
            "last_login_at":self.last_login_at,"permission_level":self.level as i64})
    }
}
#[derive(Clone)]
pub struct Store {
    pub database: Database,
}
const COLUMNS: &str = "id, username, password_hash, created_at, last_login_at, permission_level";
const IMPORT: &str = "legacy_json_v1";
fn require(condition: bool, message: &'static str) -> anyhow::Result<()> {
    if !condition {
        anyhow::bail!(message);
    }
    Ok(())
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
async fn sql(db: &Client, query: &str) -> Result<QueryResult> {
    db.query(query)
        .await
        .map_err(|error| Error::Database(error.into()))
}
fn decode(row: &[Value]) -> anyhow::Result<User> {
    require(row.len() == 6, "账户记录字段数量无效")?;
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
    let hash = text(2)?;
    let parsed = argon2::PasswordHash::new(&hash).map_err(|_| anyhow::anyhow!("密码哈希无效"))?;
    require(
        parsed.algorithm.as_str() == "argon2id",
        "密码必须使用 Argon2id 哈希",
    )?;
    let created_at = text(3)?;
    chrono::DateTime::parse_from_rfc3339(&created_at)?;
    let last_login_at = if row[4].is_null() {
        None
    } else {
        Some(text(4)?)
    };
    if let Some(time) = &last_login_at {
        chrono::DateTime::parse_from_rfc3339(time)?;
    }
    let level = match row[5].as_i64() {
        Some(0) => Level::Banned,
        Some(1) => Level::Member,
        Some(2) => Level::Author,
        Some(3) => Level::Master,
        _ => anyhow::bail!("未知账户权限等级，拒绝授权"),
    };
    Ok(User {
        id,
        username,
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
    pub async fn names(&self) -> Result<Vec<String>> {
        let db = self.database.connect().await?;
        let result = sql(&db, "SELECT username FROM blog_users LIMIT 1001").await?;
        db.close().await;
        require(
            !result.truncated && result.rows.len() <= 1000,
            "账户列表超过配额或被截断",
        )?;
        result
            .rows
            .iter()
            .map(|row| {
                row.first()
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .context("账户名称无效")
                    .map_err(Error::from)
            })
            .collect()
    }
    pub async fn register(&self, name: &str, hash: &str) -> Result<User> {
        self.transaction(|db| async move {
            if get(&db, name).await?.is_some() { return Err(Error::Conflict); }
            let count = sql(&db, "SELECT count(*) FROM blog_users").await?;
            let count = count.rows.first().and_then(|row| row.first()).and_then(Value::as_u64)
                .context("用户数量查询无效")?;
            if count >= 1000 { return Err(Error::Quota); }
            let query = Query::new("INSERT INTO blog_users (username, password_hash, created_at, last_login_at, permission_level) VALUES (")
                .bind(name).push(", ").bind(hash).push(", ").bind(now().as_str()).push(", NULL, 1)");
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
    pub async fn grant(&self, expected_actor: &User, name: &str, level: Level) -> Result<()> {
        self.transaction(|db| async move {
            let actor = get(&db, &expected_actor.username)
                .await?
                .ok_or(Error::Denied)?;
            if !actor.same_revision(expected_actor)
                || actor.level != Level::Master
                || (actor.username == name && level != Level::Master)
            {
                return Err(Error::Denied);
            }
            let target = get(&db, name).await?.ok_or(Error::NotFound)?;
            let query = Query::new("UPDATE blog_users SET permission_level = ")
                .bind(level as i64)
                .push(" WHERE id = ")
                .bind(target.id);
            sql(&db, query.as_str()).await?;
            Ok(())
        })
        .await
    }

    pub async fn initialize(
        &self,
        data: &Path,
        config: &Path,
        repository: &Path,
    ) -> anyhow::Result<()> {
        let marker = data.join("accounts-chusql-v1");
        if let Ok(value) = tokio::fs::read_to_string(&marker).await {
            require(
                value.trim() == self.database.name,
                "账户数据库名称与迁移标记不一致，拒绝自动切换",
            )?;
            return Ok(()); // Never recreate lost identity tables or fall back to JSON on restart.
        }
        let db = self.database.connect().await?;
        database::ensure_tables(&db, database::ACCOUNTS_SQL).await?;
        db.close().await;
        #[derive(Deserialize)]
        struct Credential {
            hash: String,
        }
        let legacy: BTreeMap<String, Credential> =
            match tokio::fs::read(data.join("users.json")).await {
                Ok(bytes) => {
                    require(bytes.len() <= 1024 * 1024, "旧账户文件过大")?;
                    serde_json::from_slice(&bytes)?
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
                Err(error) => return Err(error.into()),
            };
        require(legacy.len() <= 1000, "旧账户超过配额")?;
        let grants: Option<git_store::Config> = match tokio::fs::read(config).await {
            Ok(bytes) => {
                require(bytes.len() <= 256 * 1024, "旧授权文件过大")?;
                Some(serde_json::from_slice(&bytes)?)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        if let Some(grants) = &grants {
            require(
                grants.repository == repository,
                "旧授权配置的接收目录不匹配",
            )?;
        }
        self.transaction(|db| async move {
            let done = sql(&db, "SELECT name FROM blog_account_migrations WHERE name = 'legacy_json_v1'").await?;
            if !done.rows.is_empty() { return Ok(()); }
            let imported_at = now();
            for (name, credential) in &legacy {
                crate::accounts::username(name)?;
                let parsed = argon2::PasswordHash::new(&credential.hash).map_err(|_| anyhow::anyhow!("旧密码哈希无效"))?;
                require(parsed.algorithm.as_str() == "argon2id", "旧密码必须使用 Argon2id 哈希")?;
                let grant = grants.as_ref().and_then(|cfg| cfg.publishers.get(name));
                let level = grant.map(|grant| Level::from_grant(grant.enabled, grant.role)).unwrap_or(Level::Banned);
                if let Some(existing) = get(&db, name).await? {
                    require(existing.hash == credential.hash && existing.level == level, "旧账户与数据库记录冲突，拒绝覆盖")?;
                    continue;
                }
                let query = Query::new("INSERT INTO blog_users (username, password_hash, created_at, last_login_at, permission_level) VALUES (")
                    .bind(name.as_str()).push(", ").bind(credential.hash.as_str()).push(", ").bind(imported_at.as_str())
                    .push(", NULL, ").bind(level as i64).push(")");
                sql(&db, query.as_str()).await?;
            }
            let query = Query::new("INSERT INTO blog_account_migrations (name, completed_at) VALUES (").bind(IMPORT)
                .push(", ").bind(imported_at.as_str()).push(")");
            sql(&db, query.as_str()).await?;
            Ok(())
        }).await?;
        crate::accounts::atomic(&marker, self.database.name.as_bytes(), 0o600).await?;
        // Backups are retained for recovery only. Authentication never reads them.
        for source in [data.join("users.json"), config.to_owned()] {
            if source.exists() {
                let backup = source.with_extension("json.pre-chusql.bak");
                require(!backup.exists(), "旧账户备份已存在，需人工核对")?;
                tokio::fs::rename(source, backup).await?;
            }
        }
        Ok(())
    }
}
