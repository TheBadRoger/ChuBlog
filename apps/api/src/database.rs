use anyhow::{Context, Result, ensure};
use chusql_driver::{Client, Options};
use std::time::Duration;

pub const ACCOUNTS_SQL: &str = include_str!("../sql/accounts.sql");
pub const ARTICLES_SQL: &str = include_str!("../sql/articles.sql");
pub const CREATE_DATABASE_SQL: &str = include_str!("../sql/create-database.sql");
const COMPLETE_EMPTY_INSTALL_SQL: &str = include_str!("../sql/complete-empty-install.sql");

// Deployment stops the API first. Root drops empty tables; the application creates their replacements.
pub async fn complete_empty_install(db: &Client) -> Result<()> {
    for table in ["blog_users", "blog_site"] {
        let count = match db.query(&format!("SELECT count(*) FROM {table}")).await {
            Ok(count) => count,
            Err(chusql_driver::Error::Server { message, .. })
                if message.contains("unknown table") =>
            {
                return Ok(());
            }
            Err(error) => return Err(error.into()),
        };
        let count = count
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(chusql_driver::Value::as_u64)
            .context("安装记录数量无效")?;
        if count != 0 {
            let columns = db.query("SELECT * FROM blog_users LIMIT 0").await?;
            ensure!(
                columns.columns.iter().any(|column| column == "nickname"),
                "账户结构缺少 nickname，且安装数据已存在；请先备份并核对账户结构"
            );
            return Ok(());
        }
    }
    execute_script(db, COMPLETE_EMPTY_INSTALL_SQL)
        .await
        .context("重建空账户与安装表失败")?;
    Ok(())
}

#[derive(Clone)]
pub struct Database {
    pub address: String,
    pub name: String,
    pub user: String,
    pub password: String,
}
impl Database {
    pub fn from_env() -> Result<Self> {
        let name = std::env::var("CHUSQL_DATABASE").unwrap_or_else(|_| "chublog".into());
        identifier(&name)?;
        Ok(Self {
            address: std::env::var("CHUSQL_ADDRESS").unwrap_or_else(|_| "127.0.0.1:7777".into()),
            name,
            user: std::env::var("CHUSQL_USER").unwrap_or_else(|_| "chublog_index".into()),
            password: std::env::var("CHUSQL_PASSWORD").context("未配置 ChuSQL 账户口令")?,
        })
    }
    pub async fn connect(&self) -> Result<Client> {
        let db = Client::connect_with_options(
            &self.address,
            &self.user,
            &self.password,
            Options {
                timeout: Duration::from_secs(3),
                max_response_bytes: 1024 * 1024,
            },
        )
        .await?;
        db.query(&format!("USE {}", self.name)).await?;
        Ok(db)
    }
    pub fn configure_child(&self, command: &mut tokio::process::Command) {
        command
            .env("CHUSQL_ADDRESS", &self.address)
            .env("CHUSQL_DATABASE", &self.name)
            .env("CHUSQL_USER", &self.user)
            .env("CHUSQL_PASSWORD", &self.password);
    }
}
fn identifier(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && !value.starts_with(|c: char| c.is_ascii_digit())
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "SQL 标识符无效"
    );
    Ok(())
}

