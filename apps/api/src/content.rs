use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub slug: String,
    pub date: String,
    pub updated: String,
    pub status: Status,
    pub tags: Vec<String>,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub name: String,
    pub data: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Post {
    pub meta: Metadata,
    pub markdown: String,
    #[serde(default)]
    pub entry: String,
    #[serde(default)]
    pub pages: BTreeMap<String, String>,
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub article_id: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub git_commit: String,
    #[serde(default = "markdown_format")]
    pub format: String,
}
fn markdown_format() -> String {
    "markdown".into()
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Snapshot {
    pub commit: String,
    pub posts: BTreeMap<String, Post>,
    #[serde(default)]
    pub revisions: BTreeMap<String, String>,
    #[serde(default)]
    pub pending: BTreeMap<String, PendingIndex>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PendingIndex {
    pub commit: String,
    pub reason: String,
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

pub fn stamp(post: &mut Post, previous: Option<&Post>, now: &str) {
    // Legacy author-supplied dates are not trusted as server publication records.
    let previous = previous.filter(|p| !p.meta.created_at.is_empty());
    post.meta.created_at = previous
        .map(|p| p.meta.created_at.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| now.into());
    post.meta.date = previous
        .map(|p| p.meta.date.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if post.meta.status == Status::Published {
                now.into()
            } else {
                String::new()
            }
        });
    let unchanged = previous.is_some_and(|p| {
        p.meta.title == post.meta.title
            && p.meta.description == post.meta.description
            && p.meta.tags == post.meta.tags
            && p.meta.cover == post.meta.cover
            && p.meta.status == post.meta.status
            && p.path == post.path
            && p.pages == post.pages
            && p.assets == post.assets
            && p.markdown == post.markdown
            && p.entry == post.entry
    });
    post.meta.updated = if unchanged {
        previous.unwrap().meta.updated.clone()
    } else {
        now.into()
    };
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
            && !matches!(name.as_str(), "index.md" | "index.markdown" | "index.txt"),
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
    git_as(repo, args, input, "ChuBlog").await
}
pub async fn git_as(
    repo: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    author: &str,
) -> Result<Vec<u8>> {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("pack.threads=1")
        .arg("-c")
        .arg("gc.auto=0")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/etc/chublog/gitconfig")
        .env("GIT_AUTHOR_NAME", author)
        .env("GIT_AUTHOR_EMAIL", "blog@localhost")
        .env("GIT_COMMITTER_NAME", author)
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

#[derive(Debug)]
pub enum Scan {
    Ready(Snapshot),
    Pending(String),
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    articles: Vec<ManifestEntry>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    pub id: String,
    pub folder: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default, rename = "public")]
    pub is_public: bool,
    #[serde(default = "default_entry")]
    pub entry: String,
}
fn default_entry() -> String {
    "main.md".into()
}
pub fn manifest(text: &str) -> Result<Vec<ManifestEntry>> {
    ensure!(text.len() <= 65536, "index.toml 超过 64 KiB");
    validate_text(text)?;
    let index: Manifest = toml::from_str(text)
        .map_err(|error: toml::de::Error| anyhow::anyhow!("index.toml: {}", error.message()))?;
    ensure!(index.articles.len() <= 1000, "最多 1000 篇文章");
    let mut ids = std::collections::BTreeSet::new();
    let mut folders = std::collections::BTreeSet::new();
    for article in &index.articles {
        slug(&article.id)?;
        ensure!(
            article.id.len() <= 40 && ids.insert(&article.id),
            "文章 ID 过长或重复"
        );
        slug(
            article
                .folder
                .strip_prefix("posts/")
                .context("文章目录必须位于 posts 下")?,
        )?;
        ensure!(folders.insert(&article.folder), "文章目录不能重复映射");
        filename(&article.entry)?;
        ensure!(
            matches!(
                article.entry.rsplit('.').next(),
                Some("md" | "markdown" | "txt")
            ),
            "入口必须为 md、markdown 或 txt"
        );
        ensure!(
            !article.title.trim().is_empty()
                && article.title.len() <= 240
                && article.description.len() <= 1000,
            "标题或简介长度无效"
        );
        ensure!(
            article.tags.len() <= 12
                && article
                    .tags
                    .iter()
                    .all(|t| !t.trim().is_empty() && t.len() <= 80),
            "标签无效"
        );
        validate_text(&article.title)?;
        validate_text(&article.description)?;
        for tag in &article.tags {
            validate_text(tag)?;
        }
        if let Some(cover) = &article.cover {
            filename(cover)?;
        }
    }
    Ok(index.articles)
}

fn filename(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 100
            && !name.starts_with('.')
            && !name.contains("..")
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
        "文件名无效"
    );
    Ok(())
}

