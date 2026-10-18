use super::*;
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
    assert!(git_store::author_repository(std::path::Path::new("/private"), "../bob").is_err());
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
    let snapshot = ready(
        git_store::validate_change(&repo, zero, &first)
            .await
            .unwrap(),
    );
    assert_eq!(snapshot.posts["first"].path, "posts/writing/main.md");
    let removed = commit(
        &repo,
        Some(&first),
        &[("index.toml", "100644", "articles = []"), files[1]],
    )
    .await;
    assert!(
        ready(
            git_store::validate_change(&repo, &first, &removed)
                .await
                .unwrap()
        )
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
            git_store::validate_change(&repo, &first, &candidate)
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
        git_store::validate_change(&repo, &first, &malicious_staged)
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
        ("notes/.hidden/main.md", "100644", "hidden"),
        ("notes/script.js", "100644", "alert(1)"),
        ("notes/secret.txt", "100644", "-----BEGIN PRIVATE KEY-----"),
        ("notes/link.md", "120000", "/etc/passwd"),
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
            git_store::validate_change(&repo, &first, &candidate)
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
        git_store::validate_change(&repo, &first, &candidate)
            .await
            .is_err()
    );
    let skipped = commit(&repo, Some(&removed), &files).await;
    assert!(
        git_store::validate_change(&repo, &first, &skipped)
            .await
            .is_err()
    );
    assert!(
        git_store::validate_change(&repo, &first, zero)
            .await
            .is_err()
    );
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
    let alice = git_store::init_author(&ingress, "alice").await.unwrap();
    let bob = git_store::init_author(&ingress, "bob").await.unwrap();
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
    let mut first = git_store::import_authors(&data, &ingress, &Snapshot::default(), &names)
        .await
        .unwrap();
    first.policies.insert(
        "bob~first".into(),
        content::ArticlePolicy {
            featured: true,
            visibility: content::Visibility::Secret,
            hidden: content::Hidden::Full,
            review: content::Review::Blocked,
            blocked_hash: Some("administrator-hash".into()),
            password_hash: Some("administrator-hash".into()),
        },
    );
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
    let staged = git_store::import_authors(&data, &ingress, &first, &names)
        .await
        .unwrap();
    assert_eq!(staged.commit, first.commit);
    assert_eq!(staged.posts["bob~first"], first.posts["bob~first"]);
    assert_eq!(staged.pending["bob"].commit, broken);
    let staged_again = git_store::import_authors(&data, &ingress, &staged, &names)
        .await
        .unwrap();
    assert_eq!(staged_again.pending, staged.pending);
    let fixed = commit(&bob, Some(&broken), &b_files).await;
    content::git(&bob, &["update-ref", "refs/heads/main", &fixed], None)
        .await
        .unwrap();
    let repaired = git_store::import_authors(&data, &ingress, &staged, &names)
        .await
        .unwrap();
    assert!(repaired.pending.is_empty());
    assert_eq!(
        repaired.policies["bob~first"].review,
        content::Review::Blocked
    );
    assert!(repaired.policies["bob~first"].featured);
    assert_eq!(
        repaired.policies["bob~first"].visibility,
        content::Visibility::Secret
    );
    assert_eq!(
        repaired.policies["bob~first"].password_hash.as_deref(),
        Some("administrator-hash")
    );
    assert!(!repaired.is_public(&repaired.posts["bob~first"]));
    assert_eq!(
        repaired.posts["bob~first"].meta.updated,
        first.posts["bob~first"].meta.updated
    );
    let removed = commit(&bob, Some(&fixed), &[("index.toml", "100644", &b_index)]).await;
    content::git(&bob, &["update-ref", "refs/heads/main", &removed], None)
        .await
        .unwrap();
    let next = git_store::import_authors(&data, &ingress, &repaired, &names)
        .await
        .unwrap();
    assert!(!next.posts.contains_key("bob~first"));
    assert_eq!(next.policies["bob~first"].review, content::Review::Blocked);
    assert!(next.posts.contains_key("alice~first"));
    let rebuilt = git_store::import_authors(&data, &ingress, &Snapshot::default(), &names)
        .await
        .unwrap();
    assert_eq!(rebuilt.commit, next.commit);
    assert_eq!(rebuilt.posts.len(), 1);
}
async fn test_store() -> accounts_db::Store {
    let address = std::env::var("CHUBLOG_TEST_DB_ADDRESS")
        .expect("set CHUBLOG_TEST_DB_ADDRESS for real database tests");
    let password = std::env::var("CHUBLOG_TEST_DB_PASSWORD").expect("set CHUBLOG_TEST_DB_PASSWORD");
    let name = format!("blog_test_{:016x}", rand::random::<u64>());
    let root = chusql_driver::Client::connect(&address, "root", &password)
        .await
        .unwrap();
    root.query(&format!("CREATE DATABASE {name}"))
        .await
        .unwrap();
    root.close().await;
    accounts_db::Store {
        database: database::Database {
            address,
            name,
            user: "root".into(),
            password,
        },
    }
}
async fn app(dir: &std::path::Path) -> Arc<App> {
    content::init(dir).await.unwrap();
    let store = test_store().await;
    let db = store.database.connect().await.unwrap();
    database::ensure_schema(&db, database::ACCOUNTS_SQL)
        .await
        .unwrap();
    let hash = accounts::hash_password("Administrator-test-123".into())
        .await
        .unwrap();
    db.close().await;
    store
        .install(
            &setup::Site {
                name: "Test blog".into(),
                description: "Test description".into(),
            },
            "admin",
            "管理员",
            &hash,
        )
        .await
        .unwrap();
    let accounts = accounts::load(store).await.unwrap();
    let published = test_post("alice-public", true, "Hello");
    let draft = test_post("alice-secret", false, "Secret");
    Arc::new(App {
        data: dir.into(),
        repository: dir.join("ingress.git"),
        registration: true,
        setup_token: "test-install-token".into(),
        site: Mutex::new(None),
        authors: RwLock::new(BTreeMap::from([(
            "alice".into(),
            accounts_db::AuthorDisplay {
                nickname: "爱丽丝".into(),
                is_admin: false,
            },
        )])),
        accounts: Mutex::new(accounts),
        hash_slots: Arc::new(Semaphore::new(2)),
        git_slots: Arc::new(Semaphore::new(1)),
        article_changes: Mutex::new(()),
        policy_projection_pending: AtomicBool::new(true),
        snapshot: RwLock::new(Snapshot {
            commit: "test".into(),
            revisions: BTreeMap::new(),
            pending: BTreeMap::new(),
            policies: BTreeMap::new(),
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
        .header("origin", "http://localhost:4321")
        .header("host", "localhost:4321");
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
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn empty_accounts_start_and_first_user_remains_reader_after_reload() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let store = accounts::store(&app).await;
    let db = store.database.connect().await.unwrap();
    db.query("DELETE FROM blog_users").await.unwrap();
    db.close().await;
    *app.accounts.lock().await = accounts::load(store.clone()).await.unwrap();
    assert!(store.users().await.unwrap().is_empty());
    let (status, _, value) = request(
        app.clone(),
        "POST",
        "/api/auth/register",
        json!({"nickname":"测试昵称", "username":"first", "password":"First-user-password-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["role"], "reader");
    *app.accounts.lock().await = accounts::load(accounts::store(&app).await).await.unwrap();
    let (status, headers, value) = request(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({"username":"first", "password":"First-user-password-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(accounts::store(&app).await.users().await.unwrap().len(), 1);
    assert_eq!(
        accounts::store(&app)
            .await
            .user("first")
            .await
            .unwrap()
            .unwrap()
            .level,
        accounts_db::Level::Member
    );
    let cookie = headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let (status, _, _) = request(
        app,
        "GET",
        "/api/admin/users",
        json!(null),
        Some(cookie),
        value["csrf"].as_str(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn registration_login_grants_revocation_and_private_content_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let (status, _, _) = request(
        app.clone(),
        "POST",
        "/api/auth/register",
        json!({"nickname":"测试昵称", "username":"alice", "password":"Alice-test-password-123", "role":"admin"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _, value) = request(
        app.clone(),
        "POST",
        "/api/auth/register",
        json!({"nickname":"测试昵称", "username":"alice", "password":"Alice-test-password-123"}),
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
        accounts::store(&app)
            .await
            .user("alice")
            .await
            .unwrap()
            .unwrap()
            .level,
        accounts_db::Level::Author
    );
    let user = accounts::store(&app)
        .await
        .user("alice")
        .await
        .unwrap()
        .unwrap();
    assert!(user.hash.starts_with("$argon2id$"));
    assert_ne!(user.hash, "Alice-test-password-123");
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
        json!({"enabled":false,"role":"reader"}),
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
    assert_eq!(status, StatusCode::FORBIDDEN);
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
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
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
    let loaded = accounts::load(accounts::store(&app).await).await.unwrap();
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
        json!({"nickname":"测试昵称", "username":"large", "password":"x".repeat(9000)}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn proxy_origin_validation_and_https_session_cookie() {
    let mut headers = HeaderMap::new();
    headers.insert("host", HeaderValue::from_static("api:3001"));
    headers.insert(
        "x-forwarded-host",
        HeaderValue::from_static("blog.example:8443"),
    );
    headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
    headers.insert(
        "origin",
        HeaderValue::from_static("https://blog.example:8443"),
    );
    assert!(matches!(origin(&headers), Ok(true)));
    for invalid in [
        "https://evil.example",
        "https://blog.example",
        "null",
        "https://blog.example:8443/",
    ] {
        headers.insert("origin", HeaderValue::from_str(invalid).unwrap());
        assert!(origin(&headers).is_err(), "accepted {invalid}");
    }
    headers.insert(
        "origin",
        HeaderValue::from_static("https://blog.example:8443"),
    );
    headers.append("x-forwarded-host", HeaderValue::from_static("evil.example"));
    assert!(origin(&headers).is_err());
    headers.insert(
        "x-forwarded-host",
        HeaderValue::from_static("blog.example:8443"),
    );
    headers.insert("x-forwarded-proto", HeaderValue::from_static("https,http"));
    assert!(origin(&headers).is_err());
    headers.insert("x-forwarded-proto", HeaderValue::from_static("http"));
    headers.insert(
        "origin",
        HeaderValue::from_static("http://blog.example:8443"),
    );
    assert!(origin(&headers).is_err(), "public login must require HTTPS");
    headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
    headers.insert(
        "origin",
        HeaderValue::from_static("https://blog.example:8443"),
    );
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    let dir = tempfile::tempdir().unwrap();
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .body(Body::from(
            json!({"username":"admin","password":"Administrator-test-123"}).to_string(),
        ))
        .unwrap();
    *req.headers_mut() = headers;
    let response = router(app(dir.path()).await).oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .ends_with("; Secure")
    );
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn git_http_requires_basic_credentials_and_independent_publication_grant() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let uri = "/git/content.git/info/refs?service=git-upload-pack";
    let call = |auth: Option<String>, path: &str| {
        let mut builder = Request::builder()
            .uri(path)
            .header("host", "localhost:1741");
        if let Some(auth) = auth {
            builder = builder.header("authorization", auth);
        }
        router(app.clone()).oneshot(builder.body(Body::empty()).unwrap())
    };
    let response = call(None, uri).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        response.headers()[header::WWW_AUTHENTICATE]
            .to_str()
            .unwrap()
            .starts_with("Basic ")
    );
    assert_eq!(
        call(Some("Basic invalid".into()), uri)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let basic = |name: &str, password: &str| {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{name}:{password}"))
        )
    };
    assert_eq!(
        call(Some(basic("admin", "Incorrect-password-123")), uri)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(
        accounts::git_credentials(&app, "admin", "Administrator-test-123".into())
            .await
            .is_ok()
    );
    let response = call(Some(basic("admin", "Administrator-test-123")), uri)
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = router(app.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, service).await.unwrap();
    });
    let clone = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        tokio::process::Command::new("git")
            .args([
                "-c",
                &format!(
                    "http.extraHeader=Authorization: {}",
                    basic("admin", "Administrator-test-123")
                ),
                "clone",
                &format!("http://{address}/git/content.git"),
            ])
            .arg(dir.path().join("clone"))
            .env("GIT_TERMINAL_PROMPT", "0")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    server.abort();
    assert!(
        clone.status.success(),
        "{}",
        String::from_utf8_lossy(&clone.stderr)
    );
    assert!(dir.path().join("clone/posts/publishing/main.md").is_file());
    let (status, _, _) = request(
        app.clone(),
        "POST",
        "/api/auth/register",
        json!({"nickname":"测试昵称", "username":"reader","password":"Reader-password-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(Some(basic("reader", "Reader-password-123")), uri)
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            Some(basic("admin", "Administrator-test-123")),
            "/git/reader.git/info/refs?service=git-upload-pack"
        )
        .await
        .unwrap()
        .status(),
        StatusCode::NOT_FOUND
    );
    let response = router(app)
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("host", "blog.example")
                .header("authorization", basic("admin", "Administrator-test-123"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "public HTTP must not accept Git credentials"
    );
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn enum_accounts_constraints_persistence_and_index_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let store = accounts::store(&app).await;
    let db = store.database.connect().await.unwrap();
    // Initialization is safe to repeat without replacing account data or the type.
    database::ensure_schema(&db, database::ACCOUNTS_SQL)
        .await
        .unwrap();
    let admin = store.user("admin").await.unwrap().unwrap();
    assert_eq!(admin.level, accounts_db::Level::Master);
    let hash = accounts::hash_password("Enum-test-password-123".into())
        .await
        .unwrap();
    let member = store.register("enumuser", "昵称", &hash).await.unwrap();
    assert!(member.id > 0 && member.id != admin.id);
    assert_eq!(member.view()["permission_level"], "MEMBER");
    assert!(member.last_login_at.is_none());
    assert!(member.view().get("password_hash").is_none());
    let logged = store.logged_in(&member).await.unwrap();
    assert!(logged.last_login_at.is_some());
    for level in [
        accounts_db::Level::Author,
        accounts_db::Level::Banned,
        accounts_db::Level::Member,
    ] {
        store.grant(&admin, "enumuser", level).await.unwrap();
        let user = store.user("enumuser").await.unwrap().unwrap();
        assert_eq!(user.level, level);
        assert_eq!(user.view()["permission_level"], level.as_str());
    }
    for value in ["0", "1", "'reader'", "'member'", "'UNKNOWN'", "NULL"] {
        assert!(
            db.query(&format!(
                "UPDATE blog_users SET permission_level = {value} WHERE username = 'enumuser'"
            ))
            .await
            .is_err(),
            "accepted invalid permission: {value}"
        );
    }
    assert!(db.query("DROP TYPE blog_permission").await.is_err());
    index::rebuild_with_db(&Snapshot::default(), db)
        .await
        .unwrap();
    let persisted = store.user("enumuser").await.unwrap().unwrap();
    assert_eq!(persisted.id, member.id);
    assert_eq!(persisted.hash, hash);
    assert_eq!(persisted.created_at, member.created_at);
    assert_eq!(persisted.last_login_at, logged.last_login_at);
    assert_eq!(store.users().await.unwrap().len(), 2);

    app.accounts.lock().await.store.database.address = "127.0.0.1:9".into();
    let (status, _, _) = request(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({"username":"admin","password":"Administrator-test-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let (status, _, _) = request(app, "GET", "/api/posts", Value::Null, None, None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn first_install_requires_token_is_atomic_and_creates_only_one_master() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let store = accounts::store(&app).await;
    let db = store.database.connect().await.unwrap();
    db.query("DELETE FROM blog_site").await.unwrap();
    db.query("DELETE FROM blog_users").await.unwrap();
    let (_, _, state) = request(app.clone(), "GET", "/api/site", Value::Null, None, None).await;
    assert_eq!(state["installed"], false);
    let (status, _, _) = request(
        app.clone(),
        "POST",
        "/api/auth/register",
        json!({"nickname":"测试昵称", "username":"early","password":"Early-password-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let mut input = json!({"token":"wrong", "nickname":"管理员", "name":"我的博客", "description":"文章与随笔", "username":"owner", "password":"Owner-password-123"});
    let (status, _, _) =
        request(app.clone(), "POST", "/api/setup", input.clone(), None, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(store.users().await.unwrap().is_empty());
    input["token"] = json!("test-install-token");
    let (status, _, _) =
        request(app.clone(), "POST", "/api/setup", input.clone(), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!store.starters_pending().await.unwrap());
    let site = db
        .query("SELECT master_user_id, starter_commit FROM blog_site WHERE id = 1")
        .await
        .unwrap();
    let owner_id = store.user("owner").await.unwrap().unwrap().id;
    assert_eq!(site.rows[0][0], json!(owner_id));
    assert_eq!(site.rows[0][1].as_str().unwrap().len(), 40);
    let repo = git_store::author_repository(&app.repository, "owner").unwrap();
    assert!(repo.is_dir());
    let seeded = git_store::import_authors(
        &app.data,
        &app.repository,
        &Snapshot::default(),
        &["owner".into()],
    )
    .await
    .unwrap();
    assert_eq!(seeded.posts.len(), 2);
    assert!(seeded.posts.values().all(|post| post.author == "owner"));
    index::rebuild_with_db(&seeded, store.database.connect().await.unwrap())
        .await
        .unwrap();
    let indexed = db
        .query("SELECT author, format, status, created_at, published_at FROM blog_articles")
        .await
        .unwrap();
    assert_eq!(indexed.rows.len(), 2);
    for row in &indexed.rows {
        assert_eq!(
            &row[..3],
            &[json!("owner"), json!("markdown"), json!("published")]
        );
        assert!(database::from_timestamp(row[3].as_str().unwrap()).is_ok());
        assert!(!row[4].is_null());
    }
    let mut drafts = seeded.clone();
    for post in drafts.posts.values_mut() {
        post.meta.status = Status::Draft;
        post.meta.date.clear();
    }
    index::rebuild_with_db(&drafts, store.database.connect().await.unwrap())
        .await
        .unwrap();
    let unpublished = db
        .query("SELECT published_at, cover FROM blog_articles")
        .await
        .unwrap();
    assert!(
        unpublished
            .rows
            .iter()
            .all(|row| row[0].is_null() && row[1].is_null())
    );
    for query in [
        format!(
            "UPDATE blog_users SET username = '{}' WHERE username = 'owner'",
            "x".repeat(33)
        ),
        "UPDATE blog_users SET created_at = '2026-01-01T00:00:00Z' WHERE username = 'owner'".into(),
        "UPDATE blog_articles SET format = 'html'".into(),
    ] {
        assert!(db.query(&query).await.is_err());
    }

    let owner = store.user("owner").await.unwrap().unwrap();
    assert_eq!(owner.level, accounts_db::Level::Master);
    assert_eq!(owner.nickname, "管理员");
    assert_ne!(owner.hash, "Owner-password-123");
    assert_eq!(store.site().await.unwrap().unwrap().name, "我的博客");
    let before = owner.hash.clone();
    input["password"] = json!("Replacement-password-123");
    let (status, _, _) = request(app.clone(), "POST", "/api/setup", input, None, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(store.user("owner").await.unwrap().unwrap().hash, before);
    let member = store.register("member", "昵称", &before).await.unwrap();
    assert!(
        store
            .grant(&owner, &member.username, accounts_db::Level::Master)
            .await
            .is_err()
    );
    assert!(
        store
            .grant(&owner, "owner", accounts_db::Level::Member)
            .await
            .is_err()
    );
    let (status, _, value) = request(
        app,
        "POST",
        "/api/auth/login",
        json!({"username":"owner","password":"Owner-password-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["permission_level"], "MASTER");
    assert_eq!(
        store
            .users()
            .await
            .unwrap()
            .iter()
            .filter(|user| user.level == accounts_db::Level::Master)
            .count(),
        1
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn concurrent_installation_commits_one_site_and_one_master() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let store = accounts::store(&app).await;
    let db = store.database.connect().await.unwrap();
    db.query("DELETE FROM blog_site").await.unwrap();
    db.query("DELETE FROM blog_users").await.unwrap();
    let first = json!({"token":"test-install-token", "nickname":"管理员", "name":"First", "description":"", "username":"first", "password":"First-password-123"});
    let second = json!({"token":"test-install-token", "nickname":"管理员", "name":"Second", "description":"", "username":"second", "password":"Second-password-123"});
    let (a, b) = tokio::join!(
        request(app.clone(), "POST", "/api/setup", first, None, None),
        request(app.clone(), "POST", "/api/setup", second, None, None)
    );
    assert_eq!(
        [a.0, b.0]
            .into_iter()
            .filter(|status| *status == StatusCode::OK)
            .count(),
        1
    );
    let users = store.users().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].level, accounts_db::Level::Master);
    let site = db
        .query("SELECT master_user_id FROM blog_site WHERE id = 1")
        .await
        .unwrap();
    assert_eq!(site.rows, vec![vec![json!(users[0].id)]]);
    db.close().await;
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn deployment_provisions_enums_as_root_and_business_data_as_regular_user() {
    let address = std::env::var("CHUBLOG_TEST_DB_ADDRESS").unwrap();
    let root_password = std::env::var("CHUBLOG_TEST_DB_PASSWORD").unwrap();
    let password = format!("Application-test-{:016x}", rand::random::<u64>());
    index::provision_at(&address, &root_password, &password)
        .await
        .unwrap();
    index::provision_at(&address, &root_password, &password)
        .await
        .unwrap();
    let store = accounts_db::Store {
        database: database::Database {
            address,
            name: "chublog".into(),
            user: "chublog_index".into(),
            password,
        },
    };
    let db = store.database.connect().await.unwrap();
    let error = db
        .query("CREATE TYPE forbidden_admin_type AS ENUM ('test')")
        .await
        .unwrap_err();
    assert!(matches!(error, chusql_driver::Error::Server { ref code, .. } if code == "forbidden"));
    database::ensure_schema(&db, database::ACCOUNTS_SQL)
        .await
        .unwrap();
    db.query("ALTER TABLE blog_users DROP COLUMN nickname")
        .await
        .unwrap();
    db.close().await;
    index::provision_at(
        &store.database.address,
        &root_password,
        &store.database.password,
    )
    .await
    .unwrap();
    let db = store.database.connect().await.unwrap();
    assert!(
        db.query("SELECT * FROM blog_users LIMIT 0")
            .await
            .unwrap()
            .columns
            .contains(&"nickname".into())
    );
    let hash = accounts::hash_password("Owner-test-password-123".into())
        .await
        .unwrap();
    store
        .install(
            &setup::Site {
                name: "Deployment test".into(),
                description: String::new(),
            },
            "owner",
            "管理员",
            &hash,
        )
        .await
        .unwrap();
    let owner = store.owner().await.unwrap().unwrap();
    assert_eq!(owner.level, accounts_db::Level::Master);
    store.register("member", "昵称", &hash).await.unwrap();
    store
        .grant(&owner, "member", accounts_db::Level::Author)
        .await
        .unwrap();
    assert_eq!(
        store.user("member").await.unwrap().unwrap().level,
        accounts_db::Level::Author
    );
    index::rebuild_with_db(&Snapshot::default(), db)
        .await
        .unwrap();
    assert_eq!(store.owner().await.unwrap().unwrap().id, owner.id);
    index::provision_at(
        &store.database.address,
        &root_password,
        &store.database.password,
    )
    .await
    .unwrap();
    assert_eq!(store.owner().await.unwrap().unwrap().id, owner.id);
}

#[test]
fn usernames_allow_safe_ascii_identifiers_and_nicknames_are_utf8_attributes() {
    for name in [
        "alice",
        "alice1",
        "alice-name",
        "user_1",
        "_abc",
        "-abc",
        "a12",
    ] {
        assert!(accounts::username(name).is_ok(), "rejected {name}");
    }
    for name in [
        "Alice",
        "1alice",
        "123",
        "a.b",
        "a/b",
        "用户名",
        "ab",
        "alice';drop",
    ] {
        assert!(accounts::username(name).is_err(), "accepted {name}");
    }
    for name in [
        "管理员 😺",
        "O'Reilly",
        "昵称\n\t\0",
        "<script>text</script>",
    ] {
        assert!(accounts::nickname(name).is_ok());
    }
    assert!(accounts::nickname("").is_err());
    assert!(accounts::nickname(&"界".repeat(43)).is_err());
    assert!(accounts::nickname(&"界".repeat(42)).is_ok());
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn duplicate_utf8_nicknames_and_sql_payloads_remain_plain_data() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let nickname = "😺'); DROP TABLE blog_users; -- \"\\\n\t\0";
    for username in ["first", "second"] {
        let (status, _, view) = request(app.clone(), "POST", "/api/auth/register", json!({"username":username,"nickname":nickname,"password":"Nickname-test-password-123"}), None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(view["nickname"], nickname);
        assert_eq!(view["username"], username);
    }
    let (status, _, _) = request(
        app.clone(),
        "POST",
        "/api/auth/register",
        json!({"username":"first","nickname":"另一个昵称","password":"Nickname-test-password-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, headers, view) = request(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({"username":"first","password":"Nickname-test-password-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["nickname"], nickname);
    let cookie = headers
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let (status, _, view) = request(
        app.clone(),
        "GET",
        "/api/account",
        Value::Null,
        Some(cookie),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["nickname"], nickname);
    let users = accounts::store(&app).await.users().await.unwrap();
    assert_eq!(users.len(), 3);
    assert_ne!(
        users.iter().find(|u| u.username == "first").unwrap().id,
        users.iter().find(|u| u.username == "second").unwrap().id
    );
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn partial_install_reports_account_conflict_without_masking_as_database_outage() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let db = accounts::store(&app)
        .await
        .database
        .connect()
        .await
        .unwrap();
    db.query("DELETE FROM blog_site").await.unwrap();
    let (status, _, view) = request(app, "POST", "/api/setup", json!({"token":"test-install-token","name":"博客","description":"","username":"newowner","nickname":"新管理员","password":"Owner-test-password-123"}), None, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(view["error"].as_str().unwrap().contains("账户表已有记录"));
    assert_eq!(
        db.query("SELECT count(*) FROM blog_users")
            .await
            .unwrap()
            .rows[0][0],
        json!(1)
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn empty_installation_rebuilds_current_schema_but_preserves_existing_accounts() {
    let store = test_store().await;
    let db = store.database.connect().await.unwrap();
    let old_schema = database::ACCOUNTS_SQL
        .replace("    nickname VARCHAR(128) NOT NULL,\n", "")
        .replace(
            "permission_level blog_permission NOT NULL DEFAULT 'MEMBER'",
            "permission_level INT NOT NULL DEFAULT 1",
        );
    database::ensure_schema(&db, &old_schema).await.unwrap();
    // Match the server after nickname was added while permission_level remained INT.
    db.query("ALTER TABLE blog_users ADD COLUMN nickname VARCHAR(128) NOT NULL")
        .await
        .unwrap();
    database::complete_empty_install(&db).await.unwrap();
    database::ensure_schema(&db, database::ACCOUNTS_SQL)
        .await
        .unwrap();
    database::complete_empty_install(&db).await.unwrap();
    database::ensure_schema(&db, database::ACCOUNTS_SQL)
        .await
        .unwrap();
    let hash = accounts::hash_password("Setup-repair-password-123".into())
        .await
        .unwrap();
    store
        .install(
            &setup::Site {
                name: "博客".into(),
                description: String::new(),
            },
            "owner",
            "管理员 😺",
            &hash,
        )
        .await
        .unwrap();
    assert_eq!(store.owner().await.unwrap().unwrap().nickname, "管理员 😺");
    database::complete_empty_install(&db).await.unwrap();
    database::ensure_schema(&db, database::ACCOUNTS_SQL)
        .await
        .unwrap();
    assert_eq!(store.owner().await.unwrap().unwrap().nickname, "管理员 😺");
    db.close().await;

    let store = test_store().await;
    let db = store.database.connect().await.unwrap();
    database::ensure_schema(&db, &old_schema).await.unwrap();
    let query = chusql_driver::Query::new("INSERT INTO blog_users (username, password_hash, created_at, permission_level) VALUES ('existing', ")
        .bind(hash.as_str()).push(", '2026-01-01 00:00:00', 1)");
    db.query(query.as_str()).await.unwrap();
    assert!(database::complete_empty_install(&db).await.is_err());
    assert_eq!(
        db.query("SELECT count(*) FROM blog_users")
            .await
            .unwrap()
            .rows[0][0],
        json!(1)
    );
    assert!(
        !db.query("SELECT * FROM blog_users LIMIT 0")
            .await
            .unwrap()
            .columns
            .contains(&"nickname".into())
    );
    db.close().await;
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn public_site_metadata_is_cached_while_account_checks_remain_live() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let (status, _, initial) =
        request(app.clone(), "GET", "/api/site", json!(null), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(initial["owner"], "admin");
    // Break new connections after warming public metadata, keeping authorization fail-closed.
    app.accounts.lock().await.store.database.address = "127.0.0.1:0".into();
    let (status, _, cached) =
        request(app.clone(), "GET", "/api/site", json!(null), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cached, initial);
    assert!(
        accounts::git_credentials(&app, "admin", "Administrator-test-123".into())
            .await
            .is_err()
    );
}

#[test]
fn author_display_uses_nicknames_without_changing_identity() {
    let post = test_post("alice~first", true, "hello");
    let mut names = BTreeMap::from([(
        "alice".into(),
        accounts_db::AuthorDisplay {
            nickname: "同名昵称".into(),
            is_admin: false,
        },
    )]);
    let meta = display_metadata(&post.meta, &names);
    assert_eq!(meta["author"], "alice");
    assert_eq!(meta["author_name"], "同名昵称");
    assert_eq!(meta["author_is_admin"], false);
    names.get_mut("alice").unwrap().is_admin = true;
    assert_eq!(
        display_metadata(&post.meta, &names)["author_is_admin"],
        true
    );
    assert_eq!(
        display_metadata(&post.meta, &BTreeMap::new())["author_is_admin"],
        false
    );
    assert_eq!(
        display_metadata(&post.meta, &BTreeMap::new())["author_name"],
        "已注销用户"
    );
}

#[tokio::test]
async fn article_directories_are_independent_of_ids_and_repository_location() {
    let dir = tempfile::tempdir().unwrap();
    let repo = content::init(dir.path()).await.unwrap();
    let index = format!(
        "{}{}",
        manifest("development", "notes/rust/core", true),
        manifest("writing", "中文/随笔", true)
    );
    let commit = commit(
        &repo,
        None,
        &[
            ("index.toml", "100644", &index),
            (
                "notes/rust/core/main.md",
                "100644",
                "Development [next](chapter.md)",
            ),
            ("notes/rust/core/chapter.md", "100644", "Chapter"),
            ("中文/随笔/main.md", "100644", "随笔"),
            ("unlisted/notes.md", "100644", "Unlisted"),
        ],
    )
    .await;
    let snapshot = ready(content::scan_at(&repo, &commit).await.unwrap());
    assert_eq!(snapshot.posts.len(), 2);
    assert_eq!(
        snapshot.posts["development"].path,
        "notes/rust/core/main.md"
    );
    assert_eq!(snapshot.posts["development"].pages["chapter.md"], "Chapter");
    assert_eq!(snapshot.posts["writing"].markdown, "随笔");
}

#[test]
fn article_directory_validation_rejects_aliases_hidden_and_escaping_paths() {
    for folder in [
        ".",
        "/absolute",
        "../escape",
        "notes/../escape",
        "notes/.private",
        "notes//draft",
        "notes/./draft",
        "C:/notes",
        "notes\\draft",
        "",
        ".git",
        "notes/\u{202e}hidden",
    ] {
        assert!(
            content::manifest(&manifest("first", folder, true)).is_err(),
            "accepted {folder}"
        );
    }
    for folder in ["notes", "notes/rust/core", "我的文章/随笔", "my notes/2026"] {
        assert!(
            content::manifest(&manifest("first", folder, true)).is_ok(),
            "rejected {folder}"
        );
    }
    assert!(content::manifest(&manifest("first", &"a".repeat(92), true)).is_err());
}

#[tokio::test]
async fn article_pages_require_unique_names_without_extensions() {
    let dir = tempfile::tempdir().unwrap();
    let repo = content::init(dir.path()).await.unwrap();
    let index = manifest("first", "notes", true);
    let commit = commit(
        &repo,
        None,
        &[
            ("index.toml", "100644", &index),
            (
                "notes/main.md",
                "100644",
                "[One](chapter.md) [Two](chapter.txt)",
            ),
            ("notes/chapter.md", "100644", "One"),
            ("notes/chapter.txt", "100644", "Two"),
        ],
    )
    .await;
    assert!(
        matches!(content::scan_at(&repo, &commit).await.unwrap(), content::Scan::Pending(reason) if reason.contains("名称需要唯一"))
    );
}

#[tokio::test]
async fn moderation_excludes_restricted_articles_and_password_hashes_from_public_export() {
    use content::{ArticlePolicy, Hidden, Review, Visibility};
    let dir = tempfile::tempdir().unwrap();
    let mut snapshot = Snapshot::default();
    for (id, visibility, review) in [
        ("public", Visibility::Default, Review::Open),
        ("secret", Visibility::Secret, Review::Open),
        ("top", Visibility::Top, Review::Open),
        ("blocked", Visibility::Default, Review::Blocked),
    ] {
        let post = test_post(id, true, &format!("body-{id}"));
        snapshot.posts.insert(id.into(), post);
        snapshot.policies.insert(
            id.into(),
            ArticlePolicy {
                featured: true,
                visibility,
                hidden: Hidden::Full,
                review,
                blocked_hash: None,
                password_hash: Some("private-hash".into()),
            },
        );
    }
    content::persist(dir.path(), &snapshot).await.unwrap();
    let bytes = std::fs::read(dir.path().join("published.json")).unwrap();
    let public: Snapshot = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(public.posts.keys().collect::<Vec<_>>(), vec!["public"]);
    assert!(public.policies.is_empty());
    assert!(!String::from_utf8(bytes).unwrap().contains("private-hash"));
    let meta = moderation::metadata(&snapshot, &snapshot.posts["secret"], &BTreeMap::new());
    assert_eq!(meta["visibility"], "secret");
    assert_eq!(meta["featured"], true);
    assert_eq!(meta["password_required"], true);
    assert!(!meta.to_string().contains("private-hash"));
    let decoded: Snapshot =
        serde_json::from_slice(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    assert_eq!(decoded.policies["blocked"].review, Review::Blocked);
    assert_eq!(
        decoded.policies["secret"].password_hash.as_deref(),
        Some("private-hash")
    );
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn article_moderation_checks_roles_csrf_passwords_pages_assets_and_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    {
        let mut snapshot = app.snapshot.write().await;
        snapshot
            .posts
            .get_mut("alice-public")
            .unwrap()
            .assets
            .push(content::Asset {
                name: "test.png".into(),
                data: "aGVsbG8=".into(),
            });
    }
    let (status, headers, admin) = request(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({"username":"admin", "password":"Administrator-test-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let csrf = admin["csrf"].as_str();
    let path = "/api/admin/articles/alice-public";
    let settings =
        json!({"featured":true, "visibility":"confidential", "password":"Reading-secret-123"});
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            path,
            settings.clone(),
            Some(cookie),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(app.clone(), "PATCH", path, settings, Some(cookie), csrf)
            .await
            .0,
        StatusCode::OK
    );
    for url in [
        "/api/posts/alice-public",
        "/api/posts/alice-public/pages/main.md",
        "/api/assets/alice-public/test.png",
    ] {
        assert_eq!(
            request(app.clone(), "GET", url, json!(null), None, None)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(app.clone(), "GET", url, json!(null), Some(cookie), None)
                .await
                .0,
            StatusCode::OK
        );
    }
    let (_, _, public) = request(app.clone(), "GET", "/api/posts", json!(null), None, None).await;
    assert!(public["posts"].as_array().unwrap().is_empty());
    let hash = accounts::hash_password("Reader-password-123".into())
        .await
        .unwrap();
    accounts::store(&app)
        .await
        .register("reader", "读者", &hash)
        .await
        .unwrap();
    let (_, reader_headers, reader) = request(
        app.clone(),
        "POST",
        "/api/auth/login",
        json!({"username":"reader", "password":"Reader-password-123"}),
        None,
        None,
    )
    .await;
    let reader_cookie = reader_headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let reader_csrf = reader["csrf"].as_str();
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            path,
            json!({"featured":false,"visibility":"public"}),
            Some(reader_cookie),
            reader_csrf
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, _, list) = request(
        app.clone(),
        "GET",
        "/api/posts",
        json!(null),
        Some(reader_cookie),
        None,
    )
    .await;
    assert!(
        list["posts"].as_array().unwrap().is_empty(),
        "机密文章不进入普通读者列表"
    );
    for url in [
        "/api/posts/alice-public",
        "/api/posts/alice-public/pages/main.md",
        "/api/assets/alice-public/test.png",
    ] {
        assert_eq!(
            request(
                app.clone(),
                "GET",
                url,
                json!(null),
                Some(reader_cookie),
                None
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    let unlock = "/api/posts/alice-public/unlock";
    assert_eq!(
        request(
            app.clone(),
            "POST",
            unlock,
            json!({"password":"Wrong-password"}),
            Some(reader_cookie),
            reader_csrf
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            app.clone(),
            "POST",
            unlock,
            json!({"password":"Reading-secret-123"}),
            Some(reader_cookie),
            reader_csrf
        )
        .await
        .0,
        StatusCode::OK
    );
    for url in [
        "/api/posts/alice-public",
        "/api/posts/alice-public/pages/main.md",
        "/api/assets/alice-public/test.png",
    ] {
        assert_eq!(
            request(
                app.clone(),
                "GET",
                url,
                json!(null),
                Some(reader_cookie),
                None
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            path,
            json!({"featured":true,"visibility":"confidential","password":"Changed-secret-123"}),
            Some(cookie),
            csrf
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/api/posts/alice-public",
            json!(null),
            Some(reader_cookie),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            path,
            json!({"featured":true,"visibility":"secret"}),
            Some(cookie),
            csrf
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/api/posts/alice-public",
            json!(null),
            Some(reader_cookie),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            path,
            json!({"featured":true,"visibility":"top"}),
            Some(cookie),
            csrf
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/api/posts/alice-public",
            json!(null),
            Some(reader_cookie),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(app.clone(), "DELETE", path, json!(null), Some(cookie), csrf)
            .await
            .0,
        StatusCode::OK
    );
    // Blocked articles stay visible to their author and administrators, but are gone for everyone else.
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/api/posts/alice-public",
            json!(null),
            Some(cookie),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/api/posts/alice-public",
            json!(null),
            Some(reader_cookie),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let saved: Snapshot =
        serde_json::from_slice(&std::fs::read(dir.path().join("collection.json")).unwrap())
            .unwrap();
    assert_eq!(
        saved.policies["alice-public"].review,
        content::Review::Blocked
    );
    let public: Snapshot =
        serde_json::from_slice(&std::fs::read(dir.path().join("published.json")).unwrap()).unwrap();
    assert!(public.posts.is_empty());
}

#[test]
fn semi_hidden_secret_articles_expose_metadata_but_full_hidden_ones_do_not() {
    let mut snapshot = Snapshot::default();
    for (key, hidden) in [
        ("semi", content::Hidden::Semi),
        ("full", content::Hidden::Full),
    ] {
        snapshot
            .posts
            .insert(key.into(), test_post(key, true, "restricted body"));
        snapshot.policies.insert(
            key.into(),
            content::ArticlePolicy {
                visibility: content::Visibility::Secret,
                hidden,
                ..Default::default()
            },
        );
    }
    let access = std::collections::BTreeSet::new();
    assert!(moderation::listed(
        &snapshot,
        &snapshot.posts["semi"],
        None,
        &access
    ));
    assert!(!moderation::listed(
        &snapshot,
        &snapshot.posts["full"],
        None,
        &access
    ));
}

#[test]
fn legacy_article_policies_migrate_to_the_new_enums() {
    let password_protected: content::ArticlePolicy = serde_json::from_str(
        r#"{"featured":false,"visibility":"internal","deleted":false,"password_hash":"legacy"}"#,
    )
    .unwrap();
    assert_eq!(
        password_protected.visibility,
        content::Visibility::Confidential
    );
    assert_eq!(password_protected.hidden, content::Hidden::Full);
    assert_eq!(password_protected.review, content::Review::Open);
    assert_eq!(password_protected.password_hash.as_deref(), Some("legacy"));

    let removed: content::ArticlePolicy =
        serde_json::from_str(r#"{"visibility":"hidden","deleted":true}"#).unwrap();
    assert_eq!(removed.visibility, content::Visibility::Top);
    assert_eq!(removed.review, content::Review::Blocked);
    let encoded = serde_json::to_value(&removed).unwrap();
    assert_eq!(encoded["visibility"], "top");
    assert_eq!(encoded["review"], "blocked");
    assert!(encoded.get("deleted").is_none());
}

#[tokio::test]
async fn anonymous_routes_enforce_moderation_without_a_database_connection() {
    use content::{ArticlePolicy, Hidden, Review, Visibility};
    let dir = tempfile::tempdir().unwrap();
    let store = accounts_db::Store {
        database: database::Database {
            address: "127.0.0.1:1".into(),
            name: "unused".into(),
            user: "unused".into(),
            password: "unused".into(),
        },
    };
    let mut snapshot = Snapshot::default();
    for (key, visibility, review, published) in [
        ("public", Visibility::Default, Review::Open, true),
        ("secret", Visibility::Secret, Review::Open, true),
        ("confidential", Visibility::Confidential, Review::Open, true),
        ("top", Visibility::Top, Review::Open, true),
        ("blocked", Visibility::Default, Review::Blocked, true),
        ("draft", Visibility::Secret, Review::Open, false),
    ] {
        let mut post = test_post(key, published, "Private or public body");
        post.assets.push(content::Asset {
            name: "test.png".into(),
            data: "aGVsbG8=".into(),
        });
        snapshot.posts.insert(key.into(), post);
        snapshot.policies.insert(
            key.into(),
            ArticlePolicy {
                visibility,
                hidden: Hidden::Full,
                review,
                ..Default::default()
            },
        );
    }
    let app = Arc::new(App {
        data: dir.path().into(),
        repository: dir.path().join("ingress"),
        registration: false,
        setup_token: String::new(),
        site: Mutex::new(None),
        authors: RwLock::new(BTreeMap::new()),
        accounts: Mutex::new(accounts::load(store).await.unwrap()),
        hash_slots: Arc::new(Semaphore::new(2)),
        git_slots: Arc::new(Semaphore::new(1)),
        snapshot: RwLock::new(snapshot),
        article_changes: Mutex::new(()),
        policy_projection_pending: AtomicBool::new(true),
        indexed: RwLock::new(None),
        views: AtomicU64::new(0),
    });
    for (key, expected) in [
        ("public", StatusCode::OK),
        ("secret", StatusCode::UNAUTHORIZED),
        ("confidential", StatusCode::UNAUTHORIZED),
        ("top", StatusCode::FORBIDDEN),
        ("blocked", StatusCode::NOT_FOUND),
        ("draft", StatusCode::NOT_FOUND),
    ] {
        for path in [
            format!("/api/posts/{key}"),
            format!("/api/posts/{key}/pages/main.md"),
            format!("/api/assets/{key}/test.png"),
        ] {
            assert_eq!(
                request(app.clone(), "GET", &path, json!(null), None, None)
                    .await
                    .0,
                expected,
                "{path}"
            );
        }
    }
    let (_, _, blocked) = request(
        app.clone(),
        "GET",
        "/api/posts/blocked",
        json!(null),
        None,
        None,
    )
    .await;
    assert_eq!(blocked["error"], "文章已删除");
    let (_, _, draft) = request(
        app.clone(),
        "GET",
        "/api/posts/draft",
        json!(null),
        None,
        None,
    )
    .await;
    assert_eq!(draft["error"], "文章不存在");
    for cookie in [None, Some(""), Some("chublog_session=expired")] {
        let (status, _, list) =
            request(app.clone(), "GET", "/api/posts", json!(null), cookie, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list["posts"].as_array().unwrap().len(), 1);
        assert_eq!(list["posts"][0]["slug"], "public");
    }
    let (_, _, stats) = request(app.clone(), "GET", "/api/stats", json!(null), None, None).await;
    assert_eq!(stats["published_posts"], 1);
    for (method, path, body) in [
        ("GET", "/api/admin/articles", json!(null)),
        (
            "PATCH",
            "/api/admin/articles/public",
            json!({"featured":true,"visibility":"secret"}),
        ),
        ("DELETE", "/api/admin/articles/public", json!(null)),
        (
            "POST",
            "/api/posts/secret/unlock",
            json!({"password":"Secret-123"}),
        ),
    ] {
        assert_eq!(
            request(app.clone(), method, path, body, None, None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
}

async fn article_test_login(app: Arc<App>, username: &str) -> (String, String) {
    let (status, headers, account) = request(
        app,
        "POST",
        "/api/auth/login",
        json!({"username": username, "password": "Administrator-test-123"}),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    (
        headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into(),
        account["csrf"].as_str().unwrap().into(),
    )
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn article_author_permissions_persistent_grants_and_projection_retry() {
    use accounts_db::{AccessSource, Level};
    use content::{Hidden, Review, Visibility};
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let store = accounts::store(&app).await;
    let admin = store.user("admin").await.unwrap().unwrap();
    let hash = accounts::hash_password("Administrator-test-123".into())
        .await
        .unwrap();
    store.register("alice", "作者", &hash).await.unwrap();
    store.grant(&admin, "alice", Level::Author).await.unwrap();
    let reader = store.register("reader", "读者", &hash).await.unwrap();
    let (admin_cookie, admin_csrf) = article_test_login(app.clone(), "admin").await;
    let (author_cookie, author_csrf) = article_test_login(app.clone(), "alice").await;
    {
        let mut snapshot = app.snapshot.write().await;
        let mut own = test_post("admin~own", true, "Admin body");
        own.author = "admin".into();
        own.meta.author = "admin".into();
        snapshot.posts.insert(own.meta.slug.clone(), own);
    }
    let path = "/api/admin/articles/alice-public";
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            "/api/admin/articles/admin~own",
            json!({"hidden":"semi"}),
            Some(&admin_cookie),
            Some(&admin_csrf)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            path,
            json!({"hidden":"semi"}),
            Some(&admin_cookie),
            Some(&admin_csrf)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            "/api/articles/alice-public",
            json!({"visibility":"secret","hidden":"semi"}),
            Some(&author_cookie),
            Some(&author_csrf)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        app.snapshot.read().await.policies["alice-public"].hidden,
        Hidden::Semi
    );
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            "/api/articles/alice-public",
            json!({"featured":true}),
            Some(&author_cookie),
            Some(&author_csrf)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            path,
            json!({"visibility":"top"}),
            Some(&admin_cookie),
            Some(&admin_csrf)
        )
        .await
        .0,
        StatusCode::OK
    );
    let access = "/api/articles/alice-public/access";
    assert_eq!(
        request(
            app.clone(),
            "POST",
            access,
            json!({"username":"reader","allowed":true}),
            Some(&author_cookie),
            Some(&author_csrf)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(
        store
            .has_article_access(reader.id, "alice-public", AccessSource::Granted)
            .await
            .unwrap()
    );
    // Restarting sessions must not lose the database-backed article authorization.
    *app.accounts.lock().await = accounts::load(store.clone()).await.unwrap();
    let (reader_cookie, _) = article_test_login(app.clone(), "reader").await;
    let (admin_cookie, admin_csrf) = article_test_login(app.clone(), "admin").await;
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/api/posts/alice-public",
            json!(null),
            Some(&reader_cookie),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "POST",
            access,
            json!({"username":"reader","allowed":false}),
            Some(&admin_cookie),
            Some(&admin_csrf)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/api/posts/alice-public",
            json!(null),
            Some(&reader_cookie),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            app.clone(),
            "POST",
            access,
            json!({"username":"reader","allowed":true}),
            Some(&admin_cookie),
            Some(&admin_csrf)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            path,
            json!({"visibility":"default"}),
            Some(&admin_cookie),
            Some(&admin_csrf)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(
        !store
            .has_article_access(reader.id, "alice-public", AccessSource::Granted)
            .await
            .unwrap()
    );
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            path,
            json!({"review":"blocked"}),
            Some(&admin_cookie),
            Some(&admin_csrf)
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut snapshot = app.snapshot.read().await.clone();
    assert!(
        !moderation::reopen_modified(&app, &mut snapshot)
            .await
            .unwrap()
    );
    snapshot.posts.get_mut("alice-public").unwrap().markdown = "Modified body".into();
    assert!(
        moderation::reopen_modified(&app, &mut snapshot)
            .await
            .unwrap()
    );
    assert_eq!(snapshot.policies["alice-public"].review, Review::Open);
    assert!(snapshot.policies["alice-public"].blocked_hash.is_none());
    // Simulate a projection outage after the authoritative file and memory save succeeds.
    let db = store.database.connect().await.unwrap();
    db.query("DROP TABLE blog_article_policy").await.unwrap();
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            path,
            json!({"visibility":"secret"}),
            Some(&admin_cookie),
            Some(&admin_csrf)
        )
        .await
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(app.policy_projection_pending.load(Ordering::Relaxed));
    assert_eq!(
        app.snapshot.read().await.policies["alice-public"].visibility,
        Visibility::Secret
    );
    assert!(moderation::retry_policy_projection(&app).await.is_err());
    assert!(app.policy_projection_pending.load(Ordering::Relaxed));
    database::ensure_schema(&db, database::ACCOUNTS_SQL)
        .await
        .unwrap();
    moderation::retry_policy_projection(&app)
        .await
        .unwrap_or_else(|error| panic!("{}", error.1));
    assert!(!app.policy_projection_pending.load(Ordering::Relaxed));
    let rows = db
        .query("SELECT visibility FROM blog_article_policy WHERE article = 'alice-public'")
        .await
        .unwrap();
    assert_eq!(rows.rows[0][0].as_str(), Some("SECRET"));
    db.close().await;
}

#[tokio::test]
#[ignore = "requires isolated ChuSQL; see test/test-accounts-db.py"]
async fn article_access_waits_for_policy_change_and_rechecks_visibility() {
    use content::{ArticlePolicy, Visibility};
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path()).await;
    let store = accounts::store(&app).await;
    let hash = accounts::hash_password("Administrator-test-123".into())
        .await
        .unwrap();
    let reader = store.register("reader", "读者", &hash).await.unwrap();
    let (cookie, csrf) = article_test_login(app.clone(), "admin").await;
    app.snapshot.write().await.policies.insert(
        "alice-public".into(),
        ArticlePolicy {
            visibility: Visibility::Top,
            ..Default::default()
        },
    );
    let guard = app.article_changes.lock().await;
    let task_app = app.clone();
    let mut grant = tokio::spawn(async move {
        request(
            task_app,
            "POST",
            "/api/articles/alice-public/access",
            json!({"username":"reader","allowed":true}),
            Some(&cookie),
            Some(&csrf),
        )
        .await
    });
    // A pending grant must not commit while a policy mutation owns the writer lock.
    assert!(
        tokio::time::timeout(Duration::from_secs(3), &mut grant)
            .await
            .is_err()
    );
    app.snapshot
        .write()
        .await
        .policies
        .get_mut("alice-public")
        .unwrap()
        .visibility = Visibility::Default;
    store.revoke_granted_access("alice-public").await.unwrap();
    drop(guard);
    assert_eq!(grant.await.unwrap().0, StatusCode::BAD_REQUEST);
    assert!(
        !store
            .has_article_access(
                reader.id,
                "alice-public",
                accounts_db::AccessSource::Granted
            )
            .await
            .unwrap()
    );
}
