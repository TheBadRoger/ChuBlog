use crate::{
    ApiError, ApiResult, App, accounts,
    accounts_db::{AccessSource, Level, User},
    bad,
    content::{ArticlePolicy, Hidden, Post, Review, Snapshot, Status, Visibility},
    internal,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::{Arc, atomic::Ordering},
};

pub fn metadata(
    snapshot: &Snapshot,
    post: &Post,
    authors: &std::collections::BTreeMap<String, crate::accounts_db::AuthorDisplay>,
) -> Value {
    let mut meta = crate::display_metadata(&post.meta, authors);
    let policy = snapshot
        .policies
        .get(&post.meta.slug)
        .cloned()
        .unwrap_or_default();
    meta["featured"] = json!(policy.featured);
    meta["visibility"] = json!(policy.visibility);
    meta["hidden"] = json!(policy.hidden);
    meta["review"] = json!(policy.review);
    meta["deleted"] = json!(policy.review == Review::Blocked);
    meta["password_required"] = json!(policy.password_hash.is_some());
    meta
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockReason {
    Login,
    Password,
    Denied,
}
impl LockReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Password => "password",
            Self::Denied => "denied",
        }
    }
    fn message(self) -> &'static str {
        match self {
            Self::Login => "请先登录后查看",
            Self::Password => "需要口令才可访问",
            Self::Denied => "无权访问",
        }
    }
}

pub struct Locked {
    pub status: StatusCode,
    pub reason: LockReason,
    pub meta: Option<Value>,
}
impl Locked {
    pub fn response(&self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.reason.message(),
                "locked": true,
                "reason": self.reason.as_str(),
                "meta": self.meta,
            })),
        )
            .into_response()
    }
}

pub enum Access {
    Allowed,
    Locked(Locked),
}

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "文章不存在".into())
}

fn removed() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "文章已删除".into())
}

async fn viewer(app: &App, headers: &HeaderMap) -> ApiResult<Option<User>> {
    if !headers.contains_key(axum::http::header::COOKIE) {
        return Ok(None);
    }
    match accounts::authorize(app, headers, false, false).await {
        Ok(user) => Ok(Some(user)),
        Err(error) if error.0 == StatusCode::UNAUTHORIZED => Ok(None),
        Err(error) => Err(error),
    }
}

pub async fn access_set(app: &App, user_id: i64) -> ApiResult<BTreeSet<String>> {
    let store = app.accounts.lock().await.store.clone();
    store
        .article_access_set(user_id)
        .await
        .map_err(accounts::database_error)
}

async fn has_access(app: &App, user_id: i64, key: &str, source: AccessSource) -> ApiResult<bool> {
    let store = app.accounts.lock().await.store.clone();
    store
        .has_article_access(user_id, key, source)
        .await
        .map_err(accounts::database_error)
}

/// Rewrite the queryable database projection after a policy change.
pub async fn sync_policies(app: &App, snapshot: &Snapshot) -> ApiResult<()> {
    app.policy_projection_pending.store(true, Ordering::Relaxed);
    let rows = snapshot
        .policies
        .iter()
        .map(|(article, policy)| crate::accounts_db::ArticlePolicyRow {
            article: article.clone(),
            visibility: policy.visibility.as_str().into(),
            hidden: policy.hidden.as_str().into(),
            review: policy.review.as_str().into(),
            blocked_hash: policy.blocked_hash.clone(),
            password_hash: policy.password_hash.clone(),
        })
        .collect();
    let store = app.accounts.lock().await.store.clone();
    store
        .replace_article_policies(rows)
        .await
        .map_err(accounts::database_error)?;
    app.policy_projection_pending
        .store(false, Ordering::Relaxed);
    Ok(())
}

pub async fn retry_policy_projection(app: &App) -> ApiResult<()> {
    let _guard = app.article_changes.lock().await;
    if app.policy_projection_pending.load(Ordering::Relaxed) {
        let snapshot = app.snapshot.read().await.clone();
        sync_policies(app, &snapshot).await?;
    }
    Ok(())
}

fn locked(
    snapshot: &Snapshot,
    post: &Post,
    policy: &ArticlePolicy,
    reason: LockReason,
    status: StatusCode,
    authors: &std::collections::BTreeMap<String, crate::accounts_db::AuthorDisplay>,
) -> Access {
    Access::Locked(Locked {
        status,
        reason,
        meta: (policy.hidden == Hidden::Semi).then(|| metadata(snapshot, post, authors)),
    })
}

