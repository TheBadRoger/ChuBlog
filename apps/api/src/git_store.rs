use crate::content::{self, Scan, Snapshot};
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

pub fn author_repository(root: &Path, identity: &str) -> Result<PathBuf> {
    crate::accounts::username(identity)?;
    Ok(root.join(format!("{identity}.git")))
}

pub async fn init_author(root: &Path, identity: &str) -> Result<PathBuf> {
    let repo = author_repository(root, identity)?;
    if !repo.exists() {
        tokio::fs::create_dir(&repo).await?;
        content::git(
            &repo,
            &[
                "init",
                "--bare",
                "--initial-branch=main",
                "--shared=group",
                ".",
            ],
            None,
        )
        .await?;
    }
    ensure!(
        std::fs::symlink_metadata(&repo)?.is_dir(),
        "作者仓库必须是普通目录"
    );
    Ok(repo)
}

pub fn directory_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.file_type()?;
        ensure!(!meta.is_symlink(), "接收仓库不得包含符号链接");
        total = total.saturating_add(if meta.is_dir() {
            directory_size(&entry.path())?
        } else {
            entry.metadata()?.len()
        });
        ensure!(
            total <= 256 * 1024 * 1024,
            "接收仓库达到 256 MiB 配额，请管理员维护"
        );
    }
    Ok(total)
}

pub async fn validate_change(repo: &Path, previous: &str, next: &str) -> Result<Scan> {
    ensure!(
        next.len() == 40
            && next.bytes().all(|b| b.is_ascii_hexdigit())
            && next != "0000000000000000000000000000000000000000",
        "禁止删除主分支或无效提交"
    );
    let initial = previous == "0000000000000000000000000000000000000000";
    let parents =
        String::from_utf8(content::git(repo, &["show", "-s", "--format=%P", next], None).await?)?;
    ensure!(
        parents.trim() == if initial { "" } else { previous },
        "每次只允许推送一个直接继承主分支的提交，首次必须为根提交；不允许合并或强推"
    );
    let snapshot = content::scan_at(repo, next).await?;
    Ok(snapshot)
}

pub async fn pre_receive() -> Result<()> {
    let store = crate::accounts_db::Store::from_env()?;
    let identity = std::env::var("CHUBLOG_PUBLISHER").context("拒绝未经 HTTPS 网关的推送")?;
    let hash = std::env::var("CHUBLOG_PASSWORD_HASH").context("缺少认证凭据版本")?;
    let publisher = store.user(&identity).await?.context("发布者不存在")?;
    ensure!(
        matches!(
            publisher.level,
            crate::accounts_db::Level::Author | crate::accounts_db::Level::Master
        ) && publisher.hash == hash,
        "服务器未授权此账号发布文章"
    );
    let repository = PathBuf::from(std::env::var("INGRESS_DIR").context("缺少受控接收目录")?);
    let mut input = String::new();
    tokio::io::stdin()
        .take(1025)
        .read_to_string(&mut input)
        .await?;
    ensure!(input.len() <= 1024, "引用更新过多");
    let lines: Vec<_> = input.lines().collect();
    ensure!(lines.len() == 1, "每次只能更新主分支，禁止标签及其他引用");
    let fields: Vec<_> = lines[0].split_whitespace().collect();
    ensure!(
        fields.len() == 3 && fields[2] == "refs/heads/main",
        "仅允许 refs/heads/main"
    );
    ensure!(
        fields[0].len() == 40 && fields[0].bytes().all(|b| b.is_ascii_hexdigit()),
        "旧提交无效"
    );
    let repo = author_repository(&repository, &identity)?;
    let actual = std::env::current_dir()?;
    ensure!(
        std::fs::canonicalize(&repo)? == std::fs::canonicalize(actual)?,
        "推送仓库与作者身份不符"
    );
    if let Scan::Pending(reason) = validate_change(&repo, fields[0], fields[1]).await? {
        eprintln!("ChuBlog: index.toml 待修复，提交已暂存，现有文章不变：{reason}");
    }
    let current = store.user(&identity).await?.context("发布者已移除")?;
    ensure!(
        current.same_revision(&publisher),
        "推送期间口令或权限已变更，请重新认证"
    );
    eprintln!(
        "ChuBlog: identity={identity} validated commit={}",
        fields[1]
    );
    Ok(())
}