pub fn validate_text(text: &str) -> Result<()> {
    ensure!(
        !text.contains("-----BEGIN ") && !text.contains("-----END "),
        "禁止提交 PEM 证书或密钥"
    );
    ensure!(
        !text.chars().any(|c| c == '\0'
            || (c.is_control() && !"\n\r\t".contains(c))
            || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')),
        "禁止控制符及双向隐藏字符"
    );
    Ok(())
}

#[derive(Default)]
struct Folder {
    texts: BTreeMap<String, String>,
    assets: Vec<Asset>,
    bytes: usize,
}

pub async fn scan_at(repo: &Path, commit: &str) -> Result<Scan> {
    let entries = git(repo, &["ls-tree", "-rz", commit], None).await?;
    let mut grouped: BTreeMap<String, Folder> = BTreeMap::new();
    let mut index = None;
    let mut total = 0;
    let mut count = 0;
    for entry in entries.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        count += 1;
        ensure!(count <= 14001, "仓库文件数超过限制");
        let entry = std::str::from_utf8(entry)?;
        let (info, path) = entry.split_once('\t').context("Git 树格式错误")?;
        let info: Vec<_> = info.split_whitespace().collect();
        ensure!(
            info.len() == 3 && info[0] == "100644" && info[1] == "blob",
            "禁止符号链接及特殊文件"
        );
        let parts: Vec<_> = path.split('/').collect();
        let is_index = path == "index.toml";
        if !is_index {
            ensure!(
                parts.len() == 3 && parts[0] == "posts",
                "仅允许 index.toml 和 posts/文件夹/文件"
            );
            slug(parts[1])?;
            filename(parts[2])?;
        }
        let extension = path.rsplit('.').next().unwrap_or("");
        let text_file = matches!(extension, "txt" | "md" | "markdown");
        ensure!(
            is_index || text_file || matches!(extension, "png" | "jpg" | "jpeg" | "webp"),
            "仅允许 txt、md、markdown、PNG/JPEG/WebP；禁止证书、配置及隐藏文件"
        );
        let size = String::from_utf8(git(repo, &["cat-file", "-s", info[2]], None).await?)?
            .trim()
            .parse::<usize>()?;
        let limit = if is_index {
            65536
        } else if text_file {
            MAX_MARKDOWN
        } else {
            2 * 1024 * 1024
        };
        ensure!(size <= limit, "文件超过大小限制");
        total += size;
        ensure!(total <= MAX_SNAPSHOT, "作者仓库超过 64 MiB 配额");
        let bytes = git(repo, &["cat-file", "blob", info[2]], None).await?;
        if is_index {
            let text = String::from_utf8(bytes).context("index.toml 必须为 UTF-8");
            if let Ok(value) = &text {
                validate_text(value)?;
            }
            index = Some(text);
            continue;
        }
        let folder = grouped.entry(format!("posts/{}", parts[1])).or_default();
        folder.bytes += bytes.len();
        ensure!(folder.bytes <= 6 * 1024 * 1024, "文章目录不得超过 6 MiB");
        if text_file {
            let text = String::from_utf8(bytes).context("文本必须为 UTF-8")?;
            validate_text(&text)?;
            folder
                .texts
                .insert(parts[2].into(), text.replace("\r\n", "\n"));
        } else {
            let asset = Asset {
                name: parts[2].into(),
                data: STANDARD.encode(bytes),
            };
            decode_asset(&asset)?;
            folder.assets.push(asset);
            ensure!(folder.assets.len() <= 12, "每个目录最多 12 张图片");
        }
        ensure!(grouped.len() <= 1000, "最多 1000 个文章目录");
    }
    for folder in grouped.values() {
        for text in folder.texts.values() {
            validate_markdown(text, &folder.assets)?;
        }
    }
    // Index/schema failures stage the commit, while security failures above reject the push.
    let index = match index {
        Some(Ok(text)) => match manifest(&text) {
            Ok(index) => index,
            Err(error) => return Ok(Scan::Pending(error.to_string().chars().take(512).collect())),
        },
        Some(Err(error)) => {
            return Ok(Scan::Pending(error.to_string().chars().take(512).collect()));
        }
        None => return Ok(Scan::Pending("缺少 index.toml".into())),
    };
    let result = build_posts(index, &mut grouped, commit);
    match result {
        Ok(posts) => Ok(Scan::Ready(Snapshot {
            commit: commit.into(),
            posts,
            revisions: BTreeMap::new(),
            pending: BTreeMap::new(),
        })),
        Err(error) => Ok(Scan::Pending(error.to_string().chars().take(512).collect())),
    }
}

