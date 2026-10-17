# 后端 API

[返回项目首页](../README.md)

## 请求约定

- 普通账户和阅读接口使用 JSON；Git Smart HTTP 使用其标准二进制协议。
- 同源 `Origin` 必须与可信反向代理传入的原始协议、主机及端口完全一致，例如 `https://blog.example.com`；无需域名环境变量。转发头的信任边界见[部署说明](deployment.md)。
- 响应含 `Cache-Control: no-store`。
- 登录 Cookie 为 `HttpOnly; SameSite=Strict; Secure`，仅 localhost HTTP 开发场景省略 Secure。

## 接口一览

| 方法 | 路径 | 权限 / 请求 |
| --- | --- | --- |
| GET | `/api/site` | 公共；`{installed,site:{name,description}或null,owner}`，不含安装密钥、站长 ID 或凭据 |
| POST | `/api/setup` | 公共 + Origin + 安装密钥；首次创建网站与唯一站长，见首次安装说明 |
| GET | `/api/health` | 公共，API 存活，不代表数据库或发布同步健康 |
| GET | `/api/posts` | 公共，`{commit,posts:[metadata]}`，仅已发布，日期倒序 |
| GET | `/api/posts/:slug` | 公共，`{meta,markdown,format,author,article_id,entry,pages:[{name,url}],assets:[{name,url}]}`，草稿及不存在均 404 |
| GET | `/api/posts/:slug/pages/:name` | 公共，指定页面的正文、元信息、格式、页面列表和资源地址；非公开或不存在均 404 |
| GET | `/api/assets/:slug/:name` | 公共，仅已发布文章的已验证图片 |
| GET | `/api/stats` | 公共，文章数、标签数、正文源字节数、启动以来文章请求数 |
| POST | `/api/auth/register` | 公共 + Origin；`{username,password}`，安装后创建 MEMBER；可配置关闭 |
| POST | `/api/auth/login` | 公共 + Origin；`{username,password}`，返回 `{id,username,created_at,last_login_at,permission_level,role,csrf}` 及 Cookie |
| POST | `/api/auth/logout` | 登录 + Origin + CSRF；使会话失效并清除 Cookie |
| GET | `/api/account` | 登录；返回自己的 `{id,username,created_at,last_login_at,permission_level,permissions,csrf}`，不含口令哈希 |
| PUT | `/api/account/password` | 登录 + Origin + CSRF；`{old_password,new_password}`，成功后全部会话失效 |
| GET | `/api/admin/users` | 管理员；返回用户元信息、ENUM 权限、启用状态与角色，不含哈希 |
| PATCH | `/api/admin/users/:username` | 管理员 + Origin + CSRF；`{enabled,role}` |
| GET | `/api/admin/status` | 管理员；`{commit,total_posts,drafts,indexed_commit,pending}` |

## 权限与账户

- 角色：`reader | publisher | admin`。
- 网站管理员与数据库账号、OS 用户不是同一个系统。
- Git 访问独立验证网站账号密码及发布角色，不依赖浏览器登录会话。
- 每个账户使用自己的仓库。
- 网站登录不产生 Git 访问凭证。

用户名：3–32 位小写字母、数字、短横线，以字母开头；口令：12–1024 字节。注册不接受 `role` 等额外字段。

## 修改请求

- 修改请求的头为 `Origin: https://blog.example.com` 和 `X-CSRF-Token: <登录返回的 csrf>`，并携带 Cookie。
- 没有配置跨域 CORS，也没有浏览器本地持久存储令牌的要求。
- 授权或停用用户会使该用户既有会话失效；再次登录后才能继续使用自己的账户接口。

## 错误响应

- 普通业务错误返回 `{ "error": "可读说明" }`；401 未登录/口令错误，403 Origin/CSRF/权限错误，404 不存在或非公开内容，409 用户名冲突，429 超过限流。
- Axum 的无效 JSON/字段结构错误使用框架的 400/415/422 响应，超过请求体上限返回 413。
- 服务器内部错误不返回路径、凭据或数据库详情。

## 文章发布边界

文章写入、上传、在线预览、后台发布/删除接口当前都不存在。文章修改和权限审批分开：管理员只能通过账号 API 批准身份与发布资格；文章在作者本机编辑，通过 HTTPS Git 发布。

