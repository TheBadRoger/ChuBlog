use crate::accounts::{Grant, Role};
use crate::content::{self, Scan, Snapshot};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub repository: PathBuf,
    pub publishers: BTreeMap<String, Grant>,
}

pub fn config(path: &Path) -> Result<Config> {
    let meta = std::fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && (meta.uid() == 0 || meta.uid() == 10001) && meta.mode() & 0o022 == 0,
        "SSH 授权配置必须为 root 或 API UID 10001 所有且不可被组或其他用户写入"
    );
    let bytes = std::fs::read(path)?;
    ensure!(bytes.len() < 65536, "SSH 配置过大");
    let cfg: Config = serde_json::from_slice(&bytes)?;
    ensure!(cfg.repository.is_absolute(), "接收仓库必须为绝对路径");
    Ok(cfg)
}
fn authorized<'a>(cfg: &'a Config, identity: &str) -> Result<&'a Grant> {
    ensure!(
        !identity.is_empty()
            && identity.len() <= 64
            && identity
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "发布者身份无效"
    );
    let publisher = cfg
        .publishers
        .get(identity)
        .filter(|p| p.enabled && p.role != Role::Reader && p.ssh_key.is_some())
        .context("服务器未授权此身份发布文章")?;
    Ok(publisher)
}
pub fn author_repository(root: &Path, identity: &str) -> Result<PathBuf> {
    ensure!(
        (3..=32).contains(&identity.len())
            && identity.starts_with(|c: char| c.is_ascii_lowercase())
            && identity
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "作者仓库身份无效"
    );
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

fn directory_size(path: &Path) -> Result<u64> {
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

pub async fn gate(identity: &str) -> Result<()> {
    let config_path = PathBuf::from(
        std::env::var("CHUBLOG_SSH_CONFIG").unwrap_or_else(|_| "/auth/publishers.json".into()),
    );
    let cfg = config(&config_path)?;
    authorized(&cfg, identity)?;
    let original = std::env::var("SSH_ORIGINAL_COMMAND").unwrap_or_default();
    ensure!(
        matches!(
            original.as_str(),
            "git-receive-pack 'content.git'" | "git-upload-pack 'content.git'"
        ),
        "仅允许对 content.git 执行 Git 推送或读取"
    );
    let receiving = original.starts_with("git-receive-pack");
    let repo = author_repository(&cfg.repository, identity)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(cfg.repository.join(format!(".{identity}.lock")))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("另一位发布者正在访问仓库，请稍后重试")?;
    init_author(&cfg.repository, identity).await?;
    directory_size(&cfg.repository)?;
    let executable = if receiving {
        "git-receive-pack"
    } else {
        "git-upload-pack"
    };
    let mut child = Command::new(executable)
        .arg(&repo)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/nonexistent")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("CHUBLOG_SSH_CONFIG", &config_path)
        .env("CHUBLOG_PUBLISHER", identity)
        .env("GIT_CONFIG_COUNT", "5")
        .env("GIT_CONFIG_KEY_0", "receive.maxInputSize")
        .env("GIT_CONFIG_VALUE_0", "8388608")
        .env("GIT_CONFIG_KEY_1", "receive.fsckObjects")
        .env("GIT_CONFIG_VALUE_1", "true")
        .env("GIT_CONFIG_KEY_2", "transfer.fsckObjects")
        .env("GIT_CONFIG_VALUE_2", "true")
        .env("GIT_CONFIG_KEY_3", "gc.auto")
        .env("GIT_CONFIG_VALUE_3", "0")
        .env("GIT_CONFIG_KEY_4", "core.hooksPath")
        .env("GIT_CONFIG_VALUE_4", "/hooks")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .context("启动受限 Git 服务失败")?;
    let status = match tokio::time::timeout(Duration::from_secs(90), child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            child.kill().await?;
            bail!("SSH Git 会话超过 90 秒限制");
        }
    };
    ensure!(status.success(), "Git 访问失败");
    drop(lock);
    Ok(())
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
    let config_path =
        PathBuf::from(std::env::var("CHUBLOG_SSH_CONFIG").context("缺少受控 SSH 配置")?);
    let cfg = config(&config_path)?;
    let identity = std::env::var("CHUBLOG_PUBLISHER").context("拒绝未经 SSH 网关的推送")?;
    let publisher = authorized(&cfg, &identity)?;
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
    let repo = author_repository(&cfg.repository, &identity)?;
    let actual = std::env::current_dir()?;
    ensure!(
        std::fs::canonicalize(&repo)? == std::fs::canonicalize(actual)?,
        "推送仓库与作者身份不符"
    );
    if let Scan::Pending(reason) = validate_change(&repo, fields[0], fields[1]).await? {
        eprintln!("ChuBlog: index.toml 待修复，提交已暂存，现有文章不变：{reason}");
    }
    let latest = config(&config_path)?;
    let current = authorized(&latest, &identity)?;
    ensure!(
        current.prefixes == publisher.prefixes && current.ssh_key == publisher.ssh_key,
        "推送期间授权已变更，请重新连接"
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

pub fn authorized_keys() -> Result<()> {
    let path =
        std::env::var("CHUBLOG_SSH_CONFIG").unwrap_or_else(|_| "/auth/publishers.json".into());
    let cfg = config(Path::new(&path))?;
    for (identity, grant) in &cfg.publishers {
        if authorized(&cfg, identity).is_ok()
            && let Some(key) = &grant.ssh_key
        {
            let key = crate::accounts::public_key(key)?;
            println!("restrict,command=\"/usr/local/bin/chublog-api ssh-gate {identity}\" {key}");
        }
    }
    Ok(())
}
