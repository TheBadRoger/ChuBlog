use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub const MAX_MARKDOWN: usize = 256 * 1024;
pub const MAX_SNAPSHOT: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Draft,
    Published,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub title: String,
    pub slug: String,
    pub date: String,
    pub updated: String,
    pub status: Status,
    pub tags: Vec<String>,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub name: String,
    pub data: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Post {
    pub meta: Metadata,
    pub markdown: String,
    pub assets: Vec<Asset>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Snapshot {
    pub commit: String,
    pub posts: BTreeMap<String, Post>,
}

pub fn slug(value: &str) -> Result<&str> {
    ensure!(
        (1..=80).contains(&value.len())
            && value
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            && !value.starts_with('-')
            && !value.ends_with('-'),
        "slug 仅允许 1–80 位小写英文、数字和中间短横线"
    );
    ensure!(
        ![
            "admin", "api", "posts", "tags", "archive", "rss", "sitemap", "404"
        ]
        .contains(&value),
        "slug 为保留名称"
    );
    Ok(value)
}

pub fn parse(markdown: String, assets: Vec<Asset>) -> Result<Post> {
    ensure!(markdown.len() <= MAX_MARKDOWN, "Markdown 超过 256 KiB");
    let normalized = markdown.replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .context("缺少 YAML Frontmatter")?;
    let (yaml, _) = rest.split_once("\n---\n").context("Frontmatter 未闭合")?;
    let meta: Metadata = serde_yaml::from_str(yaml).context("Frontmatter 无效")?;
    slug(&meta.slug)?;
    ensure!(
        !meta.title.trim().is_empty() && meta.title.len() <= 240 && meta.description.len() <= 1000,
        "标题或摘要长度无效"
    );
    for date in [&meta.date, &meta.updated] {
        let parsed =
            NaiveDate::parse_from_str(date, "%Y-%m-%d").context("日期必须为 YYYY-MM-DD")?;
        ensure!(
            parsed.format("%Y-%m-%d").to_string() == *date,
            "日期格式不规范"
        );
    }
    ensure!(meta.updated >= meta.date, "更新时间不能早于发布日期");
    ensure!(
        meta.tags.len() <= 12
            && meta
                .tags
                .iter()
                .all(|t| !t.trim().is_empty() && t.len() <= 80),
        "标签无效"
    );
    ensure!(assets.len() <= 12, "最多 12 张图片");
    let mut names = std::collections::HashSet::new();
    let mut total = normalized.len();
    for asset in &assets {
        ensure!(names.insert(&asset.name), "图片名称重复");
        let bytes = decode_asset(asset)?;
        total += bytes.len();
    }
    ensure!(total <= 6 * 1024 * 1024, "文章及图片总计不得超过 6 MiB");
    validate_markdown(rest.split_once("\n---\n").unwrap().1, &assets)?;
    Ok(Post {
        meta,
        markdown: normalized,
        assets,
    })
}

pub fn decode_asset(asset: &Asset) -> Result<Vec<u8>> {
    let name = &asset.name;
    ensure!(
        name.len() <= 100
            && !name.starts_with('.')
            && !name.contains("..")
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            && name != "index.md",
        "图片文件名无效"
    );
    ensure!(asset.data.len() <= 3 * 1024 * 1024, "图片超过大小限制");
    let bytes = STANDARD.decode(&asset.data).context("图片编码无效")?;
    ensure!(bytes.len() <= 2 * 1024 * 1024, "单张图片不得超过 2 MiB");
    let format = image::guess_format(&bytes).context("无法识别图片")?;
    let extension = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    ensure!(
        matches!(
            (format, extension.as_str()),
            (image::ImageFormat::Png, "png")
                | (image::ImageFormat::Jpeg, "jpg" | "jpeg")
                | (image::ImageFormat::WebP, "webp")
        ),
        "仅支持与扩展名一致的 PNG/JPEG/WebP"
    );
    let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
    let (width, height) = reader.into_dimensions().context("图片头无效")?;
    ensure!(
        width > 0 && height > 0 && width <= 4096 && height <= 4096,
        "图片尺寸不得超过 4096×4096"
    );
    image::load_from_memory_with_format(&bytes, format).context("图片内容损坏")?;
    Ok(bytes)
}

pub async fn git(repo: &Path, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/etc/chublog/gitconfig")
        .env("GIT_AUTHOR_NAME", "ChuBlog")
        .env("GIT_AUTHOR_EMAIL", "blog@localhost")
        .env("GIT_COMMITTER_NAME", "ChuBlog")
        .env("GIT_COMMITTER_EMAIL", "blog@localhost")
        .env("GIT_INDEX_FILE", repo.join("publish.index"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(15), async {
        let mut child = command.spawn().context("启动 Git 失败")?;
        let mut stdin = child.stdin.take().context("Git stdin")?;
        let stdout = child.stdout.take().context("Git stdout")?;
        let stderr = child.stderr.take().context("Git stderr")?;
        let write = async {
            stdin.write_all(input.unwrap_or_default()).await?;
            drop(stdin);
            Ok::<_, std::io::Error>(())
        };
        let read_out = async {
            let mut out = Vec::new();
            stdout
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut out)
                .await?;
            Ok::<_, std::io::Error>(out)
        };
        let read_err = async {
            let mut err = Vec::new();
            stderr.take(65537).read_to_end(&mut err).await?;
            Ok::<_, std::io::Error>(err)
        };
        let (_, stdout, stderr) = tokio::try_join!(write, read_out, read_err)?;
        ensure!(
            stdout.len() <= 4 * 1024 * 1024 && stderr.len() <= 65536,
            "Git 输出超过限制"
        );
        let status = child.wait().await?;
        Ok::<_, anyhow::Error>(std::process::Output {
            status,
            stdout,
            stderr,
        })
    })
    .await
    .context("Git 操作超时")??;
    ensure!(
        output.status.success(),
        "Git 操作失败: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output.stdout)
}

pub async fn init(data: &Path) -> Result<PathBuf> {
    tokio::fs::create_dir_all(data).await?;
    let repo = data.join("content.git");
    if !repo.exists() {
        tokio::fs::create_dir(&repo).await?;
        git(
            &repo,
            &["init", "--bare", "--initial-branch=main", "."],
            None,
        )
        .await?;
        git(&repo, &["read-tree", "--empty"], None).await?;
        let tree = git(&repo, &["write-tree"], None).await?;
        let tree = String::from_utf8(tree)?;
        let commit = git(
            &repo,
            &[
                "commit-tree",
                tree.trim(),
                "-m",
                "Initialize content repository",
            ],
            None,
        )
        .await?;
        let commit = String::from_utf8(commit)?;
        git(
            &repo,
            &["update-ref", "refs/heads/main", commit.trim()],
            None,
        )
        .await?;
    }
    Ok(repo)
}

pub async fn scan(repo: &Path) -> Result<Snapshot> {
    let commit = String::from_utf8(git(repo, &["rev-parse", "refs/heads/main"], None).await?)?
        .trim()
        .to_owned();
    scan_at(repo, &commit).await
}

pub async fn scan_at(repo: &Path, commit: &str) -> Result<Snapshot> {
    let commit = commit.to_owned();
    let entries = git(repo, &["ls-tree", "-rz", &commit], None).await?;
    let mut grouped: BTreeMap<String, (Option<String>, Vec<Asset>)> = BTreeMap::new();
    let mut total = 0;
    for entry in entries.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let entry = std::str::from_utf8(entry)?;
        let (info, path) = entry.split_once('\t').context("Git 树格式错误")?;
        let info: Vec<_> = info.split_whitespace().collect();
        ensure!(
            info.len() == 3 && info[0] == "100644" && info[1] == "blob",
            "禁止符号链接及特殊文件"
        );
        let parts: Vec<_> = path.split('/').collect();
        ensure!(
            parts.len() == 3 && parts[0] == "posts",
            "仓库包含非文章路径"
        );
        slug(parts[1])?;
        let size = String::from_utf8(git(repo, &["cat-file", "-s", info[2]], None).await?)?
            .trim()
            .parse::<usize>()?;
        let limit = if parts[2] == "index.md" {
            MAX_MARKDOWN
        } else {
            2 * 1024 * 1024
        };
        ensure!(size <= limit, "文件超过大小限制");
        let bytes = git(repo, &["cat-file", "blob", info[2]], None).await?;
        total += bytes.len();
        ensure!(total <= MAX_SNAPSHOT, "内容仓库超过 64 MiB 配额");
        let group = grouped.entry(parts[1].into()).or_default();
        if parts[2] == "index.md" {
            group.0 = Some(String::from_utf8(bytes)?);
        } else {
            group.1.push(Asset {
                name: parts[2].into(),
                data: STANDARD.encode(bytes),
            });
        }
        ensure!(grouped.len() <= 1000, "最多 1000 篇文章");
    }
    let mut posts = BTreeMap::new();
    for (key, (markdown, assets)) in grouped {
        let post = parse(markdown.context("文章缺少 index.md")?, assets)?;
        ensure!(post.meta.slug == key, "slug 与 Git 路径不一致");
        posts.insert(key, post);
    }
    Ok(Snapshot { commit, posts })
}

pub async fn persist(data: &Path, snapshot: &Snapshot) -> Result<()> {
    let public = Snapshot {
        commit: snapshot.commit.clone(),
        posts: snapshot
            .posts
            .iter()
            .filter(|(_, p)| p.meta.status == Status::Published)
            .map(|(k, p)| (k.clone(), p.clone()))
            .collect(),
    };
    let bytes = serde_json::to_vec(&public)?;
    let tmp = data.join("published.json.tmp");
    let mut file = tokio::fs::File::create(&tmp).await?;
    file.write_all(&bytes).await?;
    file.sync_all().await?;
    tokio::fs::rename(tmp, data.join("published.json")).await?;
    Ok(())
}

pub fn validate_markdown(body: &str, assets: &[Asset]) -> Result<()> {
    use pulldown_cmark::{Event, Parser, Tag};
    ensure!(
        !body.chars().any(|c| c == '\0'
            || (c.is_control() && !"\n\r\t".contains(c))
            || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
        "禁止控制符及双向隐藏字符"
    );
    for event in Parser::new_ext(body, pulldown_cmark::Options::all()) {
        match event {
            Event::Html(_) | Event::InlineHtml(_) => anyhow::bail!("Markdown 不允许原始 HTML"),
            Event::Start(Tag::Link { dest_url, .. }) => {
                ensure!(
                    dest_url.starts_with("https://")
                        || dest_url.starts_with("http://")
                        || dest_url.starts_with("mailto:")
                        || dest_url.starts_with('#')
                        || (dest_url.starts_with('/') && !dest_url.starts_with("//")),
                    "链接协议不允许"
                );
                ensure!(
                    !dest_url.contains('\\') && !dest_url.chars().any(char::is_control),
                    "链接含非法字符"
                );
            }
            Event::Start(Tag::Image { dest_url, .. }) => ensure!(
                assets.iter().any(|a| a.name == dest_url.as_ref()),
                "图片必须引用同文章已验证的图片文件"
            ),
            _ => {}
        }
    }
    Ok(())
}