## 文章与资源

文章由作者仓库的 `index.toml` 登记，结构、字段和多页样例见[文章发布](publishing.md)。站内 `:slug` 为 `作者名~文章ID`，目录或入口改名不会改变 ID。

`GET /api/posts/:slug` 返回入口正文。`markdown` 是正文源文本，服务端不再把 Frontmatter 解析为元信息，`entry` 是入口文件名，`pages` 和 `assets` 提供该文章可达页面及已导入图片的 URL。`format` 为 `markdown` 或 `text`，阅读端不得将纯文本直接当作 HTML。

`meta` 包含 `title/description/tags/cover/slug/author/created_at/date/updated/status`。`cover` 是文件名或 null，对应 `assets` 中的资源。`author` 由服务端认证身份填写；`created_at` 是首次导入时间，`date` 是首次公开时间，未公开时为空字符串；`updated` 是元信息、可达页面或有效图片实际变化的时间。时间使用 UTC RFC 3339 字符串，不接受作者在索引或 Git 提交中指定。

页面链接如 `chapter.md#part` 由阅读端根据 `pages` 映射到页面 URL，并保留锚点。非公开内容、未登记目录、未链接页面与未引用图片不会由公共接口返回。移除索引条目或入口缺失时，页面和资源入口同时撤回。

## 同步与暂存状态

`/api/admin/status` 的 `commit` 和 `indexed_commit` 为整个生效作者集合的版本摘要，不是单个作者的 Git HEAD。`pending` 的格式为 `{作者名: {commit, reason}}`，仅管理员可读。TOML 无效时保留原生效版本及其数据库记录，暂存状态跨进程重启保存；修复后的正常推送会替换暂存状态并重新导入。

作者的源提交记录在数据库 `blog_articles.git_commit` 和私有集合快照中。数据库不可用时，已导入文章仍可通过阅读快照访问；登录、注册、权限检查和 Git clone/push 返回不可用或拒绝操作，绝不回退文件账户。恢复后自动重建文章索引。站长审核暂未启用。

## Git Smart HTTP

入口为 `/git/content.git`，仅接受以下协议请求：

| 方法 | 路径 |
| --- | --- |
| GET | `/git/content.git/info/refs?service=git-upload-pack` |
| GET | `/git/content.git/info/refs?service=git-receive-pack` |
| POST | `/git/content.git/git-upload-pack` |
| POST | `/git/content.git/git-receive-pack` |

每个请求均验证 HTTP Basic 网站账号密码和独立发布授权。缺少或错误凭据返回 `401` 与 `WWW-Authenticate: Basic`；未获发布权或停用账户返回 `403`；不支持的仓库路径返回 `404`。不提供匿名或 dumb HTTP 文件读取，也不接受浏览器 Cookie 代替 Git 密码。公网必须由可信代理提供 HTTPS。

Rust 实现 HTTP 认证、固定路径映射、CGI 环境隔离和响应解析，调用 Git 自带 `git http-backend` 执行 Smart HTTP 协议。请求内容不作为 shell 命令执行。失败响应不返回口令或进程诊断。

## 账户存储与权限

账户只保存在 ChuSQL 的 `blog_users` 表中。创建 SQL 位于 [`apps/api/sql/accounts.sql`](../apps/api/sql/accounts.sql)，由 `db-init` 初始化执行；不读取或迁移 JSON 账户文件，不创建迁移标记或兼容表。

| 列 | 类型与含义 |
| --- | --- |
| `id` | INT，自增主键 |
| `username` | VARCHAR(32)，唯一且非空的账号 |
| `password_hash` | VARCHAR(255)，完整 Argon2id PHC，含随机盐和参数，不是可解密密码 |
| `created_at` | TIMESTAMP，UTC 秒级创建时间 |
| `last_login_at` | TIMESTAMP，可空，最近一次成功网站登录时间；Git 验证不更新此列 |
| `permission_level` | 自定义 ENUM `blog_permission`，非空，默认 `MEMBER` |

权限类型由 `CREATE TYPE blog_permission AS ENUM ('BANNED', 'MEMBER', 'AUTHOR', 'MASTER')` 定义。安装完成后普通注册只创建 `MEMBER`，不会自动提权。首次安装由专用接口创建唯一 `MASTER`。