fn local_target(url: &str) -> &str {
    url.split('#')
        .next()
        .unwrap_or("")
        .strip_prefix("./")
        .unwrap_or_else(|| url.split('#').next().unwrap_or(""))
}
fn is_external(url: &str) -> bool {
    url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("mailto:")
        || url.starts_with('/')
        || url.starts_with('#')
}
fn build_posts(
    index: Vec<ManifestEntry>,
    grouped: &mut BTreeMap<String, Folder>,
    commit: &str,
) -> Result<BTreeMap<String, Post>> {
    use pulldown_cmark::{Event, Parser, Tag};
    let mut posts = BTreeMap::new();
    for article in index {
        let Some(folder) = grouped.remove(&article.folder) else {
            continue;
        };
        // A valid index with no entry document denotes an absent article.
        let Some(body) = folder.texts.get(&article.entry).cloned() else {
            continue;
        };
        let mut pages = BTreeMap::new();
        let mut queue = vec![article.entry.clone()];
        let mut images = std::collections::BTreeSet::new();
        if let Some(cover) = &article.cover {
            images.insert(cover.clone());
        }
        while let Some(name) = queue.pop() {
            if pages.contains_key(&name) {
                continue;
            }
            let text = folder
                .texts
                .get(&name)
                .context("文章内链接指向不存在的页面")?;
            for event in Parser::new_ext(text, pulldown_cmark::Options::all()) {
                match event {
                    Event::Start(Tag::Link { dest_url, .. }) if !is_external(&dest_url) => {
                        let target = local_target(&dest_url);
                        ensure!(
                            folder.texts.contains_key(target),
                            "文章内链接指向不存在的页面"
                        );
                        queue.push(target.into());
                    }
                    Event::Start(Tag::Image { dest_url, .. }) => {
                        images.insert(local_target(&dest_url).into());
                    }
                    _ => {}
                }
            }
            pages.insert(name, text.clone());
            ensure!(pages.len() <= 64, "每篇文章最多 64 个可达页面");
        }
        ensure!(
            images
                .iter()
                .all(|name| folder.assets.iter().any(|a| &a.name == name)),
            "封面或图片不存在"
        );
        let assets = folder
            .assets
            .into_iter()
            .filter(|a| images.contains(&a.name))
            .collect();
        let id = article.id;
        posts.insert(
            id.clone(),
            Post {
                meta: Metadata {
                    title: article.title,
                    author: String::new(),
                    slug: id.clone(),
                    description: article.description,
                    tags: article.tags,
                    cover: article.cover,
                    status: if article.is_public {
                        Status::Published
                    } else {
                        Status::Draft
                    },
                    date: String::new(),
                    updated: String::new(),
                    created_at: String::new(),
                },
                markdown: body,
                entry: article.entry.clone(),
                pages,
                assets,
                author: String::new(),
                article_id: id,
                path: format!("{}/{}", article.folder, article.entry),
                git_commit: commit.into(),
                format: if article.entry.ends_with(".txt") {
                    "text"
                } else {
                    "markdown"
                }
                .into(),
            },
        );
    }
    Ok(posts)
}

pub async fn persist(data: &Path, snapshot: &Snapshot) -> Result<()> {
    let public = Snapshot {
        commit: snapshot.commit.clone(),
        revisions: BTreeMap::new(),
        pending: BTreeMap::new(),
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
    validate_text(body)?;
    for event in Parser::new_ext(body, pulldown_cmark::Options::all()) {
        match event {
            Event::Html(_) | Event::InlineHtml(_) => anyhow::bail!("Markdown 不允许原始 HTML"),
            Event::Start(Tag::Link { dest_url, .. }) => {
                ensure!(
                    dest_url.starts_with("https://")
                        || dest_url.starts_with("http://")
                        || dest_url.starts_with("mailto:")
                        || dest_url.starts_with('#')
                        || (dest_url.starts_with('/') && !dest_url.starts_with("//"))
                        || (!is_external(&dest_url) && {
                            let target = local_target(&dest_url);
                            filename(target).is_ok()
                                && matches!(
                                    target.rsplit('.').next(),
                                    Some("md" | "markdown" | "txt")
                                )
                        }),
                    "链接协议不允许"
                );
                ensure!(
                    !dest_url.contains('\\') && !dest_url.chars().any(char::is_control),
                    "链接含非法字符"
                );
            }
            Event::Start(Tag::Image { dest_url, .. }) => ensure!(
                assets.iter().any(|a| a.name == local_target(&dest_url)),
                "图片必须引用同文章已验证的图片文件"
            ),
            _ => {}
        }
    }
    Ok(())
}
