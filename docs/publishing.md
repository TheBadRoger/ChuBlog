# 账户授权与文章发布

[返回项目首页](../README.md)

## 注册与发布授权

在网站注册并登录。新用户为 `reader`，不能自行获得发布权。账号密码存储及会话说明见 [API 文档](API.md)。由具有授权权限的既有管理账号将作者设为 `enabled:true`、`role:publisher`，之后才能 clone、fetch 和 push。部署不自动创建管理员。

网站登录会话与 Git 凭据分别使用：浏览器使用 Cookie 与 CSRF，Git 使用 HTTP Basic 账号密码，不使用浏览器 Cookie，也不再登记 SSH 公钥。

## 克隆个人文章仓库

```sh
git clone https://blog.example.com/git/content.git my-articles
cd my-articles
```

Git 提示时输入网站用户名和密码。可以使用系统提供的安全凭据管理器保存凭据，避免将密码放进 URL、命令行或仓库配置。不要关闭 TLS 证书验证。Git 的 `credential.helper store` 默认明文存储密码，不建议用于保存网站口令。

所有作者使用同一个链接，服务端根据认证账号映射到各自独立的仓库。即使文章公开，也不允许匿名 clone 或跨作者读取 Git 历史。作者无需创建服务器裸仓库，首次获授权访问会自动建立。提交中的 author/email 不作为认证依据。已有仓库可以执行：

```sh
git remote set-url origin https://blog.example.com/git/content.git
git pull --ff-only
```

取消发布权、封禁或改密码后，下一次 Git 请求会重新检查；旧密码不能继续使用，推送 Hook 也会在验收时复查凭据和授权。公网只接受 HTTPS，本地回环 HTTP 仅用于开发验证。

## 创建 TOML 索引

在仓库根目录创建 UTF-8 文件 `index.toml`，每篇文章使用一个 `[[articles]]` 表：

```toml
[[articles]]
id = "first"
folder = "posts/first"
title = "第一篇文章"
description = "文章简介"
tags = ["Rust", "随笔"]
public = true
# cover = "cover.png"
# entry = "main.md"
```

`id` 和 `folder` 用于标识文章及其目录。文章元信息仅接受以下字段：

| 字段 | 含义与默认值 |
| --- | --- |
| `title` | 必填标题，最多 240 字节 |
| `description` | 简介，省略时为空，最多 1000 字节 |
| `tags` | 标签，默认空数组，最多 12 项，每项最多 80 字节 |
| `cover` | 可选封面，填写同文章目录中的图片文件名 |
| `public` | 是否公开，默认 `false` |
| `entry` | 入口文档名，默认 `main.md` |

作者身份由 HTTPS 账号密码认证的账户确定。创建时间、首次公开时间和修改时间由服务端生成，以 UTC 时间保存。索引不接受 `author`、`date`、`updated`、精华或权限等额外字段。正文不再使用 Frontmatter。

文章 ID 在作者内唯一，允许 1 到 40 位小写字母、数字和中间短横线。目录为 `posts/<目录名>`，目录名最多 80 字节并遵循 slug 规则。ID 和目录名不能使用保留名称，例如 `admin` 或 `api`。同一目录不能登记多次。

文件名只允许 ASCII 字母、数字、短横线、下划线和点，最多 100 字节，不能以点开头或含 `..`。入口允许 `.md`、`.markdown` 和 `.txt`，扩展名须为小写。索引最多 64 KiB、1000 篇文章。要撤回全部文章，明确写入 `articles = []`；空文件缺少文章字段，会暂存并保留原文章。

## 编写多页文章

示例目录：

```text
index.toml
posts/
└── first/
    ├── main.md
    ├── chapter.md
    └── cover.png
```

`main.md` 内容：

```markdown
# 第一篇文章

[阅读第二页](chapter.md#details)

![封面](cover.png)
```

`chapter.md` 内容：

```markdown
# 第二页

## Details

[返回入口](./main.md)
```

服务端从入口沿文章内相对链接收集页面，允许循环链接，每篇最多 64 个可达页面。链接目标必须是同目录的文本文件，可以带 `#锚点`，不支持子目录、`../`、编码路径和查询参数。相对链接缺少目标文件时，整份索引暂存，原文章不更新。普通 HTTP、HTTPS、mailto 和站内绝对链接继续受安全检查。

未登记目录、从入口无法到达的文档，以及未被页面引用的图片不进入阅读快照或数据库。封面即使未在正文引用，也作为文章资源导入。所有上传文件仍受白名单、安全检查和仓库配额约束。`.txt` 页面通过 `format: text` 返回，阅读端须作为纯文本显示。

参考[索引样例](../content/index.toml)、[入口样例](../content/posts/welcome/main.md)和[第二页样例](../content/posts/welcome/chapter.md)。

## 提交并验证发布

```sh
git add index.toml posts/first
git commit -m 'Publish first article'
git push -u origin main
```

服务端会校验提交并自动导入。站内文章 ID 为 `作者名~文章ID`，例如 `/api/posts/alice~first`。文章 API 返回入口正文、页面列表和资源地址；页面通过 `/api/posts/alice~first/pages/chapter.md` 读取。前端需将文档相对链接映射到返回的页面 URL，并保留锚点。

| 提交内容 | 结果 |
| --- | --- |
| 有效索引、新增文章且入口存在 | 导入文章并创建数据库元信息与页面记录 |
| 有效索引、修改正文或元信息 | 更新文章，保留首次公开时间；内容实际变化时更新修改时间 |
| 有效索引、删除文章表 | 撤回文章、页面、资源入口及数据库派生记录 |
| 有效索引、入口文件或目录缺失 | 将该文章视为不存在，撤回旧版本；其他有效文章仍可导入 |
| TOML 缺失、语法错误、未知字段或元信息无效 | Git 提交暂存，不修改该作者原有文章，等待修复后的下一次推送 |
| 脚本、非法文件类型、危险路径或内容超限 | 拒绝整个推送，远端分支不前进 |

`public = false` 的内容不会由公共文章、页面或资源接口返回。登录后查看内部文章的功能仍在待办中。删除和撤回不擦除个人 Git 历史，也不收回作者已克隆的副本。

## 提交限制与状态查询

每次推送只接受一个直接继承个人远端 `main` 的提交，首次必须是根提交。禁止 merge、force push、删除 `main`、标签和其他分支。多次本地提交须先整理为一次提交，再变基到最新远端。同一作者在多台电脑操作仍需处理冲突，不同作者使用不同仓库锁。

管理员通过 `GET /api/admin/status` 查看状态。`commit` 是当前生效作者提交映射的集合摘要，`indexed_commit` 是数据库已同步的集合版本。`pending` 按作者返回暂存提交和索引错误。暂存提交不会替换生效版本，API 重启后仍保留原文章和暂存状态。

同步通常在下一轮后台处理时完成。数据库失败至少间隔 30 秒重试，作者较多或索引重建时可能延迟。Git 接受不代表文章已公开或数据库已同步。站长审核尚未启用，计划见 [TODO](../TODO.md)。
