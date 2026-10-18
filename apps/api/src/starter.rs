use crate::{App, accounts, content, git_store};
use anyhow::{Context, Result, ensure};
use std::path::Path;

const FILES: &[(&str, &str)] = &[
    ("index.toml", include_str!("../starter/index.toml")),
    (
        "posts/publishing/main.md",
        include_str!("../starter/posts/publishing/main.md"),
    ),
    (
        "posts/development/main.md",
        include_str!("../starter/posts/development/main.md"),
    ),
];

// The installation row and MASTER are committed before any repository is released.
pub async fn ensure(app: &App) -> Result<()> {
    let store = accounts::store(app).await;
    if !store.starters_pending().await? {
        return Ok(());
    }
    let owner = store
        .owner()
        .await?
        .context("安装记录没有有效的数据库管理员账户")?;
    tokio::fs::create_dir_all(&app.repository).await?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(app.repository.join(format!(".{}.lock", owner.username)))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("管理员仓库正在使用")?;
    if !store.starters_pending().await? {
        return Ok(());
    }
    let repo = git_store::init_author(&app.repository, &owner.username).await?;
    let commit = write_repository(&repo, &owner.username).await?;
    store.mark_starters(&owner, &commit).await?;
    Ok(())
}
async fn text(repo: &Path, args: &[&str], input: Option<&[u8]>) -> Result<String> {
    Ok(String::from_utf8(content::git(repo, args, input).await?)?
        .trim()
        .into())
}
async fn write_repository(repo: &Path, author: &str) -> Result<String> {
    content::git(repo, &["read-tree", "--empty"], None).await?;
    for (path, body) in FILES {
        let blob = text(
            repo,
            &["hash-object", "-w", "--stdin"],
            Some(body.as_bytes()),
        )
        .await?;
        content::git(
            repo,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                "100644",
                &blob,
                path,
            ],
            None,
        )
        .await?;
    }
    let tree = text(repo, &["write-tree"], None).await?;
    // Recover a crash after update-ref but before the database completion record.
    if let Ok(head) = text(repo, &["rev-parse", "--verify", "refs/heads/main"], None).await {
        let current_tree = text(repo, &["rev-parse", "HEAD^{tree}"], None).await?;
        ensure!(current_tree == tree, "管理员仓库已有其他内容，拒绝覆盖");
        return Ok(head);
    }
    let commit = String::from_utf8(
        content::git_as(
            repo,
            &["commit-tree", &tree, "-m", "发布发文教程与开发进度"],
            None,
            author,
        )
        .await?,
    )?
    .trim()
    .to_owned();
    ensure!(
        matches!(
            content::scan_at(repo, &commit).await?,
            content::Scan::Ready(_)
        ),
        "内置文章没有通过内容校验"
    );
    content::git(
        repo,
        &[
            "update-ref",
            "refs/heads/main",
            &commit,
            "0000000000000000000000000000000000000000",
        ],
        None,
    )
    .await?;
    Ok(commit)
}

#[cfg(test)]
#[path = "../../../test/api/starter.rs"]
mod tests;
