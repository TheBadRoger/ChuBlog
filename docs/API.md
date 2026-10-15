# 后端 API

全部正文使用 JSON。同源 `Origin` 必须等于 `PUBLIC_ORIGIN`，例如 `https://blog.example.com`。响应含 `Cache-Control: no-store`。登录 Cookie 为 `HttpOnly; SameSite=Strict; Secure`，仅 localhost HTTP 开发场景省略 Secure。

| 方法 | 路径 | 权限 / 请求 |
| --- | --- | --- |
| GET | `/api/health` | 公共，API 存活，不代表数据库或发布同步健康 |
| GET | `/api/posts` | 公共，`{commit,posts:[metadata]}`，仅已发布，日期倒序 |
| GET | `/api/posts/:slug` | 公共，`{meta,markdown,assets:[{name,url}]}`，草稿及不存在均 404 |
| GET | `/api/assets/:slug/:name` | 公共，仅已发布文章的已验证图片 |
| GET | `/api/stats` | 公共，文章数、标签数、正文源字节数、启动以来文章请求数 |
| POST | `/api/auth/register` | 公共 + Origin；`{username,password}`，创建 reader；可配置关闭 |
| POST | `/api/auth/login` | 公共 + Origin；`{username,password}`，返回 `{username,role,csrf}` 及 Cookie |
| POST | `/api/auth/logout` | 登录 + Origin + CSRF；使会话失效并清除 Cookie |
| GET | `/api/account` | 登录；返回自己的 `{username,permissions,csrf}`，不含口令哈希 |
| PUT | `/api/account/ssh-key` | 登录 + Origin + CSRF；`{public_key}`，替换自己的唯一 Ed25519 公钥 |
| PUT | `/api/account/password` | 登录 + Origin + CSRF；`{old_password,new_password}`，成功后全部会话失效 |
| GET | `/api/admin/users` | 管理员；返回用户启用状态、角色、前缀及公钥，不含哈希 |
| PATCH | `/api/admin/users/:username` | 管理员 + Origin + CSRF；`{enabled,role,prefixes}` |
| GET | `/api/admin/status` | 管理员；`{commit,total_posts,drafts,indexed_commit}` |

角色：`reader | publisher | admin`。网站管理员与数据库账号、OS 用户不是同一个系统。发布角色仍需已登记公钥和非空文章前缀才能使用 SSH。网站登录不产生 Git 访问凭证。

用户名：3–32 位小写字母、数字、短横线，以字母开头；口令：12–1024 字节；前缀：2–64 位小写英文、数字、短横线且以短横线结尾，最多 8 个。注册不接受 `role` 等额外字段。公钥不能与别的用户重复。

修改请求的头为 `Origin: https://blog.example.com` 和 `X-CSRF-Token: <登录返回的 csrf>`，并携带 Cookie。没有配置跨域 CORS，也没有浏览器本地持久存储令牌的要求。授权或停用用户会使该用户既有会话失效；再次登录后才能继续使用自己的账户接口。

普通业务错误返回 `{ "error": "可读说明" }`；401 未登录/口令错误，403 Origin/CSRF/权限错误，404 不存在或非公开内容，409 用户名冲突，429 超过限流。Axum 的无效 JSON/字段结构错误使用框架的 400/415/422 响应，超过请求体上限返回 413。服务器内部错误不返回路径、凭据或数据库详情。

文章写入、上传、在线预览、后台发布/删除接口当前都不存在。文章修改和权限审批分开：管理员只能通过账号 API 批准身份与命名空间；文章在作者本机编辑，通过 SSH Git 发布。
