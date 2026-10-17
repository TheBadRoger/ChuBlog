# 后端 API

[返回项目首页](../README.md)

普通接口使用 JSON，Git 使用 Smart HTTP。响应设置 `Cache-Control: no-store`。写请求使用与可信代理来源一致的 Origin；会话内修改另携带 Cookie 和 `X-CSRF-Token`。

## 接口

| 方法 | 路径 | 权限与请求 |
| --- | --- | --- |
| GET | `/api/health` | 公共，返回 API 存活状态 |
| GET | `/api/site` | 公共，返回 `{installed,site,owner}`；site 为 `{name,description}` 或 null |
| POST | `/api/setup` | Origin + 安装密钥；`{token,name,description,username,nickname,password}` |
| GET | `/api/posts` | 公共，返回 `{commit,posts:[metadata]}`，公开文章按日期倒序 |
| GET | `/api/posts/:slug` | 公共，文章入口及页面、资源列表 |
| GET | `/api/posts/:slug/pages/:name` | 公共，文章指定页面 |
| GET | `/api/assets/:slug/:name` | 公共，文章图片 |
| GET | `/api/stats` | 公共，文章数、标签数、正文大小与启动以来文章请求数 |
| POST | `/api/auth/register` | 安装后 + Origin；`{username,nickname,password}`，创建 MEMBER |
| POST | `/api/auth/login` | Origin；`{username,password}`，返回用户信息、role、csrf 及 Cookie |
| POST | `/api/auth/logout` | 登录 + Origin + CSRF，注销会话 |
| GET | `/api/account` | 登录，返回用户信息、permissions 和 csrf |
| PUT | `/api/account/password` | 登录 + Origin + CSRF；`{old_password,new_password}` |
| GET | `/api/admin/users` | 管理员，返回用户信息、enabled 和 role |
| PATCH | `/api/admin/users/:username` | 管理员 + Origin + CSRF；`{enabled,role}` |
| GET | `/api/admin/status` | 管理员，返回 `{commit,total_posts,drafts,indexed_commit,pending}` |

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

`:slug` 为 `作者名~文章ID`。文章响应包含 `meta,markdown,format,entry,pages,assets,author,article_id`；页面响应另含页面名。`format` 为 markdown 或 text，pages 和 assets 提供名称与 URL。

元数据包含 `title,description,tags,cover,slug,author,author_name,created_at,date,updated,status`。author 保留用户名身份，author_name 提供用于展示的数据库昵称。系统记录作者和 UTC RFC 3339 时间；date 为首次公开时间，草稿使用空字符串。cover 为图片名或 null。页面相对链接可映射为返回的 URL，并保留锚点。

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
