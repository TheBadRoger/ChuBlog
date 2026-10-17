use crate::{ApiError, ApiResult, App, bad, internal, origin};
use anyhow::{Context, Result, ensure};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    path::Path as FilePath,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Reader,
    Publisher,
    Admin,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub enabled: bool,
    pub role: Role,
}
#[derive(Clone)]
struct Session {
    user: crate::accounts_db::User,
    csrf: String,
    expires: Instant,
}
pub struct Accounts {
    pub store: crate::accounts_db::Store,
    sessions: BTreeMap<String, Session>,
    dummy_hash: String,
    attempts: VecDeque<Instant>,
    git_failures: VecDeque<Instant>,
    changes: VecDeque<Instant>,
}

pub async fn atomic(path: &FilePath, bytes: &[u8], mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let tmp = path.with_extension("tmp");
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(mode)
        .open(&tmp)
        .await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    tokio::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode)).await?;
    tokio::fs::rename(&tmp, path).await?;
    let directory = std::fs::File::open(path.parent().context("文件缺少父目录")?)?;
    directory.sync_all()?;
    Ok(())
}
pub(crate) fn username(value: &str) -> Result<()> {
    ensure!(
        (3..=32).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_lowercase()),
        "用户名必须为 3–32 位小写英文字母"
    );
    Ok(())
}
pub(crate) fn nickname(value: &str) -> Result<()> {
    ensure!(
        (1..=128).contains(&value.len()),
        "昵称需要 1–128 个 UTF-8 字节"
    );
    Ok(())
}
fn password(value: &str) -> Result<()> {
    ensure!(
        (12..=1024).contains(&value.len()),
        "口令必须为 12–1024 字节"
    );
    Ok(())
}
pub async fn hash_password(value: String) -> Result<String> {
    password(&value)?;
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(value.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|hash| hash.to_string())
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    })
    .await?
}
pub async fn load(store: crate::accounts_db::Store) -> Result<Accounts> {
    Ok(Accounts {
        store,
        sessions: BTreeMap::new(),
        dummy_hash: hash_password(token()).await?,
        attempts: VecDeque::new(),
        git_failures: VecDeque::new(),
        changes: VecDeque::new(),
    })
}
pub async fn store(app: &App) -> crate::accounts_db::Store {
    app.accounts.lock().await.store.clone()
}
pub fn database_error(error: crate::accounts_db::Error) -> ApiError {
    use crate::accounts_db::Error;
    let status = match error {
        Error::Conflict => StatusCode::CONFLICT,
        Error::NotFound => StatusCode::NOT_FOUND,
        Error::Denied => StatusCode::FORBIDDEN,
        Error::Quota => StatusCode::BAD_REQUEST,
        Error::InstallationConflict => StatusCode::CONFLICT,
        Error::Database(source) => {
            let (reason, message) = database_problem(&source);
            tracing::error!(
                action = "account_database_unavailable",
                reason,
                database_detail = database_detail(&source),
                operation = source
                    .downcast_ref::<&'static str>()
                    .copied()
                    .unwrap_or("connect_or_decode"),
                "账户数据库操作失败"
            );
            return ApiError(StatusCode::SERVICE_UNAVAILABLE, message.into());
        }
    };
    ApiError(status, error.to_string())
}
// Only fixed diagnostic labels reach logs; server errors can contain submitted values.
fn database_detail(source: &anyhow::Error) -> &'static str {
    let Some(chusql_driver::Error::Server { message, .. }) =
        source.downcast_ref::<chusql_driver::Error>()
    else {
        return "connection_or_decode";
    };
    if message.starts_with("NOT NULL: column id ") {
        "id_not_generated"
    } else if message.starts_with("NOT NULL:") {
        "required_column_missing"
    } else if message.starts_with("UNIQUE:") {
        "unique_constraint"
    } else if message.starts_with("cannot put ") {
        "column_type_mismatch"
    } else if message.starts_with("invalid ENUM value") {
        "enum_value_rejected"
    } else if message.starts_with("timestamp must ") {
        "timestamp_format"
    } else if message.starts_with("value is longer than ") {
        "column_length"
    } else {
        "unclassified_server_error"
    }
}
fn database_problem(source: &anyhow::Error) -> (&'static str, &'static str) {
    match source.downcast_ref::<chusql_driver::Error>() {
        Some(chusql_driver::Error::Server { code, message }) => match code.as_str() {
            "forbidden" => (
                "permissions",
                "账户数据库权限配置异常，请重新执行部署初始化",
            ),
            "unauthorized" => (
                "credentials",
                "账户数据库连接凭据校验失败，请核对数据库配置",
            ),
            _ if message.contains("unknown column")
                || message.contains("unknown table")
                || message.contains("unknown database") =>
            {
                ("schema", "账户数据库结构与当前版本不一致，请核对初始化结果")
            }
            _ => ("query", "账户数据库操作失败，请查看服务日志中的原因分类"),
        },
        Some(
            chusql_driver::Error::Io(_)
            | chusql_driver::Error::Timeout
            | chusql_driver::Error::Closed,
        ) => ("connection", "账户数据库暂时不可用，请稍后重试"),
        _ => ("data", "账户数据校验失败，请核对数据库结构与记录"),
    }
}
async fn authenticate(
    app: &App,
    name: &str,
    password: String,
) -> ApiResult<crate::accounts_db::User> {
    let store = store(app).await;
    let user = store.user(name).await.map_err(database_error)?;
    let hash = match &user {
        Some(user) => user.hash.clone(),
        None => app.accounts.lock().await.dummy_hash.clone(),
    };
    let permit = app
        .hash_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "认证服务繁忙".into()))?;
    let verified = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&hash).ok().is_some_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .map_err(internal)?;
    let denied = || ApiError(StatusCode::UNAUTHORIZED, "登录失败".into());
    let expected = user.filter(|_| verified).ok_or_else(denied)?;
    let current = store
        .user(name)
        .await
        .map_err(database_error)?
        .ok_or_else(denied)?;
    if !expected.same_revision(&current) {
        return Err(denied());
    }
    if current.level == crate::accounts_db::Level::Banned {
        return Err(ApiError(StatusCode::FORBIDDEN, "用户已停用".into()));
    }
    Ok(current)
}