pub async fn check(
    app: &App,
    headers: &HeaderMap,
    snapshot: &Snapshot,
    post: &Post,
) -> ApiResult<Access> {
    let policy = snapshot
        .policies
        .get(&post.meta.slug)
        .cloned()
        .unwrap_or_default();
    if policy.review == Review::Open
        && post.meta.status == Status::Published
        && policy.visibility == Visibility::Default
    {
        return Ok(Access::Allowed);
    }
    let user = viewer(app, headers).await?;
    let privileged = user
        .as_ref()
        .is_some_and(|user| user.level == Level::Master || user.username == post.author);
    if policy.review == Review::Blocked {
        return if privileged {
            Ok(Access::Allowed)
        } else {
            Err(removed())
        };
    }
    if post.meta.status != Status::Published {
        return if privileged {
            Ok(Access::Allowed)
        } else {
            Err(not_found())
        };
    }
    if privileged {
        return Ok(Access::Allowed);
    }
    let authors = app.authors.read().await;
    let result = match policy.visibility {
        Visibility::Default => Access::Allowed,
        Visibility::Secret => match user {
            Some(_) => Access::Allowed,
            None => locked(
                snapshot,
                post,
                &policy,
                LockReason::Login,
                StatusCode::UNAUTHORIZED,
                &authors,
            ),
        },
        Visibility::Confidential => {
            let Some(user) = user else {
                return Ok(locked(
                    snapshot,
                    post,
                    &policy,
                    LockReason::Login,
                    StatusCode::UNAUTHORIZED,
                    &authors,
                ));
            };
            let allowed = has_access(app, user.id, &post.meta.slug, AccessSource::Password).await?;
            if allowed {
                Access::Allowed
            } else if policy.password_hash.is_some() {
                locked(
                    snapshot,
                    post,
                    &policy,
                    LockReason::Password,
                    StatusCode::FORBIDDEN,
                    &authors,
                )
            } else {
                locked(
                    snapshot,
                    post,
                    &policy,
                    LockReason::Denied,
                    StatusCode::FORBIDDEN,
                    &authors,
                )
            }
        }
        Visibility::Top => {
            let allowed = match &user {
                Some(user) => {
                    has_access(app, user.id, &post.meta.slug, AccessSource::Granted).await?
                }
                None => false,
            };
            if allowed {
                Access::Allowed
            } else {
                locked(
                    snapshot,
                    post,
                    &policy,
                    LockReason::Denied,
                    StatusCode::FORBIDDEN,
                    &authors,
                )
            }
        }
    };
    Ok(result)
}

/// Metadata-list visibility. Authors and administrators always see their own articles.
pub fn listed(
    snapshot: &Snapshot,
    post: &Post,
    viewer: Option<&User>,
    access: &BTreeSet<String>,
) -> bool {
    if post.meta.status != Status::Published {
        return false;
    }
    let policy = snapshot
        .policies
        .get(&post.meta.slug)
        .cloned()
        .unwrap_or_default();
    let privileged =
        viewer.is_some_and(|user| user.level == Level::Master || user.username == post.author);
    if policy.review == Review::Blocked {
        return privileged;
    }
    if privileged {
        return true;
    }
    match policy.visibility {
        Visibility::Default => true,
        Visibility::Secret => viewer.is_some() || policy.hidden == Hidden::Semi,
        Visibility::Confidential => false,
        Visibility::Top => access.contains(&post.meta.slug) || policy.hidden == Hidden::Semi,
    }
}

pub async fn list(State(app): State<Arc<App>>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    accounts::authorize(&app, &headers, false, true).await?;
    let snapshot = app.snapshot.read().await;
    let authors = app.authors.read().await;
    Ok(Json(
        json!({"posts": snapshot.posts.values().map(|p| metadata(&snapshot, p, &authors)).collect::<Vec<_>>()}),
    ))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    featured: Option<bool>,
    visibility: Option<Visibility>,
    hidden: Option<Hidden>,
    // Omitted preserves the hash; an empty string clears it.
    password: Option<String>,
    review: Option<Review>,
}

async fn save(app: &App, snapshot: Snapshot) -> ApiResult<()> {
    accounts::atomic(
        &app.data.join("collection.json"),
        &serde_json::to_vec(&snapshot).map_err(internal)?,
        0o600,
    )
    .await
    .map_err(internal)?;
    // The private collection is authoritative. Regenerate public exports before acknowledging success.
    app.policy_projection_pending.store(true, Ordering::Relaxed);
    *app.snapshot.write().await = snapshot.clone();
    *app.indexed.write().await = None;
    crate::content::persist(&app.data, &snapshot)
        .await
        .map_err(internal)?;
    sync_policies(app, &snapshot).await?;
    Ok(())
}

