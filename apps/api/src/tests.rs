use super::*;
use accounts::{Grant, Role};
use axum::http::Request;
use std::collections::BTreeMap;
use tower::ServiceExt;

fn markdown(key: &str, status: &str, body: &str) -> String {
    format!(
        "---\ntitle: Test\nslug: {key}\ndate: '2026-10-15'\nupdated: '2026-10-15'\nstatus: {status}\ntags: [Rust]\ndescription: Test article\n---\n\n{body}\n"
    )
}
fn grant(prefix: &str) -> Grant {
    Grant {
        enabled: true,
        role: Role::Publisher,
        prefixes: vec![prefix.into()],
        ssh_key: None,
    }
}
async fn commit(
    repo: &std::path::Path,
    parent: Option<&str>,
    files: &[(&str, &str, &str)],
) -> String {
    content::git(repo, &["read-tree", "--empty"], None)
        .await
        .unwrap();
    for (path, mode, body) in files {
        let object = String::from_utf8(
            content::git(
                repo,
                &["hash-object", "-w", "--stdin"],
                Some(body.as_bytes()),
            )
            .await
            .unwrap(),
        )
        .unwrap();
        content::git(
            repo,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                mode,
                object.trim(),
                path,
            ],
            None,
        )
        .await
        .unwrap();
    }
    let tree = String::from_utf8(content::git(repo, &["write-tree"], None).await.unwrap()).unwrap();
    let mut args = vec!["commit-tree", tree.trim(), "-m", "Test"];
    if let Some(parent) = parent {
        args.extend(["-p", parent]);
    }
    String::from_utf8(content::git(repo, &args, None).await.unwrap())
        .unwrap()
        .trim()
        .into()
}
#[test]
fn rejects_active_content_and_unsafe_metadata_but_keeps_inert_code() {
    for body in [
        "<script>alert(1)</script>",
        "<img src=x onerror=alert(1)>",
        "[x](javascript:alert%281%29)",
        "[x](jav&#x61;script:alert%281%29)",
        "![x](https://evil.test/x.png)",
        "![x](../secret.png)",
        "a\u{202e}exe",
        "[x](//evil.test)",
        "[x](file:///etc/passwd)",
    ] {
        assert!(
            content::parse(markdown("alice-test", "published", body), vec![]).is_err(),
            "accepted {body}"
        );
    }
    assert!(content::parse(markdown("../escape", "published", "Hello"), vec![]).is_err());
    assert!(
        content::parse(
            markdown(
                "alice-test",
                "published",
                &"x".repeat(content::MAX_MARKDOWN)
            ),
            vec![]
        )
        .is_err()
    );
    assert!(
        content::parse(
            markdown(
                "alice-test",
                "published",
                "```html\n<script>alert(1)</script>\n```\n[valid](https://example.com)"
            ),
            vec![]
        )
        .is_ok()
    );
    assert!(
        content::decode_asset(&content::Asset {
            name: "hidden.svg".into(),
            data: "PHN2Zz4=".into()
        })
        .is_err()
    );
    assert!(
        content::decode_asset(&content::Asset {
            name: "fake.png".into(),
            data: "PHNjcmlwdD4=".into()
        })
        .is_err()
    );
}
#[tokio::test]
async fn git_checks_namespace_history_and_file_modes_before_publication() {
    let dir = tempfile::tempdir().unwrap();
    let repo = content::init(dir.path()).await.unwrap();
    let zero = "0000000000000000000000000000000000000000";
    let first = commit(
        &repo,
        None,
        &[(
            "posts/alice-first/index.md",
            "100644",
            &markdown("alice-first", "published", "Hello"),
        )],
    )
    .await;
    let snapshot = ssh::validate_change(&repo, zero, &first, &grant("alice-"))
        .await
        .unwrap();
    assert_eq!(snapshot.posts.len(), 1);
    assert!(
        ssh::validate_change(&repo, zero, &first, &grant("bob-"))
            .await
            .is_err()
    );
    let crlf = commit(
        &repo,
        Some(&first),
        &[(
            "posts/alice-first/index.md",
            "100644",
            &markdown("alice-first", "published", "Hello").replace('\n', "\r\n"),
        )],
    )
    .await;
    assert!(
        ssh::validate_change(&repo, &first, &crlf, &grant("bob-"))
            .await
            .is_err(),
        "normalization must not bypass path authorization"
    );
    let second = commit(
        &repo,
        Some(&first),
        &[(
            "posts/alice-first/index.md",
            "100644",
            &markdown("alice-first", "draft", "Private"),
        )],
    )
    .await;
    ssh::validate_change(&repo, &first, &second, &grant("alice-"))
        .await
        .unwrap();
    let third = commit(
        &repo,
        Some(&second),
        &[(
            "posts/alice-first/index.md",
            "100644",
            &markdown("alice-first", "published", "Skipped commit"),
        )],
    )
    .await;
    assert!(
        ssh::validate_change(&repo, &first, &third, &grant("alice-"))
            .await
            .is_err()
    );
    let link = commit(
        &repo,
        Some(&first),
        &[("posts/alice-first/index.md", "120000", "/etc/passwd")],
    )
    .await;
    assert!(
        ssh::validate_change(&repo, &first, &link, &grant("alice-"))
            .await
            .is_err()
    );
    let executable = commit(
        &repo,
        Some(&first),
        &[(
            "posts/alice-first/index.md",
            "100755",
            &markdown("alice-first", "published", "Hello"),
        )],
    )
    .await;
    assert!(
        ssh::validate_change(&repo, &first, &executable, &grant("alice-"))
            .await
            .is_err()
    );
    let hidden = commit(
        &repo,
        Some(&first),
        &[(".github/workflows/run.yml", "100644", "bad")],
    )
    .await;
    assert!(
        ssh::validate_change(&repo, &first, &hidden, &grant("alice-"))
            .await
            .is_err()
    );
    assert!(
        ssh::validate_change(&repo, &first, zero, &grant("alice-"))
            .await
            .is_err()
    );
    content::persist(dir.path(), &content::scan_at(&repo, &second).await.unwrap())
        .await
        .unwrap();
    let public: Snapshot = serde_json::from_slice(
        &tokio::fs::read(dir.path().join("published.json"))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(public.posts.is_empty(), "draft leaked into public snapshot");
}
#[test]
fn ssh_keys_cannot_inject_options_or_commands() {
    let mut bytes = vec![0, 0, 0, 11];
    bytes.extend(b"ssh-ed25519");
    bytes.extend([0, 0, 0, 32]);
    bytes.extend([42; 32]);
    let key = format!(
        "ssh-ed25519 {} comment",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    assert!(accounts::public_key(&key).is_ok());
    assert!(accounts::public_key(&format!("command=\"sh\" {key}")).is_err());
    assert!(accounts::public_key(&format!("{key}\nssh-rsa AAA")).is_err());
    assert!(accounts::public_key("ssh-ed25519 AAAA").is_err());
}
fn rsa_key(bits: usize, exponent: &[u8]) -> String {
    let mut modulus = vec![0xff; bits.div_ceil(8)];
    modulus[0] >>= (8 - bits % 8) % 8;
    if modulus[0] & 0x80 != 0 {
        modulus.insert(0, 0);
    }
    let mut bytes = Vec::new();
    for field in [b"ssh-rsa".as_slice(), exponent, modulus.as_slice()] {
        bytes.extend((field.len() as u32).to_be_bytes());
        bytes.extend(field);
    }
    format!(
        "ssh-rsa {}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}
#[test]
fn rsa_keys_enforce_strength_encoding_and_safe_canonical_form() {
    for bits in [2048, 3072, 4096, 8192] {
        let key = rsa_key(bits, &[1, 0, 1]);
        assert_eq!(
            accounts::public_key(&format!("{key} user comment")).unwrap(),
            key
        );
    }
    for key in [
        rsa_key(1024, &[1, 0, 1]),
        rsa_key(2047, &[1, 0, 1]),
        rsa_key(8193, &[1, 0, 1]),
        rsa_key(2048, &[2]),
        rsa_key(2048, &[1]),
        rsa_key(2048, &[0x80]),
        rsa_key(2048, &[1, 0, 0, 0, 1]),
    ] {
        assert!(accounts::public_key(&key).is_err());
    }
    let key = rsa_key(2048, &[1, 0, 1]);
    assert!(accounts::public_key(&key.replace("ssh-rsa", "ssh-ed25519")).is_err());
    assert!(accounts::public_key(&format!("command=\"sh\" {key}")).is_err());
    let encoded = key.split_whitespace().nth(1).unwrap();
    let mut bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    bytes.push(0);
    assert!(
        accounts::public_key(&format!(
            "ssh-rsa {}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
        .is_err()
    );
}
async fn app(dir: &std::path::Path) -> Arc<App> {
    let config = dir.join("publishers.json");
    let hash = accounts::hash_password("Administrator-test-123".into())
        .await
        .unwrap();
    accounts::atomic(
        &dir.join("users.json"),
        &serde_json::to_vec(&json!({"admin": {"hash": hash}})).unwrap(),
        0o600,
    )
    .await
    .unwrap();
    let grants = ssh::Config {
        repository: dir.join("ingress.git"),
        publishers: BTreeMap::from([(
            "admin".into(),
            Grant {
                enabled: true,
                role: Role::Admin,
                prefixes: vec![],
                ssh_key: None,
            },
        )]),
    };
    accounts::atomic(&config, &serde_json::to_vec(&grants).unwrap(), 0o644)
        .await
        .unwrap();
    let accounts = accounts::load(dir, &config, grants.repository)
        .await
        .unwrap();
    let published = content::parse(markdown("alice-public", "published", "Hello"), vec![]).unwrap();
    let draft = content::parse(markdown("alice-secret", "draft", "Secret"), vec![]).unwrap();
    Arc::new(App {
        data: dir.into(),
        ssh_config: config,
        origin: "http://localhost:4321".into(),
        secure: false,
        registration: true,
        accounts: Mutex::new(accounts),
        hash_slots: Arc::new(Semaphore::new(2)),
        snapshot: RwLock::new(Snapshot {
            commit: "test".into(),
            posts: BTreeMap::from([
                ("alice-public".into(), published),
                ("alice-secret".into(), draft),
            ]),
        }),
        indexed: RwLock::new(None),
        views: AtomicU64::new(0),
    })
}
async fn request(
    app: Arc<App>,
    method: &str,
    path: &str,
    body: Value,
    cookie: Option<&str>,
    csrf: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("origin", "http://localhost:4321");
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf-token", csrf);
    }
    let response = router(app)
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        headers,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn registration_login_grants_revocation_and_private_content_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let (status, _, _) = request(
        app.clone(),
        "POST",
        "/api/auth/register",
        json!({"username":"alice", "password":"Alice-test-password-123", "role":"admin"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, value) = request(
        app.clone(),
        "POST",
        "/api/auth/register",
        json!({"username":"alice", "password":"Alice-test-password-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["role"], "reader");
    let (_, headers, value) = request(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({"username":"alice", "password":"Alice-test-password-123"}),
        None,
        None,
    )
    .await;
    let alice_cookie = headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let alice_csrf = value["csrf"].as_str().unwrap().to_owned();
    let (status, _, _) = request(
        app.clone(),
        "GET",
        "/api/admin/users",
        Value::Null,
        Some(&alice_cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (_, headers, value) = request(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({"username":"admin", "password":"Administrator-test-123"}),
        None,
        None,
    )
    .await;
    let admin_cookie = headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let admin_csrf = value["csrf"].as_str().unwrap().to_owned();
    let input = json!({"enabled":true,"role":"publisher","prefixes":["alice-"]});
    let (status, _, _) = request(
        app.clone(),
        "PATCH",
        "/api/admin/users/alice",
        input.clone(),
        Some(&admin_cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = request(
        app.clone(),
        "PATCH",
        "/api/admin/users/alice",
        input,
        Some(&admin_cookie),
        Some(&admin_csrf),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        app.accounts.lock().await.grants.publishers["alice"].role,
        Role::Publisher
    );
    let disk = tokio::fs::read(dir.path().join("users.json"))
        .await
        .unwrap();
    assert!(
        !String::from_utf8(disk)
            .unwrap()
            .contains("Alice-test-password-123")
    );
    let (status, _, _) = request(
        app.clone(),
        "POST",
        "/api/auth/logout",
        json!({}),
        Some(&alice_cookie),
        Some(&alice_csrf),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "grant change must invalidate existing sessions"
    );
    let (status, _, _) = request(
        app.clone(),
        "PATCH",
        "/api/admin/users/alice",
        json!({"enabled":false,"role":"reader","prefixes":[]}),
        Some(&admin_cookie),
        Some(&admin_csrf),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = request(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({"username":"alice", "password":"Alice-test-password-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (_, _, value) = request(app.clone(), "GET", "/api/posts", Value::Null, None, None).await;
    assert_eq!(value["posts"].as_array().unwrap().len(), 1);
    let (status, _, _) = request(
        app.clone(),
        "GET",
        "/api/posts/alice-secret",
        Value::Null,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, _, value) = request(app.clone(), "GET", "/api/stats", Value::Null, None, None).await;
    assert_eq!(value["published_posts"], 1);
    let (status, _, _) = request(
        app.clone(),
        "POST",
        "/api/admin/posts/publish",
        json!({}),
        Some(&admin_cookie),
        Some(&admin_csrf),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "online publishing must not exist"
    );
}

#[tokio::test]
async fn password_rotation_origin_and_request_limits() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let response = router(app.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"username":"admin","password":"Administrator-test-123"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "login must validate Origin"
    );
    let (_, headers, value) = request(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({"username":"admin","password":"Administrator-test-123"}),
        None,
        None,
    )
    .await;
    let cookie = headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let csrf = value["csrf"].as_str().unwrap();
    let (status, _, _) = request(
        app.clone(),
        "PUT",
        "/api/account/password",
        json!({"old_password":"Incorrect-password", "new_password":"New-password-12345"}),
        Some(&cookie),
        Some(csrf),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = request(
        app.clone(),
        "PUT",
        "/api/account/password",
        json!({"old_password":"Administrator-test-123", "new_password":"New-password-12345"}),
        Some(&cookie),
        Some(csrf),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = request(
        app.clone(),
        "GET",
        "/api/account",
        Value::Null,
        Some(&cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let loaded = accounts::load(dir.path(), &app.ssh_config, dir.path().join("ingress.git"))
        .await
        .unwrap();
    *app.accounts.lock().await = loaded;
    let (status, _, _) = request(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({"username":"admin", "password":"Administrator-test-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = request(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({"username":"admin", "password":"New-password-12345"}),
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "password change must survive account reload"
    );
    let (status, _, _) = request(
        app,
        "POST",
        "/api/auth/register",
        json!({"username":"large", "password":"x".repeat(9000)}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}