pub(crate) fn rate(accounts: &mut Accounts) -> ApiResult<()> {
    let now = Instant::now();
    while accounts
        .attempts
        .front()
        .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60))
    {
        accounts.attempts.pop_front();
    }
    // One global bucket also bounds attacks distributed across IPs and account names.
    if accounts.attempts.len() >= 10 {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "认证请求过于频繁，请稍后再试".into(),
        ));
    }
    accounts.attempts.push_back(now);
    Ok(())
}
pub(crate) fn token() -> String {
    let mut bytes = [0; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
pub async fn authorize(
    app: &App,
    headers: &HeaderMap,
    change: bool,
    admin: bool,
) -> ApiResult<crate::accounts_db::User> {
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .filter_map(|p| p.trim().split_once('='))
        .find(|(name, _)| *name == "chublog_session")
        .map(|(_, value)| value)
        .unwrap_or("");
    let session = app
        .accounts
        .lock()
        .await
        .sessions
        .get(cookie)
        .filter(|s| s.expires > Instant::now())
        .cloned()
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "请先登录".into()))?;
    let user = store(app)
        .await
        .user(&session.user.username)
        .await
        .map_err(database_error)?
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "会话已失效".into()))?;
    if user.level == crate::accounts_db::Level::Banned {
        return Err(ApiError(StatusCode::FORBIDDEN, "用户已停用".into()));
    }
    if !user.same_revision(&session.user) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "会话已失效，请重新登录".into(),
        ));
    }
    if admin && user.level != crate::accounts_db::Level::Master {
        return Err(ApiError(StatusCode::FORBIDDEN, "需要管理员权限".into()));
    }
    if change {
        origin(headers)?;
        if headers.get("x-csrf-token").and_then(|v| v.to_str().ok()) != Some(&session.csrf) {
            return Err(ApiError(StatusCode::FORBIDDEN, "CSRF 校验失败".into()));
        }
        let mut accounts = app.accounts.lock().await;
        let now = Instant::now();
        accounts
            .changes
            .retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        if accounts.changes.len() >= 60 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "修改请求过于频繁".into(),
            ));
        }
        accounts.changes.push_back(now);
    }
    Ok(user)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Login {
    username: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Register {
    username: String,
    nickname: String,
    password: String,
}
pub async fn register(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<Register>,
) -> ApiResult<Json<Value>> {
    origin(&headers)?;
    if !app.registration {
        return Err(ApiError(StatusCode::FORBIDDEN, "注册已关闭".into()));
    }
    username(&input.username).map_err(bad)?;
    nickname(&input.nickname).map_err(bad)?;
    password(&input.password).map_err(bad)?;
    rate(&mut *app.accounts.lock().await)?;
    let _permit = app
        .hash_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "认证服务繁忙".into()))?;
    let hash = hash_password(input.password).await.map_err(internal)?;
    let user = store(&app)
        .await
        .register(&input.username, &input.nickname, &hash)
        .await
        .map_err(database_error)?;
    tracing::info!(action = "register", user = %user.username);
    let mut view = user.view();
    view["role"] = json!(user.level.grant().role);
    Ok(Json(view))
}
pub async fn login(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> ApiResult<Response> {
    let secure = origin(&headers)?;
    username(&input.username).map_err(bad)?;
    password(&input.password).map_err(bad)?;
    rate(&mut *app.accounts.lock().await)?;
    let user = authenticate(&app, &input.username, input.password).await?;
    let mut accounts = app.accounts.lock().await;
    let user = accounts
        .store
        .logged_in(&user)
        .await
        .map_err(database_error)?;
    let now = Instant::now();
    accounts
        .sessions
        .retain(|_, session| session.expires > now && session.user.id != user.id);
    if accounts.sessions.len() >= 1000 {
        return Err(bad("会话配额已满"));
    }
    let id = token();
    let csrf = token();
    accounts.sessions.insert(
        id.clone(),
        Session {
            user: user.clone(),
            csrf: csrf.clone(),
            expires: now + Duration::from_secs(3600),
        },
    );
    let cookie = format!(
        "chublog_session={id}; Path=/; HttpOnly; SameSite=Strict; Max-Age=3600{}",
        if secure { "; Secure" } else { "" }
    );
    let mut view = user.view();
    view["role"] = json!(user.level.grant().role);
    view["csrf"] = json!(csrf);
    let mut response = Json(view).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(internal)?,
    );
    tracing::info!(action = "login", user = %user.username);
    Ok(response)
}
pub async fn me(State(app): State<Arc<App>>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let user = authorize(&app, &headers, false, false).await?;
    let accounts = app.accounts.lock().await;
    let csrf = accounts
        .sessions
        .values()
        .find(|s| s.user.id == user.id)
        .map(|s| &s.csrf);
    let mut view = user.view();
    view["permissions"] = json!(user.level.grant());
    view["csrf"] = json!(csrf);
    Ok(Json(view))
}
pub async fn logout(State(app): State<Arc<App>>, headers: HeaderMap) -> ApiResult<Response> {
    let secure = origin(&headers)?;
    let user = authorize(&app, &headers, true, false).await?;
    app.accounts
        .lock()
        .await
        .sessions
        .retain(|_, session| session.user.id != user.id);
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "chublog_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
            if secure { "; Secure" } else { "" }
        ))
        .map_err(internal)?,
    );
    Ok(response)
}
pub async fn users(State(app): State<Arc<App>>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    authorize(&app, &headers, false, true).await?;
    let users = store(&app).await.users().await.map_err(database_error)?;
    let views: BTreeMap<_, _> = users
        .into_iter()
        .map(|user| {
            let mut view = user.view();
            let grant = user.level.grant();
            view["enabled"] = json!(grant.enabled);
            view["role"] = json!(grant.role);
            (user.username, view)
        })
        .collect();
    Ok(Json(json!(views)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantInput {
    enabled: bool,
    role: Role,
}
pub async fn grant(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(input): Json<GrantInput>,
) -> ApiResult<Json<Value>> {
    let actor = authorize(&app, &headers, true, true).await?;
    let mut accounts = app.accounts.lock().await;
    accounts
        .store
        .grant(
            &actor,
            &name,
            crate::accounts_db::Level::from_grant(input.enabled, input.role),
        )
        .await
        .map_err(database_error)?;
    accounts.sessions.retain(|_, session| {
        session.user.username != name || session.user.username == actor.username
    });
    tracing::info!(action = "permission_changed", user = %name, administrator = %actor.username);
    Ok(Json(json!({"status":"saved"})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordInput {
    old_password: String,
    new_password: String,
}
pub async fn change_password(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<PasswordInput>,
) -> ApiResult<Json<Value>> {
    let user = authorize(&app, &headers, true, false).await?;
    password(&input.old_password).map_err(bad)?;
    password(&input.new_password).map_err(bad)?;
    let verified = authenticate(&app, &user.username, input.old_password).await?;
    if !verified.same_revision(&user) {
        return Err(ApiError(StatusCode::CONFLICT, "凭据已变更".into()));
    }
    let _permit = app
        .hash_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "认证服务繁忙".into()))?;
    let hash = hash_password(input.new_password).await.map_err(internal)?;
    let mut accounts = app.accounts.lock().await;
    accounts
        .store
        .change_password(&user, &hash)
        .await
        .map_err(database_error)?;
    accounts
        .sessions
        .retain(|_, session| session.user.id != user.id);
    tracing::info!(action = "password_changed", user = %user.username);
    Ok(Json(
        json!({"status":"saved","message":"请使用新口令重新登录"}),
    ))
}
pub async fn git_credentials(app: &App, name: &str, secret: String) -> ApiResult<String> {
    username(name).map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "认证失败".into()))?;
    password(&secret).map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "认证失败".into()))?;
    {
        let mut accounts = app.accounts.lock().await;
        let now = Instant::now();
        accounts
            .git_failures
            .retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        if accounts.git_failures.len() >= 20 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "Git 认证失败过于频繁".into(),
            ));
        }
    }
    let user = match authenticate(app, name, secret).await {
        Ok(user) => user,
        Err(error) => {
            if error.0 == StatusCode::UNAUTHORIZED {
                app.accounts
                    .lock()
                    .await
                    .git_failures
                    .push_back(Instant::now());
            }
            return Err(error);
        }
    };
    if user.level == crate::accounts_db::Level::Member {
        return Err(ApiError(StatusCode::FORBIDDEN, "未授权访问文章仓库".into()));
    }
    Ok(user.hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn database_diagnostics_classify_write_failures_without_exposing_values() {
        for (detail, expected) in [
            ("NOT NULL: column id cannot be null", "id_not_generated"),
            ("cannot put this value into TInt", "column_type_mismatch"),
            (
                "invalid ENUM value for blog_permission: private input",
                "enum_value_rejected",
            ),
            (
                "timestamp must look like YYYY-MM-DD HH:MM:SS",
                "timestamp_format",
            ),
            ("value is longer than 32 characters", "column_length"),
            (
                "private SQL containing a password hash",
                "unclassified_server_error",
            ),
        ] {
            let error = anyhow::Error::new(chusql_driver::Error::Server {
                code: "query_error".into(),
                message: detail.into(),
            })
            .context("create_account");
            assert_eq!(database_detail(&error), expected);
        }
    }
    #[test]
    fn database_errors_report_safe_categories_instead_of_one_outage_message() {
        for (code, detail, expected) in [
            ("forbidden", "administrator required", "权限配置异常"),
            (
                "unauthorized",
                "secret credential detail",
                "连接凭据校验失败",
            ),
            (
                "query_error",
                "unknown column: nickname",
                "结构与当前版本不一致",
            ),
        ] {
            let error = database_error(crate::accounts_db::Error::Database(
                chusql_driver::Error::Server {
                    code: code.into(),
                    message: detail.into(),
                }
                .into(),
            ));
            assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);
            assert!(error.1.contains(expected));
            assert!(!error.1.contains(detail));
        }
    }
}
