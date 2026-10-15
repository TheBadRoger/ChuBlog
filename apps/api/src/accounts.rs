use crate::{ApiError, ApiResult, App, bad, internal, origin, ssh};
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
    pub prefixes: Vec<String>,
    pub ssh_key: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Credential {
    hash: String,
}
struct Session {
    username: String,
    csrf: String,
    expires: Instant,
}
pub struct Accounts {
    credentials: BTreeMap<String, Credential>,
    pub grants: ssh::Config,
    sessions: BTreeMap<String, Session>,
    attempts: VecDeque<Instant>,
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
fn username(value: &str) -> Result<()> {
    ensure!(
        (3..=32).contains(&value.len())
            && value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && value.starts_with(|c: char| c.is_ascii_lowercase()),
        "用户名必须为 3–32 位小写字母、数字或短横线，以字母开头"
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
pub async fn load(
    data: &FilePath,
    config: &FilePath,
    repository: std::path::PathBuf,
) -> Result<Accounts> {
    let credentials: BTreeMap<String, Credential> =
        match tokio::fs::read(data.join("users.json")).await {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
    let grants = match tokio::fs::read(config).await {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ssh::Config {
            repository,
            publishers: BTreeMap::new(),
        },
        Err(error) => return Err(error.into()),
    };
    let mut accounts = Accounts {
        credentials,
        grants,
        sessions: BTreeMap::new(),
        attempts: VecDeque::new(),
        changes: VecDeque::new(),
    };
    if accounts.credentials.is_empty() {
        let name = std::env::var("ADMIN_USERNAME").unwrap_or_else(|_| "admin".into());
        username(&name)?;
        let hash =
            std::env::var("ADMIN_PASSWORD_HASH").context("首次启动必须设置 ADMIN_PASSWORD_HASH")?;
        let parsed = PasswordHash::new(&hash).map_err(|_| anyhow::anyhow!("管理员哈希格式无效"))?;
        ensure!(
            parsed.algorithm.as_str() == "argon2id",
            "管理员哈希必须为 Argon2id"
        );
        accounts
            .credentials
            .insert(name.clone(), Credential { hash });
        accounts.grants.publishers.insert(
            name,
            Grant {
                enabled: true,
                role: Role::Admin,
                prefixes: vec![],
                ssh_key: None,
            },
        );
        atomic(config, &serde_json::to_vec_pretty(&accounts.grants)?, 0o644).await?;
        atomic(
            &data.join("users.json"),
            &serde_json::to_vec(&accounts.credentials)?,
            0o600,
        )
        .await?;
    }
    ensure!(
        accounts
            .grants
            .publishers
            .iter()
            .any(|(name, g)| g.role == Role::Admin
                && g.enabled
                && accounts.credentials.contains_key(name)),
        "至少需要一个启用的管理员"
    );
    Ok(accounts)
}
fn rate(accounts: &mut Accounts) -> ApiResult<()> {
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
fn token() -> String {
    let mut bytes = [0; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
pub async fn authorize(
    app: &App,
    headers: &HeaderMap,
    change: bool,
    admin: bool,
) -> ApiResult<String> {
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .filter_map(|p| p.trim().split_once('='))
        .find(|(name, _)| *name == "chublog_session")
        .map(|(_, v)| v)
        .unwrap_or("");
    let mut accounts = app.accounts.lock().await;
    let session = accounts
        .sessions
        .get(cookie)
        .filter(|s| s.expires > Instant::now())
        .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "请先登录".into()))?;
    let grant = accounts
        .grants
        .publishers
        .get(&session.username)
        .filter(|g| g.enabled)
        .ok_or_else(|| ApiError(StatusCode::FORBIDDEN, "用户已停用".into()))?;
    if admin && grant.role != Role::Admin {
        return Err(ApiError(StatusCode::FORBIDDEN, "需要管理员权限".into()));
    }
    if change {
        origin(app, headers)?;
        if headers.get("x-csrf-token").and_then(|v| v.to_str().ok()) != Some(session.csrf.as_str())
        {
            return Err(ApiError(StatusCode::FORBIDDEN, "CSRF 校验失败".into()));
        }
    }
    let username = session.username.clone();
    if change {
        let now = Instant::now();
        while accounts
            .changes
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60))
        {
            accounts.changes.pop_front();
        }
        if accounts.changes.len() >= 60 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "修改请求过于频繁".into(),
            ));
        }
        accounts.changes.push_back(now);
    }
    Ok(username)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Login {
    username: String,
    password: String,
}
pub async fn register(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> ApiResult<Json<Value>> {
    origin(&app, &headers)?;
    if !app.registration {
        return Err(ApiError(StatusCode::FORBIDDEN, "注册已关闭".into()));
    }
    username(&input.username).map_err(bad)?;
    password(&input.password).map_err(bad)?;
    {
        let mut accounts = app.accounts.lock().await;
        rate(&mut accounts)?;
        if accounts.credentials.len() >= 1000 {
            return Err(bad("用户配额已满"));
        }
    }
    let _permit = app
        .hash_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "认证服务繁忙".into()))?;
    let hash = hash_password(input.password).await.map_err(internal)?;
    let mut accounts = app.accounts.lock().await;
    if accounts.credentials.contains_key(&input.username) {
        return Err(ApiError(StatusCode::CONFLICT, "用户名不可用".into()));
    }
    if accounts.credentials.len() >= 1000 {
        return Err(bad("用户配额已满"));
    }
    let mut grants = accounts.grants.clone();
    grants.publishers.insert(
        input.username.clone(),
        Grant {
            enabled: true,
            role: Role::Reader,
            prefixes: vec![],
            ssh_key: None,
        },
    );
    atomic(
        &app.ssh_config,
        &serde_json::to_vec_pretty(&grants).map_err(internal)?,
        0o644,
    )
    .await
    .map_err(internal)?;
    accounts.grants = grants;
    accounts
        .credentials
        .insert(input.username.clone(), Credential { hash });
    if let Err(error) = atomic(
        &app.data.join("users.json"),
        &serde_json::to_vec(&accounts.credentials).map_err(internal)?,
        0o600,
    )
    .await
    {
        accounts.credentials.remove(&input.username);
        return Err(internal(error));
    }
    tracing::info!(action = "register", user = %input.username);
    Ok(Json(json!({"username": input.username, "role": "reader"})))
}
pub async fn login(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<Login>,
) -> ApiResult<Response> {
    origin(&app, &headers)?;
    username(&input.username).map_err(bad)?;
    password(&input.password).map_err(bad)?;
    let hash = {
        let mut accounts = app.accounts.lock().await;
        rate(&mut accounts)?;
        accounts
            .credentials
            .get(&input.username)
            .or_else(|| accounts.credentials.values().next())
            .map(|c| c.hash.clone())
            .ok_or_else(|| internal("无管理员"))?
    };
    let _permit = app
        .hash_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "认证服务繁忙".into()))?;
    let verified = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).ok().is_some_and(|h| {
            Argon2::default()
                .verify_password(input.password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .map_err(internal)?;
    let mut accounts = app.accounts.lock().await;
    if !verified
        || !accounts.credentials.contains_key(&input.username)
        || !accounts
            .grants
            .publishers
            .get(&input.username)
            .is_some_and(|g| g.enabled)
    {
        tracing::warn!(action = "login_failed", user = %input.username);
        return Err(ApiError(StatusCode::UNAUTHORIZED, "登录失败".into()));
    }
    let now = Instant::now();
    accounts
        .sessions
        .retain(|_, s| s.expires > now && s.username != input.username);
    if accounts.sessions.len() >= 1000 {
        return Err(bad("会话配额已满"));
    }
    let id = token();
    let csrf = token();
    accounts.sessions.insert(
        id.clone(),
        Session {
            username: input.username.clone(),
            csrf: csrf.clone(),
            expires: now + Duration::from_secs(3600),
        },
    );
    let cookie = format!(
        "chublog_session={id}; Path=/; HttpOnly; SameSite=Strict; Max-Age=3600{}",
        if app.secure { "; Secure" } else { "" }
    );
    let mut response = Json(json!({"csrf": csrf, "username": input.username, "role": accounts.grants.publishers[&input.username].role})).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(internal)?,
    );
    tracing::info!(action = "login", user = %input.username);
    Ok(response)
}
pub async fn me(State(app): State<Arc<App>>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let name = authorize(&app, &headers, false, false).await?;
    let accounts = app.accounts.lock().await;
    let csrf = accounts
        .sessions
        .values()
        .find(|s| s.username == name)
        .map(|s| &s.csrf);
    Ok(Json(
        json!({"username": name, "permissions": accounts.grants.publishers[&name], "csrf": csrf}),
    ))
}
pub async fn logout(State(app): State<Arc<App>>, headers: HeaderMap) -> ApiResult<Response> {
    let name = authorize(&app, &headers, true, false).await?;
    app.accounts
        .lock()
        .await
        .sessions
        .retain(|_, s| s.username != name);
    let mut response = StatusCode::NO_CONTENT.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "chublog_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
            if app.secure { "; Secure" } else { "" }
        ))
        .map_err(internal)?,
    );
    Ok(response)
}
pub fn public_key(value: &str) -> Result<String> {
    ensure!(
        value.len() <= 4096 && !value.contains(['\n', '\r']),
        "公钥格式无效"
    );
    let key = ssh_key::PublicKey::from_openssh(value)
        .context("SSH 公钥格式无效，禁止 authorized_keys 选项")?;
    match key.key_data() {
        ssh_key::public::KeyData::Ed25519(_) => {}
        ssh_key::public::KeyData::Rsa(rsa) => {
            let modulus = rsa.n.as_positive_bytes().context("RSA 模数必须为正数")?;
            let first = modulus.first().context("RSA 模数为空")?;
            let bits = modulus.len() * 8 - first.leading_zeros() as usize;
            ensure!(
                (2048..=8192).contains(&bits) && modulus.last().is_some_and(|b| b & 1 == 1),
                "RSA 模数必须为 2048–8192 位正奇数"
            );
            let exponent = rsa.e.as_positive_bytes().context("RSA 指数必须为正数")?;
            ensure!(exponent.len() <= 4, "RSA 指数过大");
            let exponent = exponent.iter().fold(0_u32, |n, b| (n << 8) | u32::from(*b));
            ensure!(
                exponent >= 3 && exponent % 2 == 1,
                "RSA 指数必须为至少 3 的奇数"
            );
        }
        _ => anyhow::bail!("仅支持 Ed25519 或 RSA 公钥"),
    }
    // Drop comments and options: the stored value is exactly the parsed public key.
    Ok(format!(
        "{} {}",
        key.algorithm().as_str(),
        key.to_bytes()
            .map(|bytes| base64::engine::general_purpose::STANDARD.encode(bytes))?
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyInput {
    public_key: String,
}
pub async fn set_key(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<KeyInput>,
) -> ApiResult<Json<Value>> {
    let name = authorize(&app, &headers, true, false).await?;
    let key = public_key(&input.public_key).map_err(bad)?;
    let mut accounts = app.accounts.lock().await;
    if accounts
        .grants
        .publishers
        .iter()
        .any(|(user, grant)| user != &name && grant.ssh_key.as_deref() == Some(&key))
    {
        return Err(bad("公钥已被其他用户绑定"));
    }
    let mut grants = accounts.grants.clone();
    grants
        .publishers
        .get_mut(&name)
        .ok_or_else(|| bad("用户不存在"))?
        .ssh_key = Some(key);
    atomic(
        &app.ssh_config,
        &serde_json::to_vec_pretty(&grants).map_err(internal)?,
        0o644,
    )
    .await
    .map_err(internal)?;
    accounts.grants = grants;
    tracing::info!(action = "ssh_key_changed", user = %name);
    Ok(Json(
        json!({"status": "saved", "message": "密钥已保存，发布仍需管理员授权"}),
    ))
}
pub async fn users(State(app): State<Arc<App>>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    authorize(&app, &headers, false, true).await?;
    Ok(Json(json!(app.accounts.lock().await.grants.publishers)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantInput {
    enabled: bool,
    role: Role,
    prefixes: Vec<String>,
}
pub async fn grant(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(input): Json<GrantInput>,
) -> ApiResult<Json<Value>> {
    let actor = authorize(&app, &headers, true, true).await?;
    ensure_prefixes(&input.prefixes).map_err(bad)?;
    if input.role == Role::Publisher && input.prefixes.is_empty() {
        return Err(bad("发布者必须分配文章前缀"));
    }
    let mut accounts = app.accounts.lock().await;
    if !accounts
        .grants
        .publishers
        .get(&actor)
        .is_some_and(|g| g.enabled && g.role == Role::Admin)
    {
        return Err(ApiError(StatusCode::FORBIDDEN, "管理员权限已失效".into()));
    }
    if !accounts.credentials.contains_key(&name) {
        return Err(ApiError(StatusCode::NOT_FOUND, "用户不存在".into()));
    }
    if actor == name && (input.role != Role::Admin || !input.enabled) {
        return Err(bad("不能取消自己的管理权限或停用自己"));
    }
    for (other, grant) in &accounts.grants.publishers {
        if other == &name || !grant.enabled || grant.role == Role::Reader {
            continue;
        }
        if input.enabled
            && input.role != Role::Reader
            && input.prefixes.iter().any(|a| {
                grant
                    .prefixes
                    .iter()
                    .any(|b| a.starts_with(b) || b.starts_with(a))
            })
        {
            return Err(bad("发布前缀与其他用户重叠"));
        }
    }
    let mut grants = accounts.grants.clone();
    let entry = grants
        .publishers
        .get_mut(&name)
        .ok_or_else(|| bad("用户授权缺失"))?;
    entry.enabled = input.enabled;
    entry.role = input.role;
    entry.prefixes = if input.role == Role::Reader {
        vec![]
    } else {
        input.prefixes
    };
    atomic(
        &app.ssh_config,
        &serde_json::to_vec_pretty(&grants).map_err(internal)?,
        0o644,
    )
    .await
    .map_err(internal)?;
    accounts.grants = grants;
    accounts
        .sessions
        .retain(|_, s| s.username != name || s.username == actor);
    tracing::info!(action = "permission_changed", user = %name, administrator = %actor);
    Ok(Json(json!({"status": "saved"})))
}
fn ensure_prefixes(prefixes: &[String]) -> Result<()> {
    ensure!(
        prefixes.len() <= 8
            && prefixes.iter().all(|p| (2..=64).contains(&p.len())
                && p.ends_with('-')
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')),
        "前缀必须为 2–64 位小写英文、数字、短横线且以短横线结尾，最多 8 个"
    );
    Ok(())
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
    let name = authorize(&app, &headers, true, false).await?;
    password(&input.old_password).map_err(bad)?;
    password(&input.new_password).map_err(bad)?;
    let _permit = app
        .hash_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "认证服务繁忙".into()))?;
    let previous = app
        .accounts
        .lock()
        .await
        .credentials
        .get(&name)
        .ok_or_else(|| bad("用户不存在"))?
        .hash
        .clone();
    let hash = previous.clone();
    let verified = tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).ok().is_some_and(|h| {
            Argon2::default()
                .verify_password(input.old_password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .map_err(internal)?;
    if !verified {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "旧口令错误".into()));
    }
    let hash = hash_password(input.new_password).await.map_err(internal)?;
    let mut accounts = app.accounts.lock().await;
    if !accounts
        .grants
        .publishers
        .get(&name)
        .is_some_and(|g| g.enabled)
    {
        return Err(ApiError(StatusCode::FORBIDDEN, "用户已停用".into()));
    }
    let credential = accounts
        .credentials
        .get_mut(&name)
        .ok_or_else(|| bad("用户不存在"))?;
    if credential.hash != previous {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "口令已变更，请重新登录".into(),
        ));
    }
    credential.hash = hash;
    if let Err(error) = atomic(
        &app.data.join("users.json"),
        &serde_json::to_vec(&accounts.credentials).map_err(internal)?,
        0o600,
    )
    .await
    {
        accounts.credentials.get_mut(&name).unwrap().hash = previous;
        return Err(internal(error));
    }
    accounts.sessions.retain(|_, s| s.username != name);
    tracing::info!(action = "password_changed", user = %name);
    Ok(Json(
        json!({"status": "saved", "message": "请使用新口令重新登录"}),
    ))
}
