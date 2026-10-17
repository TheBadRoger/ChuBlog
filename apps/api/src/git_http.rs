use crate::{ApiError, ApiResult, App, accounts, git_store, internal};
use anyhow::{Context, Result, ensure};
use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub const MAX_REQUEST: usize = 9 * 1024 * 1024;
const MAX_RESPONSE: u64 = 64 * 1024 * 1024;

fn challenge(error: ApiError) -> Response {
    let mut response = error.into_response();
    if response.status() == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"ChuBlog Git\", charset=\"UTF-8\""),
        );
    }
    response
}
fn credentials(headers: &HeaderMap) -> ApiResult<(String, String)> {
    let denied = || ApiError(StatusCode::UNAUTHORIZED, "需要 Git 账号密码".into());
    if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
        return Err(denied());
    }
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(denied)?;
    if value.len() > 2048 {
        return Err(denied());
    }
    let (scheme, encoded) = value.split_once(' ').ok_or_else(denied)?;
    if !scheme.eq_ignore_ascii_case("Basic") {
        return Err(denied());
    }
    let bytes = STANDARD.decode(encoded).map_err(|_| denied())?;
    let text = String::from_utf8(bytes).map_err(|_| denied())?;
    let (name, password) = text.split_once(':').ok_or_else(denied)?;
    Ok((name.into(), password.into()))
}

pub async fn handle(State(app): State<Arc<App>>, request: Request) -> Response {
    match serve(app, request).await {
        Ok(response) => response,
        Err(error) => challenge(error),
    }
}
async fn serve(app: Arc<App>, request: Request) -> ApiResult<Response> {
    // The web listener is loopback-only and overwrites the proxy headers at the public edge.
    let mut origin_headers = request.headers().clone();
    origin_headers.remove(header::ORIGIN);
    let host = origin_headers
        .get("x-forwarded-host")
        .or_else(|| origin_headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let scheme = origin_headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let origin = HeaderValue::from_str(&format!("{scheme}://{host}"))
        .map_err(|_| ApiError(StatusCode::FORBIDDEN, "来源无效".into()))?;
    origin_headers.insert(header::ORIGIN, origin);
    crate::origin(&origin_headers)?;
    if request.headers().contains_key(header::ORIGIN) {
        crate::origin(request.headers())?;
    }
    let path = request.uri().path();
    let query = request.uri().query().unwrap_or("");
    let service = match (request.method(), path, query) {
        (&Method::GET, "/git/content.git/info/refs", "service=git-upload-pack") => {
            "git-upload-pack"
        }
        (&Method::GET, "/git/content.git/info/refs", "service=git-receive-pack") => {
            "git-receive-pack"
        }
        (&Method::POST, "/git/content.git/git-upload-pack", "") => "git-upload-pack",
        (&Method::POST, "/git/content.git/git-receive-pack", "") => "git-receive-pack",
        _ => return Err(ApiError(StatusCode::NOT_FOUND, "Git 路径不存在".into())),
    };
    let compressed = match request
        .headers()
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
    {
        None | Some("identity") => false,
        Some("gzip" | "x-gzip") => true,
        Some(_) => {
            return Err(ApiError(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "Git 请求压缩格式无效".into(),
            ));
        }
    };
    if request
        .headers()
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .count()
        > 1
    {
        return Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Git 请求压缩头重复".into(),
        ));
    }
    if request.method() == Method::POST
        && request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            != Some(format!("application/x-{service}-request").as_str())
    {
        return Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Git Content-Type 无效".into(),
        ));
    }
    let permit = app
        .git_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError(StatusCode::TOO_MANY_REQUESTS, "Git 服务繁忙".into()))?;
    let (identity, password) = credentials(request.headers())?;
    let hash = accounts::git_credentials(&app, &identity, password).await?;
    let method = request.method().as_str().to_owned();
    let tail = path.trim_start_matches("/git/content.git").to_owned();
    let query = query.to_owned();
    let protocol = match request
        .headers()
        .get("git-protocol")
        .and_then(|v| v.to_str().ok())
    {
        None => None,
        Some("version=2") => Some("version=2"),
        Some(_) => return Err(ApiError(StatusCode::BAD_REQUEST, "Git 协议版本无效".into())),
    };
    let limit = if method == "GET" {
        0
    } else if service == "git-upload-pack" {
        128 * 1024
    } else {
        MAX_REQUEST
    };
    let body = tokio::time::timeout(
        Duration::from_secs(15),
        to_bytes(request.into_body(), limit),
    )
    .await
    .map_err(|_| ApiError(StatusCode::REQUEST_TIMEOUT, "请求体接收超时".into()))?
    .map_err(|_| ApiError(StatusCode::PAYLOAD_TOO_LARGE, "Git 请求体过大".into()))?;
    // A disconnected client must not drop the repository lock while Git is still running.
    tokio::spawn(async move {
        let _permit = permit;
        let body = if compressed {
            tokio::task::spawn_blocking(move || decode_gzip(&body, limit))
                .await
                .map_err(internal)??
        } else {
            body.to_vec()
        };
        let store = accounts::store(&app).await;
        let user = store
            .user(&identity)
            .await
            .map_err(accounts::database_error)?
            .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "账号已移除".into()))?;
        if !matches!(
            user.level,
            crate::accounts_db::Level::Author | crate::accounts_db::Level::Master
        ) {
            return Err(ApiError(StatusCode::FORBIDDEN, "发布权已撤销".into()));
        }
        if user.hash != hash {
            return Err(ApiError(StatusCode::UNAUTHORIZED, "口令已变更".into()));
        }
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(app.repository.join(format!(".{identity}.lock")))
            .map_err(internal)?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .map_err(|_| ApiError(StatusCode::CONFLICT, "仓库正在使用，请重试".into()))?;
        git_store::init_author(&app.repository, &identity)
            .await
            .map_err(internal)?;
        let root = app.repository.clone();
        tokio::task::spawn_blocking(move || git_store::directory_size(&root))
            .await
            .map_err(internal)?
            .map_err(internal)?;
        let response = execute(
            &app, &identity, &hash, &method, &tail, &query, service, protocol, body,
        )
        .await
        .map_err(internal);
        drop(lock);
        response
    })
    .await
    .map_err(internal)?
}

