use super::*;
use accounts::{Grant, Role};
use axum::http::Request;
use std::collections::BTreeMap;
use tower::ServiceExt;

fn manifest(id: &str, folder: &str, public: bool) -> String {
    format!(
        "[[articles]]\nid = {id:?}\nfolder = {folder:?}\ntitle = \"Test\"\ndescription = \"Test article\"\ntags = [\"Rust\"]\npublic = {public}\n"
    )
}
fn ready(outcome: content::Scan) -> Snapshot {
    match outcome {
        content::Scan::Ready(snapshot) => snapshot,
        content::Scan::Pending(reason) => panic!("unexpected staged index: {reason}"),
    }
}
fn test_post(id: &str, public: bool, body: &str) -> content::Post {
    content::Post {
        meta: content::Metadata {
            title: "Test".into(),
            author: "alice".into(),
            slug: id.into(),
            date: "2026-10-16T00:00:00Z".into(),
            updated: "2026-10-16T00:00:00Z".into(),
            created_at: "2026-10-16T00:00:00Z".into(),
            status: if public {
                Status::Published
            } else {
                Status::Draft
            },
            tags: vec!["Rust".into()],
            description: "Test article".into(),
            cover: None,
        },
        markdown: body.into(),
        pages: BTreeMap::from([("main.md".into(), body.into())]),
        entry: "main.md".into(),
        assets: vec![],
        author: "alice".into(),
        article_id: id.into(),
        path: "posts/first/main.md".into(),
        git_commit: "test".into(),
        format: "markdown".into(),
    }
}
async fn commit(
    repo: &std::path::Path,
    parent: Option<&str>,
    files: &[(&str, &str, &str)],
) -> String {
    let files: Vec<_> = files
        .iter()
        .map(|(path, mode, body)| (*path, *mode, body.as_bytes()))
        .collect();
    commit_bytes(repo, parent, &files).await
}
async fn commit_bytes(
    repo: &std::path::Path,
    parent: Option<&str>,
    files: &[(&str, &str, &[u8])],
) -> String {
    content::git(repo, &["read-tree", "--empty"], None)
        .await
        .unwrap();
    for (path, mode, body) in files {
        let object = String::from_utf8(
            content::git(repo, &["hash-object", "-w", "--stdin"], Some(body))
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
fn rejects_active_content_and_unsafe_links_but_keeps_inert_code() {
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
        "[x](../other/main.md)",
        "[x](%2e%2e%2fsecret.md)",
        "[x](a.md?script=1)",
    ] {
        assert!(
            content::validate_markdown(body, &[]).is_err(),
            "accepted {body}"
        );
    }
    for body in [
        "[next](next.md#chapter)",
        "[next](./next.markdown)",
        "```html\n<script>alert(1)</script>\n```\n[valid](https://example.com)",
    ] {
        assert!(content::validate_markdown(body, &[]).is_ok());
    }
    for (name, data) in [("hidden.svg", "PHN2Zz4="), ("fake.png", "PHNjcmlwdD4=")] {
        assert!(
            content::decode_asset(&content::Asset {
                name: name.into(),
                data: data.into()
            })
            .is_err()
        );
    }
}
#[test]
fn toml_manifest_limits_author_fields_and_defaults_entry_to_main() {
    let text = manifest("first", "posts/writing", true);
    let index = content::manifest(&text).unwrap();
    assert_eq!(index[0].id, "first");
    assert_eq!(index[0].entry, "main.md");
    for input in [
        format!("{text}author = \"master\""),
        format!("{text}date = \"2000-01-01\""),
        format!("{text}updated = \"2000-01-01\""),
        format!("{text}entry = \"../secret.md\""),
        format!("{text}entry = \".gitignore\""),
        format!("{text}entry = \"cert.pem\""),
        format!("{text}{}", manifest("first", "posts/second", true)),
        format!("{text}{}", manifest("second", "posts/writing", true)),
        manifest("../escape", "posts/a", true),
        manifest("first", "posts/../escape", true),
        "".into(),
        "[[articles".into(),
        "articles = 1".into(),
        " ".repeat(65537),
    ] {
        assert!(content::manifest(&input).is_err(), "accepted {input}");
    }
    assert!(ssh::author_repository(std::path::Path::new("/private"), "../bob").is_err());
}

#[tokio::test]
async fn git_stages_invalid_index_but_rejects_unsafe_files_and_history() {
    let dir = tempfile::tempdir().unwrap();
    let repo = content::init(dir.path()).await.unwrap();
    let zero = "0000000000000000000000000000000000000000";
    let index = manifest("first", "posts/writing", true);
    let files = [
        ("index.toml", "100644", index.as_str()),
        ("posts/writing/main.md", "100644", "Hello"),
    ];
    let first = commit(&repo, None, &files).await;
    let snapshot = ready(ssh::validate_change(&repo, zero, &first).await.unwrap());
    assert_eq!(snapshot.posts["first"].path, "posts/writing/main.md");
    let removed = commit(
        &repo,
        Some(&first),
        &[("index.toml", "100644", "articles = []"), files[1]],
    )
    .await;
    assert!(
        ready(ssh::validate_change(&repo, &first, &removed).await.unwrap())
            .posts
            .is_empty()
    );
    let missing = commit(&repo, Some(&first), &[files[0]]).await;
    assert!(
        ready(content::scan_at(&repo, &missing).await.unwrap())
            .posts
            .is_empty()
    );
    for bad_index in ["[[articles", "articles = 1"] {
        let candidate = commit(
            &repo,
            Some(&first),
            &[("index.toml", "100644", bad_index), files[1]],
        )
        .await;
        assert!(matches!(
            ssh::validate_change(&repo, &first, &candidate)
                .await
                .unwrap(),
            content::Scan::Pending(_)
        ));
    }
    let malicious_staged = commit(
        &repo,
        Some(&first),
        &[
            ("index.toml", "100644", "[[articles"),
            (
                "posts/writing/main.md",
                "100644",
                "<script>alert(1)</script>",
            ),
        ],
    )
    .await;
    assert!(
        ssh::validate_change(&repo, &first, &malicious_staged)
            .await
            .is_err(),
        "broken TOML must not bypass content security checks"
    );
    let no_index = commit(&repo, Some(&first), &[files[1]]).await;
    assert!(matches!(
        content::scan_at(&repo, &no_index).await.unwrap(),
        content::Scan::Pending(_)
    ));
    for (path, mode, value) in [
        (".gitignore", "100644", "secret"),
        ("posts/writing/cert.pem", "100644", "cert"),
        (
            "posts/writing/secret.txt",
            "100644",
            "-----BEGIN PRIVATE KEY-----",
        ),
        (
            "posts/unlisted/hidden.md",
            "100644",
            "<script>alert(1)</script>",
        ),
        ("posts/writing/fake.png", "100644", "not an image"),
        ("posts/other/main.md", "120000", "/etc/passwd"),
        ("posts/other/main.md", "100755", "executable"),
        ("posts/writing/payload.bin", "100644", "binary"),
    ] {
        let candidate = commit(
            &repo,
            Some(&first),
            &[files[0], files[1], (path, mode, value)],
        )
        .await;
        assert!(
            ssh::validate_change(&repo, &first, &candidate)
                .await
                .is_err(),
            "accepted {path}"
        );
    }
    let oversize = "x".repeat(content::MAX_MARKDOWN + 1);
    let candidate = commit(
        &repo,
        Some(&first),
        &[files[0], ("posts/writing/main.md", "100644", &oversize)],
    )
    .await;
    assert!(
        ssh::validate_change(&repo, &first, &candidate)
            .await
            .is_err()
    );
    let skipped = commit(&repo, Some(&removed), &files).await;
    assert!(ssh::validate_change(&repo, &first, &skipped).await.is_err());
    assert!(ssh::validate_change(&repo, &first, zero).await.is_err());
}

#[tokio::test]
async fn multi_page_graph_and_cover_exclude_unlinked_documents_and_assets() {
    let dir = tempfile::tempdir().unwrap();
    let repo = content::init(dir.path()).await.unwrap();
    let index = format!(
        "{}entry = \"article.markdown\"\ncover = \"cover.png\"\n",
        manifest("first", "posts/writing", true)
    );
    let mut image = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(1, 1)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let commit = commit_bytes(
        &repo,
        None,
        &[
            ("index.toml", "100644", index.as_bytes()),
            (
                "posts/writing/article.markdown",
                "100644",
                b"[Next](chapter.md#part)",
            ),
            (
                "posts/writing/chapter.md",
                "100644",
                b"[Back](./article.markdown)\n![x](figure.png)",
            ),
            ("posts/writing/notes.txt", "100644", b"Unselected notes"),
            ("posts/unlisted/draft.txt", "100644", b"Not registered"),
            ("posts/writing/cover.png", "100644", image.get_ref()),
            ("posts/writing/figure.png", "100644", image.get_ref()),
            ("posts/writing/private.png", "100644", image.get_ref()),
        ],
    )
    .await;
    let snapshot = ready(content::scan_at(&repo, &commit).await.unwrap());
    assert_eq!(snapshot.posts.len(), 1);
    let post = &snapshot.posts["first"];
    assert_eq!(post.path, "posts/writing/article.markdown");
    assert_eq!(post.pages.len(), 2);
    assert_eq!(post.assets.len(), 2);
    assert!(!post.assets.iter().any(|a| a.name == "private.png"));
    assert!(
        !serde_json::to_string(&snapshot)
            .unwrap()
            .contains("Unselected notes")
    );
    let mut draft = snapshot.clone();
    draft.posts.get_mut("first").unwrap().meta.status = Status::Draft;
    content::persist(dir.path(), &draft).await.unwrap();
    let public: Snapshot =
        serde_json::from_slice(&std::fs::read(dir.path().join("published.json")).unwrap()).unwrap();
    assert!(public.posts.is_empty());
}

#[test]
fn timestamps_are_server_assigned_and_only_change_with_article_content() {
    let mut post = test_post("first", false, "Hello");
    post.meta.date.clear();
    post.meta.created_at.clear();
    post.meta.updated.clear();
    content::stamp(&mut post, None, "2026-10-16T00:00:00Z");
    assert_eq!(post.meta.date, "");
    let initial = post.clone();
    post.meta.status = Status::Published;
    content::stamp(&mut post, Some(&initial), "2026-10-16T01:00:00Z");
    assert_eq!(post.meta.date, "2026-10-16T01:00:00Z");
    assert_eq!(post.meta.created_at, "2026-10-16T00:00:00Z");
    let published = post.clone();
    post.git_commit = "another unrelated commit".into();
    content::stamp(&mut post, Some(&published), "2026-10-16T02:00:00Z");
    assert_eq!(post.meta.updated, published.meta.updated);
    let unchanged = post.clone();
    post.pages
        .insert("chapter.md".into(), "Changed second page".into());
    content::stamp(&mut post, Some(&unchanged), "2026-10-16T03:00:00Z");
    assert_eq!(post.meta.updated, "2026-10-16T03:00:00Z");
    assert_eq!(post.meta.date, published.meta.date);
}

#[tokio::test]
async fn author_import_preserves_live_articles_on_bad_index_and_recovers_after_fix() {
    let dir = tempfile::tempdir().unwrap();
    let ingress = dir.path().join("ingress");
    std::fs::create_dir(&ingress).unwrap();
    let alice = ssh::init_author(&ingress, "alice").await.unwrap();
    let bob = ssh::init_author(&ingress, "bob").await.unwrap();
    let a_index = manifest("first", "posts/a", false);
    let b_index = manifest("first", "posts/b", true);
    let a = commit(
        &alice,
        None,
        &[
            ("index.toml", "100644", &a_index),
            ("posts/a/main.md", "100644", "Alice private body"),
        ],
    )
    .await;
    let b_files = [
        ("index.toml", "100644", b_index.as_str()),
        ("posts/b/main.md", "100644", "Bob public body"),
    ];
    let b = commit(&bob, None, &b_files).await;
    content::git(&alice, &["update-ref", "refs/heads/main", &a], None)
        .await
        .unwrap();
    content::git(&bob, &["update-ref", "refs/heads/main", &b], None)
        .await
        .unwrap();
    assert!(
        content::git(&alice, &["cat-file", "-t", &b], None)
            .await
            .is_err()
    );
    let data = dir.path().join("api");
    let names = vec!["alice".into(), "bob".into()];
    let first = ssh::import_authors(&data, &ingress, &Snapshot::default(), &names)
        .await
        .unwrap();
    assert_eq!(first.posts.len(), 2);
    assert_eq!(first.posts["alice~first"].author, "alice");
    assert_eq!(first.posts["bob~first"].article_id, "first");
    let broken = commit(
        &bob,
        Some(&b),
        &[
            ("index.toml", "100644", "[[articles"),
            ("posts/b/main.md", "100644", "Changed pending body"),
        ],
    )
    .await;
    content::git(&bob, &["update-ref", "refs/heads/main", &broken], None)
        .await
        .unwrap();
    let staged = ssh::import_authors(&data, &ingress, &first, &names)
        .await
        .unwrap();
    assert_eq!(staged.commit, first.commit);
    assert_eq!(staged.posts["bob~first"], first.posts["bob~first"]);
    assert_eq!(staged.pending["bob"].commit, broken);
    let staged_again = ssh::import_authors(&data, &ingress, &staged, &names)
        .await
        .unwrap();
    assert_eq!(staged_again.pending, staged.pending);
    let fixed = commit(&bob, Some(&broken), &b_files).await;
    content::git(&bob, &["update-ref", "refs/heads/main", &fixed], None)
        .await
        .unwrap();
    let repaired = ssh::import_authors(&data, &ingress, &staged, &names)
        .await
        .unwrap();
    assert!(repaired.pending.is_empty());
    assert_eq!(
        repaired.posts["bob~first"].meta.updated,
        first.posts["bob~first"].meta.updated
    );
    let removed = commit(&bob, Some(&fixed), &[("index.toml", "100644", &b_index)]).await;
    content::git(&bob, &["update-ref", "refs/heads/main", &removed], None)
        .await
        .unwrap();
    let next = ssh::import_authors(&data, &ingress, &repaired, &names)
        .await
        .unwrap();
    assert!(!next.posts.contains_key("bob~first"));
    assert!(next.posts.contains_key("alice~first"));
    let rebuilt = ssh::import_authors(&data, &ingress, &Snapshot::default(), &names)
        .await
        .unwrap();
    assert_eq!(rebuilt.commit, next.commit);
    assert_eq!(rebuilt.posts.len(), 1);
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
    let published = test_post("alice-public", true, "Hello");
    let draft = test_post("alice-secret", false, "Secret");
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
            revisions: BTreeMap::new(),
            pending: BTreeMap::new(),
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
    let input = json!({"enabled":true,"role":"publisher"});
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
    let (status, _, value) = request(
        app.clone(),
        "GET",
        "/api/posts/alice-public/pages/main.md",
        Value::Null,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["markdown"], "Hello");
    for path in [
        "/api/posts/alice-secret/pages/main.md",
        "/api/posts/alice-public/pages/unlinked.md",
    ] {
        let (status, _, _) = request(app.clone(), "GET", path, Value::Null, None, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
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