async fn update_inner(
    app: &App,
    key: &str,
    headers: &HeaderMap,
    input: Settings,
    admin_route: bool,
) -> ApiResult<()> {
    let actor = accounts::authorize(app, headers, true, admin_route).await?;
    let _guard = app.article_changes.lock().await;
    let mut snapshot = app.snapshot.read().await.clone();
    let post = snapshot.posts.get(key).cloned().ok_or_else(not_found)?;
    let is_author = post.author == actor.username;
    let is_admin = actor.level == Level::Master;
    if !admin_route && !is_author {
        return Err(ApiError(StatusCode::FORBIDDEN, "仅文章作者可修改".into()));
    }
    if !is_admin && input.featured.is_some() {
        return Err(ApiError(StatusCode::FORBIDDEN, "仅管理员可设置精华".into()));
    }
    if !is_admin && input.review.is_some() {
        return Err(ApiError(StatusCode::FORBIDDEN, "仅管理员可审核文章".into()));
    }
    if input.hidden.is_some() && !is_author {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "仅文章作者可设置隐藏等级".into(),
        ));
    }
    let previous = snapshot.policies.get(key).cloned().unwrap_or_default();
    let password_supplied = input.password.is_some();
    let mut policy = previous.clone();
    if let Some(featured) = input.featured {
        policy.featured = featured;
    }
    if let Some(visibility) = input.visibility {
        policy.visibility = visibility;
    }
    let hidden_supplied = input.hidden.is_some();
    if let Some(hidden) = input.hidden {
        policy.hidden = hidden;
    }
    if let Some(review) = input.review {
        if review == Review::Blocked && policy.review != Review::Blocked {
            let repo = app.data.join("content.git");
            policy.blocked_hash = Some(
                crate::content::post_hash(&repo, &post)
                    .await
                    .map_err(internal)?,
            );
        }
        if review == Review::Open {
            policy.blocked_hash = None;
        }
        policy.review = review;
    }
    if let Some(password) = input.password {
        if password.is_empty() {
            policy.password_hash = None;
        } else {
            accounts::password(&password).map_err(bad)?;
            if policy.visibility != Visibility::Confidential {
                return Err(bad("查阅口令仅用于机密文章"));
            }
            let _slot = app
                .hash_slots
                .clone()
                .try_acquire_owned()
                .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "请稍后重试".into()))?;
            policy.password_hash = Some(accounts::hash_password(password).await.map_err(internal)?);
        }
    }
    if policy.visibility != Visibility::Confidential {
        policy.password_hash = None;
    }
    if policy.visibility == Visibility::Confidential && policy.hidden != Hidden::Full {
        if hidden_supplied {
            return Err(bad("机密文章只能设置全隐藏"));
        }
        policy.hidden = Hidden::Full;
    }
    if policy.visibility == Visibility::Confidential && policy.password_hash.is_none() {
        return Err(bad("机密文章必须设置查阅口令"));
    }
    let password_cleared =
        policy.visibility != Visibility::Confidential && previous.password_hash.is_some();
    let left_top = previous.visibility == Visibility::Top && policy.visibility != Visibility::Top;
    if password_supplied || password_cleared || left_top {
        let store = app.accounts.lock().await.store.clone();
        if password_supplied || password_cleared {
            store
                .revoke_password_access(key)
                .await
                .map_err(accounts::database_error)?;
        }
        if left_top {
            store
                .revoke_granted_access(key)
                .await
                .map_err(accounts::database_error)?;
        }
    }
    snapshot.policies.insert(key.to_owned(), policy);
    save(app, snapshot).await?;
    Ok(())
}

pub async fn update_admin(
    State(app): State<Arc<App>>,
    Path(key): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Settings>,
) -> ApiResult<Json<Value>> {
    update_inner(&app, &key, &headers, input, true).await?;
    Ok(Json(json!({"ok":true})))
}

pub async fn update_author(
    State(app): State<Arc<App>>,
    Path(key): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Settings>,
) -> ApiResult<Json<Value>> {
    update_inner(&app, &key, &headers, input, false).await?;
    Ok(Json(json!({"ok":true})))
}

