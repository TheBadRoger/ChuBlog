mod accounts;
mod content;
mod index;
mod ssh;

use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, patch, post, put},
};
use base64::Engine;
use content::{Snapshot, Status};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, RwLock, Semaphore};
use tracing::{info, warn};

type ApiResult<T> = std::result::Result<T, ApiError>;
struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}
fn bad(error: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, error.to_string())
}
fn internal(error: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %error, "operation failed");
    ApiError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "服务器操作失败，请联系管理员".into(),
    )
}
struct App {
    data: PathBuf,
    ssh_config: PathBuf,
    origin: String,
    secure: bool,
    registration: bool,
    accounts: Mutex<accounts::Accounts>,
    hash_slots: Arc<Semaphore>,
    snapshot: RwLock<Snapshot>,
    indexed: RwLock<Option<String>>,
    views: AtomicU64,
}
fn origin(app: &App, headers: &HeaderMap) -> ApiResult<()> {
    if headers.get("origin").and_then(|h| h.to_str().ok()) != Some(app.origin.as_str()) {
        return Err(ApiError(StatusCode::FORBIDDEN, "Origin 校验失败".into()));
    }
    Ok(())
}
async fn posts(State(app): State<Arc<App>>) -> Json<Value> {
    let snapshot = app.snapshot.read().await;
    let mut posts: Vec<_> = snapshot
        .posts
        .values()
        .filter(|p| p.meta.status == Status::Published)
        .map(|p| &p.meta)
        .collect();
    posts.sort_by(|a, b| b.date.cmp(&a.date).then(a.slug.cmp(&b.slug)));
    Json(json!({"commit": snapshot.commit, "posts": posts}))
}
async fn public_post(
    State(app): State<Arc<App>>,
    Path(key): Path<String>,
) -> ApiResult<Json<Value>> {
    let snapshot = app.snapshot.read().await;
    let post = snapshot
        .posts
        .get(&key)
        .filter(|p| p.meta.status == Status::Published)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "文章不存在".into()))?;
    // Relaxed is sufficient: this independent counter does not synchronize other state.
    app.views.fetch_add(1, Ordering::Relaxed);
    Ok(Json(
        json!({"meta": post.meta, "markdown": post.markdown, "assets": post.assets.iter().map(|a| json!({"name": a.name, "url": format!("/api/assets/{key}/{}", a.name)})).collect::<Vec<_>>()}),
    ))
}
async fn asset(
    State(app): State<Arc<App>>,
    Path((key, name)): Path<(String, String)>,
) -> ApiResult<Response> {
    let snapshot = app.snapshot.read().await;
    let asset = snapshot
        .posts
        .get(&key)
        .filter(|p| p.meta.status == Status::Published)
        .and_then(|p| p.assets.iter().find(|a| a.name == name))
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "资源不存在".into()))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&asset.data)
        .map_err(internal)?;
    let mime = match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => return Err(internal("invalid stored image")),
    };
    Ok(([(header::CONTENT_TYPE, mime)], Body::from(bytes)).into_response())
}
async fn stats(State(app): State<Arc<App>>) -> Json<Value> {
    let snapshot = app.snapshot.read().await;
    let posts: Vec<_> = snapshot
        .posts
        .values()
        .filter(|p| p.meta.status == Status::Published)
        .collect();
    let tags: std::collections::BTreeSet<_> = posts.iter().flat_map(|p| &p.meta.tags).collect();
    Json(
        json!({"published_posts": posts.len(), "tags": tags.len(), "markdown_bytes": posts.iter().map(|p| p.markdown.len()).sum::<usize>(), "article_requests_since_start": app.views.load(Ordering::Relaxed)}),
    )
}
async fn admin_status(State(app): State<Arc<App>>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    accounts::authorize(&app, &headers, false, true).await?;
    let snapshot = app.snapshot.read().await;
    Ok(Json(
        json!({"commit": snapshot.commit, "total_posts": snapshot.posts.len(), "drafts": snapshot.posts.values().filter(|p| p.meta.status == Status::Draft).count(), "indexed_commit": *app.indexed.read().await}),
    ))
}
async fn sync_index(app: &App, snapshot: &Snapshot) {
    match tokio::time::timeout(Duration::from_secs(30), index::rebuild(snapshot)).await {
        Ok(Ok(())) => {
            *app.indexed.write().await = Some(snapshot.commit.clone());
            info!(action = "index_synced", commit = %snapshot.commit);
        }
        Ok(Err(error)) => warn!(action = "index_pending", error = %error),
        Err(_) => warn!(action = "index_pending", "rebuild timeout"),
    }
}
fn router(app: Arc<App>) -> Router {
    Router::new()
        .route(
            "/api/health",
            get(|| async { Json(json!({"status": "ok"})) }),
        )
        .route("/api/posts", get(posts))
        .route("/api/posts/{slug}", get(public_post))
        .route("/api/assets/{slug}/{name}", get(asset))
        .route("/api/stats", get(stats))
        .route("/api/auth/register", post(accounts::register))
        .route("/api/auth/login", post(accounts::login))
        .route("/api/auth/logout", post(accounts::logout))
        .route("/api/account", get(accounts::me))
        .route("/api/account/ssh-key", put(accounts::set_key))
        .route("/api/account/password", put(accounts::change_password))
        .route("/api/admin/users", get(accounts::users))
        .route("/api/admin/users/{username}", patch(accounts::grant))
        .route("/api/admin/status", get(admin_status))
        .layer(DefaultBodyLimit::max(8 * 1024))
        .layer(axum::middleware::from_fn(
            |request: axum::extract::Request, next: axum::middleware::Next| async move {
                let mut response = next.run(request).await;
                for (key, value) in [
                    (header::CACHE_CONTROL, "no-store"),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                    (
                        header::CONTENT_SECURITY_POLICY,
                        "default-src 'none'; frame-ancestors 'none'",
                    ),
                    (header::REFERRER_POLICY, "no-referrer"),
                ] {
                    response
                        .headers_mut()
                        .insert(key, HeaderValue::from_static(value));
                }
                response
            },
        ))
        .with_state(app)
}
async fn refresh(app: &App, repo: &std::path::Path, source: &std::path::Path) -> Result<()> {
    // Recover a previously committed ref even if snapshot publication was interrupted.
    let mut current = app.snapshot.read().await.clone();
    let head =
        String::from_utf8(content::git(repo, &["rev-parse", "refs/heads/main"], None).await?)?;
    if current.commit != head.trim() {
        current = content::scan(repo).await?;
    }
    let snapshot = ssh::import(repo, source, &current).await?;
    if snapshot.commit != app.snapshot.read().await.commit {
        content::persist(&app.data, &snapshot).await?;
        info!(action = "published", commit = %snapshot.commit);
        *app.snapshot.write().await = snapshot;
    }
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("ssh-gate") => {
            return ssh::gate(&std::env::args().nth(2).context("缺少发布者身份")?).await;
        }
        Some("pre-receive") => return ssh::pre_receive().await,
        Some("authorized-keys") => return ssh::authorized_keys(),
        Some("provision-db") => return index::provision().await,
        Some("hash-password") => {
            use std::io::Read;
            let mut value = String::new();
            std::io::stdin().take(1025).read_to_string(&mut value)?;
            println!(
                "{}",
                accounts::hash_password(value.trim_end_matches(['\r', '\n']).into()).await?
            );
            return Ok(());
        }
        None | Some("rebuild-index") => {}
        Some(_) => anyhow::bail!("未知命令"),
    }
    let data = PathBuf::from(std::env::var("DATA_DIR").unwrap_or_else(|_| "data".into()));
    tokio::fs::create_dir_all(&data).await?;
    let data = tokio::fs::canonicalize(data).await?;
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data.join("writer.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock_file).context("内容目录已被另一个 API 进程使用")?;
    let repo = content::init(&data).await?;
    let snapshot = content::scan(&repo).await?;
    content::persist(&data, &snapshot).await?;
    if command.as_deref() == Some("rebuild-index") {
        index::rebuild(&snapshot).await?;
        return Ok(());
    }
    let origin = std::env::var("PUBLIC_ORIGIN").context("必须设置 PUBLIC_ORIGIN")?;
    let parsed: axum::http::Uri = origin.parse().context("PUBLIC_ORIGIN 无效")?;
    let secure = parsed.scheme_str() == Some("https");
    ensure!(
        secure
            || (parsed.scheme_str() == Some("http")
                && matches!(parsed.host(), Some("localhost" | "127.0.0.1"))),
        "生产环境必须使用 HTTPS"
    );
    ensure!(
        !origin.ends_with('/')
            && parsed.path() == "/"
            && parsed.query().is_none()
            && parsed
                .authority()
                .is_some_and(|a| !a.as_str().contains('@')),
        "PUBLIC_ORIGIN 必须为不带末尾斜杠的站点 origin"
    );
    let source = PathBuf::from(
        std::env::var("INGRESS_REPO").unwrap_or_else(|_| "/srv/ingress/content.git".into()),
    );
    ensure!(source.is_absolute(), "INGRESS_REPO 必须是绝对路径");
    let ssh_config = PathBuf::from(
        std::env::var("CHUBLOG_SSH_CONFIG")
            .unwrap_or_else(|_| data.join("publishers.json").to_string_lossy().into_owned()),
    );
    let accounts = accounts::load(&data, &ssh_config, source.clone()).await?;
    ensure!(
        accounts.grants.repository == source,
        "授权配置仓库与 INGRESS_REPO 不一致"
    );
    let app = Arc::new(App {
        data,
        ssh_config,
        origin,
        secure,
        registration: std::env::var("ALLOW_REGISTRATION").as_deref() == Ok("true"),
        accounts: Mutex::new(accounts),
        hash_slots: Arc::new(Semaphore::new(2)),
        snapshot: RwLock::new(snapshot),
        indexed: RwLock::new(None),
        views: AtomicU64::new(0),
    });
    let worker_app = app.clone();
    let worker = tokio::spawn(async move {
        let mut ticks = 0;
        loop {
            if let Err(error) = refresh(&worker_app, &repo, &source).await {
                warn!(action = "import_pending", error = %error);
            }
            if ticks % 30 == 0 {
                let snapshot = worker_app.snapshot.read().await.clone();
                if worker_app.indexed.read().await.as_deref() != Some(snapshot.commit.as_str()) {
                    sync_index(&worker_app, &snapshot).await;
                } else if !matches!(
                    tokio::time::timeout(Duration::from_secs(10), index::consistent(&snapshot))
                        .await,
                    Ok(Ok(true))
                ) {
                    *worker_app.indexed.write().await = None;
                    sync_index(&worker_app, &snapshot).await;
                }
            }
            ticks += 1;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    let bind = std::env::var("API_BIND").unwrap_or_else(|_| "127.0.0.1:3001".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    info!(%bind, "ChuBlog API ready");
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    axum::serve(listener, router(app))
        .with_graceful_shutdown(async move {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
        })
        .await?;
    worker.abort();
    let _ = worker.await;
    drop(lock_file);
    Ok(())
}

#[cfg(test)]
mod tests;
