# 后端 API

[返回项目首页](../README.md)

## 请求约定

- 全部正文使用 JSON。
- 同源 `Origin` 必须与可信反向代理传入的原始协议、主机及端口完全一致，例如 `https://blog.example.com`；无需域名环境变量。转发头的信任边界见[部署说明](deployment.md)。
- 响应含 `Cache-Control: no-store`。
- 登录 Cookie 为 `HttpOnly; SameSite=Strict; Secure`，仅 localhost HTTP 开发场景省略 Secure。

## 接口一览

| 方法 | 路径 | 权限 / 请求 |
| --- | --- | --- |
| GET | `/api/health` | 公共，API 存活，不代表数据库或发布同步健康 |
| GET | `/api/posts` | 公共，`{commit,posts:[metadata]}`，仅已发布，日期倒序 |
| GET | `/api/posts/:slug` | 公共，`{meta,markdown,format,author,article_id,entry,pages:[{name,url}],assets:[{name,url}]}`，草稿及不存在均 404 |
| GET | `/api/posts/:slug/pages/:name` | 公共，指定页面的正文、元信息、格式、页面列表和资源地址；非公开或不存在均 404 |
| GET | `/api/assets/:slug/:name` | 公共，仅已发布文章的已验证图片 |
| GET | `/api/stats` | 公共，文章数、标签数、正文源字节数、启动以来文章请求数 |
| POST | `/api/auth/register` | 公共 + Origin；`{username,password}`，创建 reader；可配置关闭 |
| POST | `/api/auth/login` | 公共 + Origin；`{username,password}`，返回 `{username,role,csrf}` 及 Cookie |
| POST | `/api/auth/logout` | 登录 + Origin + CSRF；使会话失效并清除 Cookie |
| GET | `/api/account` | 登录；返回自己的 `{username,permissions,csrf}`，不含口令哈希 |
| PUT | `/api/account/ssh-key` | 登录 + Origin + CSRF；`{public_key}`，替换自己的唯一 Ed25519 或 RSA 公钥（RSA 2048–8192 位） |
| PUT | `/api/account/password` | 登录 + Origin + CSRF；`{old_password,new_password}`，成功后全部会话失效 |
| GET | `/api/admin/users` | 管理员；返回用户启用状态、角色、前缀及公钥，不含哈希 |
| PATCH | `/api/admin/users/:username` | 管理员 + Origin + CSRF；`{enabled,role}`；兼容可选的旧 `prefixes` 字段 |
| GET | `/api/admin/status` | 管理员；`{commit,total_posts,drafts,indexed_commit,pending}` |

## 权限与账户

- 角色：`reader | publisher | admin`。
- 网站管理员与数据库账号、OS 用户不是同一个系统。
- 发布角色仍需已登记公钥才能使用 SSH。
- 每个账户使用自己的仓库，旧前缀字段不再承担授权作用。
- 网站登录不产生 Git 访问凭证。

用户名：3–32 位小写字母、数字、短横线，以字母开头；口令：12–1024 字节。旧前缀字段仅兼容接收，若传入仍按原格式校验。注册不接受 `role` 等额外字段。公钥不能与别的用户重复。

## 修改请求

- 修改请求的头为 `Origin: https://blog.example.com` 和 `X-CSRF-Token: <登录返回的 csrf>`，并携带 Cookie。
- 没有配置跨域 CORS，也没有浏览器本地持久存储令牌的要求。
- 授权或停用用户会使该用户既有会话失效；再次登录后才能继续使用自己的账户接口。

## 错误响应

- 普通业务错误返回 `{ "error": "可读说明" }`；401 未登录/口令错误，403 Origin/CSRF/权限错误，404 不存在或非公开内容，409 用户名冲突，429 超过限流。
- Axum 的无效 JSON/字段结构错误使用框架的 400/415/422 响应，超过请求体上限返回 413。
- 服务器内部错误不返回路径、凭据或数据库详情。

## 文章发布边界

文章写入、上传、在线预览、后台发布/删除接口当前都不存在。文章修改和权限审批分开：管理员只能通过账号 API 批准身份与发布资格；文章在作者本机编辑，通过 SSH Git 发布。

## 文章与资源

文章由作者仓库的 `index.toml` 登记，结构、字段和多页样例见[文章发布](publishing.md)。站内 `:slug` 为 `作者名~文章ID`，目录或入口改名不会改变 ID。

`GET /api/posts/:slug` 返回入口正文。`markdown` 是正文源文本，服务端不再把 Frontmatter 解析为元信息，`entry` 是入口文件名，`pages` 和 `assets` 提供该文章可达页面及已导入图片的 URL。`format` 为 `markdown` 或 `text`，阅读端不得将纯文本直接当作 HTML。

`meta` 包含 `title/description/tags/cover/slug/author/created_at/date/updated/status`。`cover` 是文件名或 null，对应 `assets` 中的资源。`author` 由服务端认证身份填写；`created_at` 是首次导入时间，`date` 是首次公开时间，未公开时为空字符串；`updated` 是元信息、可达页面或有效图片实际变化的时间。时间使用 UTC RFC 3339 字符串，不接受作者在索引或 Git 提交中指定。

页面链接如 `chapter.md#part` 由阅读端根据 `pages` 映射到页面 URL，并保留锚点。非公开内容、未登记目录、未链接页面与未引用图片不会由公共接口返回。移除索引条目或入口缺失时，页面和资源入口同时撤回。

## 同步与暂存状态

`/api/admin/status` 的 `commit` 和 `indexed_commit` 为整个生效作者集合的版本摘要，不是单个作者的 Git HEAD。`pending` 的格式为 `{作者名: {commit, reason}}`，仅管理员可读。TOML 无效时保留原生效版本及其数据库记录，暂存状态跨进程重启保存；修复后的正常推送会替换暂存状态并重新导入。

作者的源提交记录在数据库 `blog_articles.git_commit` 和私有集合快照中。数据库失败时 Git 接收与已导入文章阅读仍可继续，恢复后自动重建。站长审核暂未启用。
