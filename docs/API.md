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


## 认证与权限

登录返回 Cookie 和 `csrf`，修改请求需要 Origin 和 `X-CSRF-Token`。会话有效期为一小时，服务重启后需要重新登录。

账户权限为 MEMBER、AUTHOR、MASTER 和 BANNED，分别对应普通用户、作者、管理员和封禁用户。普通注册不授予发布权限，管理员在账户页授权作者。

## 文章访问

`:slug` 使用 `用户名~文章ID`，网页地址使用 `/posts/用户名/文章ID`。文章响应包含元数据、正文、页面和资源列表。

| 可见性 | 读取条件 |
| --- | --- |
| `default` | 公开 |
| `secret` | 登录 |
| `confidential` | 登录并通过口令解锁，固定全隐藏 |
| `top` | 作者或管理员指定的用户 |

作者可读自己的文章，管理员可读全部文章。`hidden=semi` 可显示受限文章摘要，`full` 隐藏条目和元信息。`review=blocked` 对普通读者返回 404。

口令解锁和指定授权跨会话保留。修改口令会撤销旧口令授权，离开绝密等级后会撤销指定授权。

## Git 发布

`/git/content.git` 使用 HTTP Basic 验证网站账号密码，并按账号映射作者仓库。发布格式见[文章发布](publishing.md)，容量限制见[安全文档](security.md)。

## 错误响应

业务错误返回 `{"error":"说明"}`。常见状态码如下：

| 状态码 | 含义 |
| --- | --- |
| 401 | 需要登录或凭据错误 |
| 403 | 权限不足或来源校验失败 |
| 404 | 资源不存在或不可见 |
| 409 | 安装或数据冲突 |
| 429 | 请求过于频繁 |
| 503 | 数据库服务故障 |

受限正文还返回 `locked=true`，`reason` 为 `login`、`password` 或 `denied`。仅半隐藏文章返回元数据。
