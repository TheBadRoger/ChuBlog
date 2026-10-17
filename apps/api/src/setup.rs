use crate::{ApiError, ApiResult, App, accounts, bad, origin};
use anyhow::{Context, ensure};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

#[derive(Serialize)]
pub struct Site {
    pub name: String,
    pub description: String,
}
impl Site {
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            !self.name.is_empty()
                && self.name.len() <= 120
                && !self.name.chars().any(char::is_control),
            "网站名称需要 1–120 字节，不能包含控制字符"
        );
        ensure!(
            self.description.len() <= 1000 && !self.description.chars().any(char::is_control),
            "网站简介最多 1000 字节，不能包含控制字符"
        );
        Ok(())
    }
}

pub async fn load_token(data: &Path) -> anyhow::Result<String> {
    let path = data.join("setup-token");
    match tokio::fs::read_to_string(&path).await {
        Ok(token) => {
            ensure!(
                token.len() == 43
                    && token
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
                "安装密钥文件无效"
            );
            Ok(token)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let token = accounts::token();
            accounts::atomic(&path, token.as_bytes(), 0o600).await?;
            Ok(token)
        }
        Err(error) => Err(error).context("无法读取安装密钥"),
    }
}
fn matches_token(expected: &str, supplied: &str) -> bool {
    expected.len() == supplied.len()
        && expected
            .bytes()
            .zip(supplied.bytes())
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}

pub async fn site(State(app): State<Arc<App>>) -> ApiResult<Json<Value>> {
    let mut cached = app.site.lock().await;
    if let Some(state) = cached.as_ref() {
        return Ok(Json(state.clone()));
    }
    let store = accounts::store(&app).await;
    let site = store.site().await.map_err(accounts::database_error)?;
    let owner = if site.is_some() {
        Some(
            store
                .owner()
                .await
                .map_err(accounts::database_error)?
                .ok_or_else(|| {
                    ApiError(StatusCode::SERVICE_UNAVAILABLE, "管理员账户不存在".into())
                })?
                .username,
        )
    } else {
        None
    };
    let state = json!({"installed":site.is_some(),"site":site,"owner":owner});
    if site.is_some() {
        *cached = Some(state.clone());
    }
    Ok(Json(state))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallInput {
    token: String,
    name: String,
    description: String,
    username: String,
    nickname: String,
    password: String,
}
pub async fn install(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(input): Json<InstallInput>,
) -> ApiResult<Json<Value>> {
    origin(&headers)?;
    accounts::rate(&mut *app.accounts.lock().await)?;
    let store = accounts::store(&app).await;
    if store
        .site()
        .await
        .map_err(accounts::database_error)?
        .is_some()
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "网站已安装，安装入口已关闭".into(),
        ));
    }
    if !matches_token(&app.setup_token, &input.token) {
        return Err(ApiError(StatusCode::FORBIDDEN, "安装密钥无效".into()));
    }
    let site = Site {
        name: input.name.trim().into(),
        description: input.description.trim().into(),
    };
    site.validate().map_err(bad)?;
    accounts::username(&input.username).map_err(bad)?;
    accounts::nickname(&input.nickname).map_err(bad)?;
    let _permit = app
        .hash_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "认证服务繁忙".into()))?;
    let hash = accounts::hash_password(input.password).await.map_err(bad)?;
    store
        .install(&site, &input.username, &input.nickname, &hash)
        .await
        .map_err(accounts::database_error)?;
    app.nicknames
        .write()
        .await
        .insert(input.username.clone(), input.nickname.clone());
    *app.site.lock().await =
        Some(json!({"installed": true, "site": site, "owner": input.username}));
    let pending = if let Err(error) = crate::starter::ensure(&app).await {
        tracing::warn!(action = "starter_articles_pending", error = %error);
        true
    } else {
        false
    };
    Ok(Json(
        json!({"status":"installed", "starter_articles_pending":pending}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn installation_token_survives_restart_and_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let first = load_token(dir.path()).await.unwrap();
        assert_eq!(first.len(), 43);
        assert_eq!(load_token(dir.path()).await.unwrap(), first);
        assert_eq!(
            std::fs::metadata(dir.path().join("setup-token"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    #[test]
    fn installation_input_limits_and_token_validation() {
        assert!(
            Site {
                name: "我的博客".into(),
                description: "文章与随笔。".into()
            }
            .validate()
            .is_ok()
        );
        for name in ["".into(), "x".repeat(121), "bad\nname".into()] {
            assert!(
                Site {
                    name,
                    description: String::new()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            Site {
                name: "blog".into(),
                description: "x".repeat(1001)
            }
            .validate()
            .is_err()
        );
        assert!(matches_token("secret", "secret"));
        assert!(!matches_token("secret", "secrex"));
        assert!(!matches_token("secret", "secre"));
    }
}
