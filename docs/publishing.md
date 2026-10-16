# 账户授权与文章发布

[返回项目首页](../README.md)

## 用户与发布授权

完整接口见 [API 文档](API.md)。

1. 注册：`POST /api/auth/register`，正文 `{ "username": "alice", "password": "至少12字节的口令" }`。默认角色 `reader`，绝不接受客户端指定角色。`ALLOW_REGISTRATION=false` 可关闭注册。
2. 登录：`POST /api/auth/login`，相同字段。浏览器保存 HttpOnly Cookie；返回 `csrf`。所有修改请求必须携带站点 `Origin` 和 `X-CSRF-Token`。
3. 本地生成 `ssh-keygen -t ed25519` 或 `ssh-keygen -t rsa -b 3072`；登录后 `PUT /api/account/ssh-key` 提交 `{ "public_key": "ssh-ed25519 AAAA..." }`，只交公钥。RSA 使用标准 `ssh-rsa AAAA...` 公钥文本，支持 2048–8192 位；实际认证使用 RSA/SHA-256 或 RSA/SHA-512，不启用旧的 SHA-1 签名。见 [OpenSSH 说明](https://www.openssh.com/txt/release-8.8)。
4. 管理员登录后调用 `PATCH /api/admin/users/alice`，正文 `{ "enabled": true, "role": "publisher" }`。服务器按公钥绑定的用户分配独立仓库。
5. 此时 SSH 才接受该用户的密钥；`reader`、停用用户、未绑密钥的用户均不能访问 Git。管理员也需登记公钥才能发布。旧 `prefixes` 字段仅为兼容保留，不再用于文章隔离。

- 授权与密钥独立：知道服务器地址、有网站账户、上传了公钥都不足以推送。
- 设回 `reader` 或 `enabled:false` 会撤回发布授权，并使现有网站会话失效。
- 禁止管理员停用自己或取消自己的管理角色。

## 克隆个人文章仓库

先通过可信渠道核对服务器主机密钥指纹，再在本机 `~/.ssh/config` 配置连接：

```sshconfig
Host chublog
    HostName blog.example.com
    Port 2222
    User git
    IdentityFile ~/.ssh/id_ed25519
    IdentitiesOnly yes
```

将域名、端口和私钥路径替换为部署者提供的值。主机密钥检查应保持开启。服务器根据公钥识别作者，不使用 Git 的 author/email 判定身份。

```sh
git clone chublog:content.git my-articles
cd my-articles
```

每位作者都使用 `content.git` 别名。服务器将别名映射到该作者的独立仓库，并在首次访问时创建裸仓库，作者无需服务器 shell。仓库已有提交时，每轮编辑前先执行 `git pull --ff-only`。

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

作者身份由 SSH 公钥绑定的账户确定。创建时间、首次公开时间和修改时间由服务端生成，以 UTC 时间保存。索引不接受 `author`、`date`、`updated`、精华或权限等额外字段。正文不再使用 Frontmatter。

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