pub async fn delete(
    State(app): State<Arc<App>>,
    Path(key): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    let input = Settings {
        review: Some(Review::Blocked),
        ..Default::default()
    };
    update_inner(&app, &key, &headers, input, true).await?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unlock {
    password: String,
}
pub async fn unlock(
    State(app): State<Arc<App>>,
    Path(key): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Unlock>,
) -> ApiResult<Json<Value>> {
    let user = accounts::authorize(&app, &headers, true, false).await?;
    accounts::password(&input.password).map_err(bad)?;
    let snapshot = app.snapshot.read().await;
    let post = snapshot.posts.get(&key).ok_or_else(not_found)?;
    let policy = snapshot.policies.get(&key).cloned().unwrap_or_default();
    if policy.review == Review::Blocked
        || policy.visibility != Visibility::Confidential
        || post.meta.status != Status::Published
    {
        return Err(not_found());
    }
    let hash = policy.password_hash.ok_or_else(not_found)?;
    accounts::unlock_article(&app, user.id, &key, &hash, input.password).await?;
    Ok(Json(json!({"ok":true})))
}

async fn access_view(app: &App, key: &str) -> ApiResult<Value> {
    let store = app.accounts.lock().await.store.clone();
    let rows = store
        .article_access_users(key)
        .await
        .map_err(accounts::database_error)?;
    Ok(json!({
        "users": rows.into_iter().map(|row| json!({
            "username": row.user.username,
            "nickname": row.user.nickname,
            "source": row.source.as_str(),
        })).collect::<Vec<_>>()
    }))
}

async fn require_manager(app: &App, headers: &HeaderMap, key: &str) -> ApiResult<()> {
    let user = accounts::authorize(app, headers, false, false).await?;
    let snapshot = app.snapshot.read().await;
    let post = snapshot.posts.get(key).ok_or_else(not_found)?;
    if user.level != Level::Master && user.username != post.author {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "仅作者或管理员可管理".into(),
        ));
    }
    Ok(())
}

pub async fn access_list(
    State(app): State<Arc<App>>,
    Path(key): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Json<Value>> {
    require_manager(&app, &headers, &key).await?;
    Ok(Json(access_view(&app, &key).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessInput {
    username: String,
    allowed: bool,
}
pub async fn access_update(
    State(app): State<Arc<App>>,
    Path(key): Path<String>,
    headers: HeaderMap,
    Json(input): Json<AccessInput>,
) -> ApiResult<Json<Value>> {
    let actor = accounts::authorize(&app, &headers, true, false).await?;
    let _guard = app.article_changes.lock().await;
    let snapshot = app.snapshot.read().await;
    let post = snapshot.posts.get(&key).ok_or_else(not_found)?;
    if actor.level != Level::Master && actor.username != post.author {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "仅作者或管理员可管理".into(),
        ));
    }
    let policy = snapshot.policies.get(&key).cloned().unwrap_or_default();
    if policy.visibility != Visibility::Top {
        return Err(bad("仅绝密文章可指定访问用户"));
    }
    drop(snapshot);
    let store = app.accounts.lock().await.store.clone();
    let target = store
        .user(&input.username)
        .await
        .map_err(accounts::database_error)?
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "用户不存在".into()))?;
    if target.level == Level::Banned {
        return Err(bad("不能授权已被封禁的用户"));
    }
    if input.allowed {
        store
            .grant_article_access(target.id, &key, AccessSource::Granted)
            .await
            .map_err(accounts::database_error)?;
    } else {
        store
            .revoke_article_access(target.id, &key)
            .await
            .map_err(accounts::database_error)?;
    }
    Ok(Json(access_view(&app, &key).await?))
}

/// Reopen blocked articles whose published content changed after the block.
pub async fn reopen_modified(app: &App, snapshot: &mut Snapshot) -> anyhow::Result<bool> {
    let repo = app.data.join("content.git");
    let mut changed = false;
    let keys: Vec<_> = snapshot.policies.keys().cloned().collect();
    for key in keys {
        let Some(policy) = snapshot.policies.get(&key) else {
            continue;
        };
        let Some(blocked_hash) = policy.blocked_hash.clone() else {
            continue;
        };
        if policy.review != Review::Blocked {
            continue;
        }
        let Some(post) = snapshot.posts.get(&key) else {
            continue;
        };
        if post.meta.status != Status::Published {
            continue;
        }
        let current = crate::content::post_hash(&repo, post).await?;
        if current != blocked_hash
            && let Some(policy) = snapshot.policies.get_mut(&key)
        {
            policy.review = Review::Open;
            policy.blocked_hash = None;
            changed = true;
        }
    }
    Ok(changed)
}
