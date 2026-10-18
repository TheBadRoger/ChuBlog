# 后端 API

[返回项目首页](../README.md)

普通接口使用 JSON，Git 使用 Smart HTTP。响应设置 `Cache-Control: no-store`。写请求使用与可信代理来源一致的 Origin；会话内修改另携带 Cookie 和 `X-CSRF-Token`。

## 接口

| 方法 | 路径 | 权限与请求 |
| --- | --- | --- |
| GET | `/api/health` | 公共，返回 API 存活状态 |
| GET | `/api/site` | 公共，返回 `{installed,site,owner}`；site 为 `{name,description}` 或 null |
| POST | `/api/setup` | Origin + 安装密钥；`{token,name,description,username,nickname,password}` |
| GET | `/api/posts` | 公共，返回 `{commit,posts:[metadata]}`；按默认/秘密/机密/绝密与隐藏等级过滤，作者和管理员可见自己或全部文章 |
| GET | `/api/posts/:slug` | 先审核后可见性；受限正文返回 401/403 锁定 JSON；返回入口及页面、资源列表 |
| GET | `/api/posts/:slug/pages/:name` | 与文章入口相同的读取权限 |
| GET | `/api/assets/:slug/:name` | 与文章入口相同的读取权限 |
| GET | `/api/stats` | 公共，文章数、标签数、正文大小与启动以来文章请求数 |
| POST | `/api/auth/register` | 安装后 + Origin；`{username,nickname,password}`，创建 MEMBER |
| POST | `/api/auth/login` | Origin；`{username,password}`，返回用户信息、role、csrf 及 Cookie |
| POST | `/api/auth/logout` | 登录 + Origin + CSRF，注销会话 |
| GET | `/api/account` | 登录，返回用户信息、permissions 和 csrf |
| PUT | `/api/account/password` | 登录 + Origin + CSRF；`{old_password,new_password}` |
| GET | `/api/admin/users` | 管理员，返回用户信息、enabled 和 role |
| PATCH | `/api/admin/users/:username` | 管理员 + Origin + CSRF；`{enabled,role}` |
| GET | `/api/admin/status` | 管理员，返回 `{commit,total_posts,drafts,indexed_commit,pending}` |
| GET | `/api/admin/articles` | 管理员，返回全部文章元信息和管理状态，不返回口令哈希 |
| PATCH | `/api/admin/articles/:slug` | 管理员 + Origin + CSRF；`{featured?,visibility?,hidden?,password?,review?}` |
| DELETE | `/api/admin/articles/:slug` | 管理员 + Origin + CSRF；封禁文章，不删除 Git 历史 |
| GET | `/api/articles` | 登录文章作者；返回自己的全部文章（含草稿与已封禁）及本人暂存索引 `{posts,pending}`，供作者管理文章 |
| PATCH | `/api/articles/:slug` | 文章作者 + Origin + CSRF；`{visibility?,hidden?,password?}`，作者可设置隐藏等级 |
| GET | `/api/articles/:slug/access` | 作者或管理员；返回绝密文章的授权用户及来源 |
| POST | `/api/articles/:slug/access` | 作者或管理员 + Origin + CSRF；`{username,allowed}`，增删绝密文章授权 |
| POST | `/api/posts/:slug/unlock` | 登录 + Origin + CSRF；`{password}`，验证机密文章口令并持久记录访问权 |


## 账户与权限

用户信息包含 `id,username,nickname,created_at,last_login_at,permission_level`，密码哈希仅在数据库中保存。用户名为 3–32 位小写字母、数字、下划线或减号，首位为小写字母、下划线或减号且唯一；昵称为 1–128 个 UTF-8 字节，可以重复，作为展示属性保存。登录、授权和仓库路径均使用用户名及用户 ID。密码为 6–128 个 Unicode 字符。