// Each fetch receives only one author's objects into an API-owned private cache.
pub async fn import_authors(
    data: &Path,
    source: &Path,
    current: &Snapshot,
    identities: &[String],
) -> Result<Snapshot> {
    let cache = data.join("authors");
    tokio::fs::create_dir_all(&cache).await?;
    let mut combined = current.clone();
    for identity in identities {
        let incoming = author_repository(source, identity)?;
        if !incoming.exists() {
            continue;
        }
        ensure!(
            std::fs::symlink_metadata(&incoming)?.is_dir(),
            "接收仓库不是普通目录"
        );
        let result = async {
            let author_data = cache.join(identity);
            let repo = content::init(&author_data).await?;
            let safe = format!("safe.directory={}", incoming.display());
            // The initial bare repository may have no main branch until its first push.
            let refs = content::git(
                &repo,
                &[
                    "-c",
                    &safe,
                    "ls-remote",
                    incoming.to_str().context("接收路径无效")?,
                    "refs/heads/main",
                ],
                None,
            )
            .await?;
            if refs.is_empty() {
                return Ok::<_, anyhow::Error>(None);
            }
            content::git(
                &repo,
                &[
                    "-c",
                    &safe,
                    "fetch",
                    "--no-tags",
                    "--no-recurse-submodules",
                    "--",
                    incoming.to_str().context("接收路径无效")?,
                    "refs/heads/main",
                ],
                None,
            )
            .await?;
            let commit =
                String::from_utf8(content::git(&repo, &["rev-parse", "FETCH_HEAD"], None).await?)?;
            let commit = commit.trim();
            if current
                .revisions
                .get(identity)
                .is_some_and(|previous| previous == commit)
                || current
                    .pending
                    .get(identity)
                    .is_some_and(|pending| pending.commit == commit)
            {
                return Ok(None);
            }
            let snapshot = content::scan_at(&repo, commit).await?;
            content::git(&repo, &["update-ref", "refs/heads/main", commit], None).await?;
            Ok(Some((commit.to_owned(), snapshot)))
        }
        .await;
        match result {
            Ok(Some((commit, Scan::Pending(reason)))) => {
                combined
                    .pending
                    .insert(identity.clone(), content::PendingIndex { commit, reason });
            }
            Ok(Some((_, Scan::Ready(snapshot)))) => {
                let mut candidate = combined.clone();
                candidate.posts.retain(|_, post| post.author != *identity);
                for (id, mut post) in snapshot.posts {
                    post.author = identity.clone();
                    post.meta.author = identity.clone();
                    // Neither username nor local ID allows '~', so this is collision-free.
                    let key = format!("{identity}~{id}");
                    post.meta.slug = key.clone();
                    let now =
                        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                    content::stamp(&mut post, combined.posts.get(&key), &now);
                    candidate.posts.insert(key, post);
                }
                let bytes: usize = candidate
                    .posts
                    .values()
                    .map(|p| {
                        p.pages.values().map(String::len).sum::<usize>()
                            + p.assets.iter().map(|a| a.data.len() * 3 / 4).sum::<usize>()
                    })
                    .sum();
                if candidate.posts.len() > 1000 || bytes > content::MAX_SNAPSHOT {
                    tracing::warn!(action = "author_import_pending", author = %identity, "站点文章数量或容量超过上限");
                    continue;
                }
                candidate
                    .revisions
                    .insert(identity.clone(), snapshot.commit);
                candidate.pending.remove(identity);
                combined = candidate;
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(action = "author_import_pending", author = %identity, error = %error)
            }
        }
    }
    let repo = content::init(data).await?;
    let digest = content::git(
        &repo,
        &["hash-object", "--stdin"],
        Some(&serde_json::to_vec(&combined.revisions)?),
    )
    .await?;
    combined.commit = String::from_utf8(digest)?.trim().into();
    Ok(combined)
}