| 枚举值 | 账户状态与已实现权限 | 账户 API 的 role / enabled |
| --- | --- | --- |
| `BANNED` | 封禁，不能登录或访问 Git | `reader` / `false` |
| `MEMBER` | 正常用户，可登录和管理自己的密码 | `reader` / `true` |
| `AUTHOR` | 包含普通用户权限，可克隆和推送自己的文章仓库 | `publisher` / `true` |
| `MASTER` | 包含作者权限，可管理账户授权和查看同步状态，不能创建其他站长 | `admin` / `true` |

账户接口返回的 `permission_level` 是上述枚举标签字符串，不是整数。`role` 和 `enabled` 是前端授权表单使用的视图字段；禁用映射到 `BANNED`，启用时按指定 role 映射到对应枚举。评论、点赞、收藏、内部文章和文章管理扩展尚未实现，枚举等级不会提前开放这些接口。

注册限制用户名为 3–32 位小写字母、数字和短横线，以字母开头；密码为 12–1024 字节。注册、登录和管理员列表返回 ID、账号、创建时间、上次登录时间与权限，不返回密码哈希。

网站登录使用随机 256 位会话令牌、HttpOnly/SameSite=Strict Cookie，公网 HTTPS 下附带 Secure。会话仅存内存，一小时到期，重启后失效；密码修改和撤权会注销相关会话。修改操作另须通过 Origin 和 CSRF 校验。每次受保护请求和 Git 接收 Hook 都重新查询数据库，封禁、改密或撤权会阻止旧凭据继续使用。数据库访问失败返回 `503`，Hook 验证失败拒绝推送；已导入的公共文章仍可阅读。

## 首次安装

`POST /api/setup` 接收 `{token,name,description,username,password}`。名称去除两端空白后需要 1–120 字节，简介最多 1000 字节，两者不接受控制字符；用户名和密码沿用账户限制。站长密码使用随机盐 Argon2id 哈希，接口不返回密码或哈希。

安装前 `GET /api/site` 返回 `{installed:false,site:null,owner:null}`。安装后 `owner` 为数据库中的站长用户名，供首页生成内置文章链接。安装密钥只保存在服务器 `/data/setup-token`，获取方法见[部署文档](deployment.md#首次安装网站)。请求必须通过同源 Origin 校验；安装与登录注册共用全局每分钟 10 次请求限额和最多 2 个密码哈希任务。

接口在单个 ChuSQL 事务中创建 `MASTER` 用户并写入 ID 为 1 的 `blog_site` 记录。单例主键阻止并发安装重复提交，失败事务不留下额外站长。安装完成后 POST 返回 `409`，`/setup` 页面跳转登录页；数据库不可用返回 `503`。安装接口不用于改密、重置或修改站点信息。

`PATCH /api/admin/users/:username` 不能将其他账户设为 `admin/MASTER`，也不能封禁或降级站长自己。网站名称、简介由前端按文本转义输出，不接受 HTML 配置。

## 字段类型与内置文章

SQL 类型与数据库边界格式见[数据库结构](database.md)。数据库 TIMESTAMP 使用 UTC `YYYY-MM-DD HH:MM:SS`，账户 API 转为 UTC RFC 3339；文章 API 的系统时间来自 Git 发布快照，可以保留毫秒，索引落库时转换为秒级。未公开文章的 `published_at` 和无封面的 `cover` 为 SQL NULL。

首次安装的站长通过 `INSERT INTO blog_users ... permission_level = 'MASTER'` 真正入库，再以新用户 ID 写入 `blog_site.master_user_id`。提交后读取该站长记录，为其个人仓库写入 `publishing` 与 `development` 两篇公开文章；安装响应包含 `starter_articles_pending`，表示仓库初始化是否需要后台重试。实际公开阅读由正常内容导入流程完成，通常需等待下一轮同步。

`blog_site.starter_commit` 记录首次释放完成的 Git 提交，重启不重置、不覆盖作者修改或补回已撤回文章。文章的公共地址为 `/posts/站长用户名~publishing` 和 `/posts/站长用户名~development`，各页与其他文章使用相同的访问控制。原 `/publishing` 独立页面已删除，不再提供特殊渲染路由。