| permission_level | role / enabled | 权限 |
| --- | --- | --- |
| BANNED | reader / false | 封禁，登录与 Git 验证返回 403 |
| MEMBER | reader / true | 登录、管理自己的密码 |
| AUTHOR | publisher / true | 普通用户权限及个人仓库发布权 |
| MASTER | admin / true | 作者权限、账户授权和同步状态查询 |

安装创建唯一 MASTER，普通注册创建 MEMBER。授权接口维持管理员唯一，并保留管理员自身权限。账户和权限以 ChuSQL 为准，受保护请求及 Git Hook 均查询实时记录。

会话使用随机 256 位令牌，Cookie 属性为 HttpOnly、SameSite=Strict，HTTPS 下附带 Secure。会话期限为一小时，进程重启、改密或撤权后失效；CSRF 保存在浏览器内存。注册开关由 `ALLOW_REGISTRATION` 控制。

## 首次安装

安装密钥的获取方法见[部署文档](deployment.md#首次安装网站)。网站名称为 1–120 字节，简介最多 1000 字节，两者去除两端空白并校验控制字符。

安装事务创建真实管理员账户及 `blog_site` 单例记录，完成后再次提交返回 409。响应包含 `starter_articles_pending`，为 true 时由后台继续准备内置文章。`owner` 返回管理员用户名，首页据此生成文章链接。已有账户与安装状态冲突时返回 409。数据库故障按权限、凭据、结构、连接或数据校验分类提示；日志记录原因分类和操作阶段。

## 文章与同步

网页入口为 `/posts/用户名/文章ID`；内部 API 的 `:slug` 为 `作者名~文章ID`，保留作者间同名文章的隔离。文章响应包含 `meta,markdown,format,entry,pages,assets,author,article_id`；页面响应另含页面名。`format` 为 markdown 或 text，pages 和 assets 提供名称与 URL。

元数据包含 `title,description,tags,cover,slug,author,author_name,author_is_admin,created_at,date,updated,status,featured,visibility,hidden,review,deleted,password_required`。`visibility` 为 `default|secret|confidential|top`，`hidden` 为 `semi|full`，`review` 为 `open|blocked`；`deleted` 是 `review == blocked` 的兼容布尔值。author 保留用户名身份，author_name 提供用于展示的数据库昵称，author_is_admin 表示作者当前在数据库中是否为管理员。系统记录作者和 UTC RFC 3339 时间；date 为首次公开时间，草稿使用空字符串。cover 为图片名或 null。页面相对链接可映射为返回的 URL，并保留锚点。

`commit` 和 `indexed_commit` 是全站生效集合摘要；`pending` 按作者返回 `{commit,reason}`。错误索引保留原发布版本，修复提交后继续同步。数据库故障时已导入文章继续从快照读取，身份与发布操作返回失败，恢复后自动重试。状态、时间和字段类型见[数据库文档](database.md)。

## Git Smart HTTP

`/git/content.git` 的每次请求都使用 HTTP Basic 网站账号密码及发布权验证，按账号映射个人仓库。入口包括：

| 方法 | 路径 |
| --- | --- |
| GET | `/git/content.git/info/refs?service=git-upload-pack` |
| GET | `/git/content.git/info/refs?service=git-receive-pack` |
| POST | `/git/content.git/git-upload-pack` |
| POST | `/git/content.git/git-receive-pack` |

Rust 网关调用 Git 自带的 `git http-backend`，固定 Hook 检查候选提交。公网使用 HTTPS，文章编辑通过 Git 完成。文件与协议限制见[安全文档](security.md)。

## 错误响应

业务错误返回 `{"error":"说明"}`。常见状态为 401 身份验证失败、403 权限或来源校验失败、404 资源缺失或私有、409 冲突、429 限流、503 数据库服务故障。Git 的 401 同时携带 Basic 认证挑战。

JSON 格式、内容类型与字段错误使用 400/415/422，请求体超限使用 413；内部异常返回通用错误。完整请求限额见[安全文档](security.md)。

## SQL 值编码

所有外部字段通过 ChuSQL 驱动的 `Query.bind()` 进行字面量编码，固定 SQL 结构由代码提供。驱动采用客户端编码，字符串中的单引号按 ChuSQL 规则转为两个单引号。昵称中的引号、SQL 片段、中文、表情及控制字符始终作为值处理；服务端解析与驱动编码由真实数据库测试共同验证。用户输入的 UTF-8 字节限制与数据库字段约束同时生效。

## 文章审核、可见性与隐藏等级

访问顺序固定为：先审核 `review`，再判断 `visibility`，最后按 `hidden` 决定锁定页是否携带元信息。作者（`post.author`）和管理员永远可读自己的文章；这两类身份不参与可见性限制。

| 可见性 | 规则 |
| --- | --- |
| `default` | 所有人可读，公共列表、搜索、统计和公共索引均包含 |
| `secret` | 有效登录用户可读，不设口令 |
| `confidential` | 登录后还需 `blog_article_access` 中的持久授权；授权由口令解锁产生。必须为 `full` 隐藏 |
| `top` | 仅作者、管理员及被显式授权的用户可读，不设口令；`semi` 时未授权用户可在列表看到摘要 |

| 审核 | 规则 |
| --- | --- |
| `open` | 正常按可见性访问，默认值 |
| `blocked` | 对外按“文章已删除”返回 404，不走隐藏页；作者和管理员仍可读取。管理员封禁时记录正文哈希 |

作者修改并推送被 `blocked` 文章的正文后，后台刷新比较 `blocked_hash`；哈希变化则自动改回 `open` 并清除哈希，否则保持封禁。管理员可随时手动解除封禁。

隐藏等级只在文章对当前用户受限时生效：

| 隐藏等级 | 列表与搜索 | 正文页 |
| --- | --- | --- |
| `semi` | 可被搜索并显示元信息（受可见性规则限制） | 服务端不返回正文；前端渲染高斯模糊背景与固定前景锁图标，显示“无权访问”；有口令时显示“需要口令才可访问”及输入框 |
| `full` | 不显示条目与元信息，仅能通过链接路由 | 表现同 `semi`，但不携带元信息 |

`confidential` 只能使用 `full`；显式提交 `semi` 返回 400，未显式提交时服务端自动改为 `full`。隐藏等级只有文章作者可修改。可见性可由作者或管理员修改；`featured` 和 `review` 只有管理员可修改。

### 锁定响应

正文、子页面和图片被可见性拦截时，正文接口返回：

```json
{"error":"无权访问","locked":true,"reason":"denied","meta":null}
```

`reason` 为 `login`（401）、`password`（403）或 `denied`（403）。仅 `semi` 携带 `meta`，`full` 的 `meta` 为 `null`。图片接口只返回对应状态码与 `error`。`blocked` 返回 404 且错误信息为“文章已删除”；非发布文章返回 404“文章不存在”；两者都不进入锁定页。

### 持久访问表

口令解锁成功后写入 `blog_article_access(user_id, article, source='PASSWORD')`；作者或管理员为绝密文章指定用户时写入 `source='GRANTED'`。两种来源分别只对机密和绝密生效，不能互相替代。记录跨会话、退出和重启保留。修改或清除口令会删除该文章的 `PASSWORD` 授权；离开绝密时会删除该文章的 `GRANTED` 授权，需要重新指定。`GET /api/articles/:slug/access` 返回 `{users:[{username,nickname,source}]}`。

管理员设置与审核标记保存在私有 `collection.json`，并投影到 `blog_article_policy` 供数据库查询；作者推送不会覆盖这些设置。后续推送、移除文章后重新使用同一作者及文章 ID，都会继承设置。封禁不等于删除 Git 历史或私有快照；备份必须包含 `collection.json` 和数据库授权表。