#[allow(clippy::too_many_arguments)]
async fn execute(
    app: &App,
    identity: &str,
    hash: &str,
    method: &str,
    tail: &str,
    query: &str,
    service: &str,
    protocol: Option<&str>,
    body: Vec<u8>,
) -> Result<Response> {
    let mut command = Command::new("timeout");
    command
        .args(["--signal=KILL", "90s", "git", "http-backend"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/nonexistent")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_PROJECT_ROOT", &app.repository)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", format!("/{identity}.git{tail}"))
        .env("QUERY_STRING", query)
        .env("REQUEST_METHOD", method)
        .env("REMOTE_USER", identity)
        .env("REMOTE_ADDR", "127.0.0.1")
        .env("CONTENT_TYPE", format!("application/x-{service}-request"))
        .env("CONTENT_LENGTH", body.len().to_string())
        .env("SERVER_PROTOCOL", "HTTP/1.1")
        .env("CHUBLOG_PUBLISHER", identity)
        .env("CHUBLOG_PASSWORD_HASH", hash)
        .env("INGRESS_DIR", &app.repository)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let settings = [
        ("receive.maxInputSize", "8388608"),
        ("receive.fsckObjects", "true"),
        ("transfer.fsckObjects", "true"),
        ("gc.auto", "0"),
        ("core.hooksPath", "/hooks"),
        ("http.getanyfile", "false"),
        ("uploadpack.allowAnySHA1InWant", "false"),
        ("uploadpack.allowReachableSHA1InWant", "false"),
        ("uploadpack.allowTipSHA1InWant", "false"),
    ];
    command.env("GIT_CONFIG_COUNT", settings.len().to_string());
    for (index, (key, value)) in settings.iter().enumerate() {
        command
            .env(format!("GIT_CONFIG_KEY_{index}"), key)
            .env(format!("GIT_CONFIG_VALUE_{index}"), value);
    }
    if let Some(protocol) = protocol {
        command.env("GIT_PROTOCOL", protocol);
    }
    accounts::store(app)
        .await
        .database
        .configure_child(&mut command);
    let mut child = command.spawn().context("无法启动 Git HTTP 协议执行器")?;
    let mut input = child.stdin.take().context("缺少 Git stdin")?;
    let output = child.stdout.take().context("缺少 Git stdout")?;
    let error = child.stderr.take().context("缺少 Git stderr")?;
    let write = async move { input.write_all(&body).await };
    let read = async move {
        let mut bytes = Vec::new();
        output
            .take(MAX_RESPONSE + 1)
            .read_to_end(&mut bytes)
            .await?;
        Ok::<_, std::io::Error>(bytes)
    };
    let stderr = async move {
        let mut bytes = Vec::new();
        error.take(8193).read_to_end(&mut bytes).await?;
        Ok::<_, std::io::Error>(bytes)
    };
    let (_written, bytes, _errors) = tokio::join!(write, read, stderr);
    let status = child.wait().await?;
    let bytes = bytes?;
    ensure!(
        status.success() && bytes.len() <= MAX_RESPONSE as usize,
        "Git 服务超时、失败或输出过大"
    );
    parse_response(bytes)
}
fn parse_response(bytes: Vec<u8>) -> Result<Response> {
    let split = bytes
        .windows(4)
        .position(|s| s == b"\r\n\r\n")
        .context("Git CGI 响应头无效")?;
    ensure!(split <= 8192, "Git CGI 响应头过大");
    let mut response = Response::new(Body::from(bytes[split + 4..].to_vec()));
    for line in std::str::from_utf8(&bytes[..split])?.split("\r\n") {
        let (key, value) = line.split_once(':').context("Git CGI 响应头无效")?;
        let value = value.trim();
        if key.eq_ignore_ascii_case("status") {
            *response.status_mut() = StatusCode::from_u16(
                value
                    .split_whitespace()
                    .next()
                    .context("缺少 CGI 状态")?
                    .parse()?,
            )?;
        } else if ["content-type", "cache-control", "expires", "pragma"]
            .iter()
            .any(|name| key.eq_ignore_ascii_case(name))
        {
            response.headers_mut().insert(
                HeaderName::from_bytes(key.as_bytes())?,
                HeaderValue::from_str(value)?,
            );
        }
    }
    Ok(response)
}

fn decode_gzip(body: &[u8], limit: usize) -> ApiResult<Vec<u8>> {
    use std::io::Read;
    let mut decoded = Vec::new();
    flate2::read::MultiGzDecoder::new(body)
        .take(limit as u64 + 1)
        .read_to_end(&mut decoded)
        .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "Git gzip 数据无效".into()))?;
    if decoded.len() > limit {
        return Err(ApiError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Git 解压后请求体过大".into(),
        ));
    }
    Ok(decoded)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn gzip_requests_are_bounded_after_decompression() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&vec![b'a'; 256 * 1024]).unwrap();
        let encoded = encoder.finish().unwrap();
        assert!(matches!(
            decode_gzip(&encoded, 128 * 1024),
            Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, _))
        ));
        assert!(decode_gzip(b"bad gzip", 128 * 1024).is_err());
        assert!(matches!(decode_gzip(&encoded, 256 * 1024), Ok(body) if body.len() == 256 * 1024));
    }
}