// ChuSQL TIMESTAMP accepts second precision without a timezone. Store UTC only.
pub fn to_timestamp(value: &str) -> Result<String> {
    Ok(chrono::DateTime::parse_from_rfc3339(value)?
        .with_timezone(&chrono::Utc)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string())
}
pub fn from_timestamp(value: &str) -> Result<String> {
    Ok(
        chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")?
            .and_utc()
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}

pub fn statements(script: &str) -> Result<Vec<String>> {
    let mut chars = script.chars().peekable();
    let mut result = Vec::new();
    let mut statement = String::new();
    let mut quote = None;
    let mut block = 0;
    while let Some(ch) = chars.next() {
        if let Some(mark) = quote {
            statement.push(ch);
            if ch == mark {
                if chars.peek() == Some(&mark) {
                    statement.push(chars.next().unwrap());
                } else {
                    quote = None;
                }
            }
        } else if block > 0 {
            if ch == '*' && chars.peek() == Some(&'/') {
                chars.next();
                block -= 1;
            } else if ch == '/' && chars.peek() == Some(&'*') {
                chars.next();
                block += 1;
            }
        } else if ch == '-' && chars.peek() == Some(&'-') {
            chars.next();
            for next in chars.by_ref() {
                if next == '\n' {
                    break;
                }
            }
            statement.push(' ');
        } else if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            block = 1;
            statement.push(' ');
        } else if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            statement.push(ch);
        } else if ch == ';' {
            if !statement.trim().is_empty() {
                result.push(statement.trim().to_owned());
            }
            statement.clear();
        } else {
            statement.push(ch);
        }
    }
    ensure!(
        quote.is_none() && block == 0,
        "SQL 脚本有未闭合的引号或注释"
    );
    if !statement.trim().is_empty() {
        result.push(statement.trim().to_owned());
    }
    Ok(result)
}

pub async fn execute_script(db: &Client, script: &str) -> Result<()> {
    for sql in statements(script)? {
        db.query(&sql).await?;
    }
    Ok(())
}
// ChuSQL reserves named type creation for its database administrator.
pub async fn ensure_types(db: &Client, script: &str) -> Result<()> {
    for sql in statements(script)? {
        let mut words = sql.split_whitespace();
        if words
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("CREATE"))
            && words
                .next()
                .is_some_and(|word| word.eq_ignore_ascii_case("TYPE"))
        {
            ensure_schema(db, &sql).await?;
        }
    }
    Ok(())
}
pub async fn ensure_schema(db: &Client, script: &str) -> Result<()> {
    for sql in statements(script)? {
        let words: Vec<_> = sql.split_whitespace().take(3).collect();
        ensure!(
            words.len() == 3
                && words[0].eq_ignore_ascii_case("CREATE")
                && (words[1].eq_ignore_ascii_case("TABLE")
                    || words[1].eq_ignore_ascii_case("TYPE")),
            "初始化脚本只接受 CREATE TABLE 或 CREATE TYPE"
        );
        identifier(words[2])?;
        if words[1].eq_ignore_ascii_case("TYPE") {
            let domains = db.query("SHOW DOMAINS").await?;
            ensure!(!domains.truncated, "自定义类型列表被截断");
            if !domains
                .rows
                .iter()
                .any(|row| row.first().and_then(chusql_driver::Value::as_str) == Some(words[2]))
            {
                db.query(&sql).await?;
            }
            continue;
        }
        if db
            .query(&format!("SELECT * FROM {} LIMIT 0", words[2]))
            .await
            .is_err()
        {
            db.query(&sql).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamps_use_utc_seconds_at_the_database_boundary() {
        assert_eq!(
            to_timestamp("2026-10-17T12:34:56.789+08:00").unwrap(),
            "2026-10-17 04:34:56"
        );
        assert_eq!(
            from_timestamp("2026-10-17 04:34:56").unwrap(),
            "2026-10-17T04:34:56Z"
        );
        assert!(to_timestamp("").is_err());
        assert!(from_timestamp("2026-99-99 00:00:00").is_err());
    }
    #[test]
    fn sql_scripts_preserve_literals_and_reject_unfinished_input() {
        let result = statements("-- setup\nCREATE TABLE t (x TEXT); /* a /* nested */ b */ INSERT INTO t VALUES ('a;''b--c');").unwrap();
        assert_eq!(result.len(), 2);
        assert!(result[1].contains("'a;''b--c'"));
        assert!(statements("SELECT 'oops").is_err());
        assert!(statements("/* oops").is_err());
        assert_eq!(statements(ACCOUNTS_SQL).unwrap().len(), 3);
    }
}
