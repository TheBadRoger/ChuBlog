# 运维与备份

[返回项目首页](../README.md)

## 持久化数据

| 持久卷 | 内容 |
| --- | --- |
| `content` | 私有 Git 缓存 `authors/<用户名>/content.git`、`collection.json`、公开 `published.json` |
| `ingress` | 作者接收仓库 `authors/<用户名>.git` |
| `database` | ChuSQL 数据，包括账户、权限、网站元数据与安装状态及文章派生索引 |
| `db-logs` | ChuSQL 日志 |

账户只保存在 ChuSQL。`blog_users` 保存密码哈希、账户时间和 `blog_permission` ENUM 权限，不存在文件账户或独立授权卷。应用日志输出 stdout，由 Docker 分卷轮换，不记录口令或会话。

## 数据库初始化与版本要求

数据库依赖固定为支持自定义 ENUM 的 ChuSQL 版本。生产部署通过 `scripts/deploy.sh` 启动数据库并执行初始化；不需要宿主机 Cargo 或 GHC。

初始化 SQL 是以下独立文件，修改后重新构建部署：

- [`create-database.sql`](../apps/api/sql/create-database.sql)：创建博客数据库。
- [`accounts.sql`](../apps/api/sql/accounts.sql)：创建 `blog_permission` ENUM、`blog_users` 账户表与 `blog_site` 安装记录表。
- [`articles.sql`](../apps/api/sql/articles.sql)：创建可重建的文章索引表。

SQL 编译到 API 镜像中。重复初始化保留已有类型和表，不会自动 ALTER、清空或转换旧结构。本版本以新建 ENUM 结构为准，不包含整数权限转换、JSON 导入、旧文章表升级或迁移标记。旧版测试环境需要使用新数据库卷部署，不要将旧卷当作本版本的可用数据源；部署脚本不会擅自删除旧卷。

## 安装状态

首次安装见[部署文档](deployment.md#首次安装网站)。`content/setup-token` 是权限为 `600` 的安装密钥文件，不保存站长密码；`blog_site` 保存唯一站长 ID、网站名称、简介、安装时间与首次文章释放提交 `starter_commit`。安装事务真正创建 `blog_users` 站长记录；首次文章仅在该事务提交后写入此用户的个人仓库。以数据库中的单例记录判断是否完成安装，不能通过删除密钥文件重新安装。完整备份需要保留这些内容，站长账户与安装状态应同时恢复。此版本不提供网页重置安装或第二站长功能。

首次释放成功后，`starter_commit` 不再为空。站长之后可修改或撤回教程和进度；不要清除此字段，否则会重新触发首次释放检查。字段为空时后台会重试，但遇到已有非模板内容会拒绝覆盖。站长初始化完成前的 Git 访问会先检查并完成仓库准备，避免与首次写入并发。

## 数据库故障与索引重建

- 数据库不可用时，注册、登录、账户授权及 Git clone/push 不能继续。API 返回 `503`，接收 Hook 验证失败时拒绝推送。
- 已导入文章由阅读快照提供，数据库故障和 API 重启不影响这些公共内容的读取。恢复后自动重试账户访问与文章同步。
- `blog_articles` 保存有效登记文章（含草稿）的作者、本地 ID、正文路径、格式、源提交、封面和系统时间；`blog_pages` 保存有效页面路径，`blog_article_tags` 保存全部文章标签；`blog_posts/blog_tags` 保存公开检索索引。
- 索引同步失败至少间隔 30 秒重试，并定期检查索引是否丢失。`indexed_commit:null` 表示尚无已确认可用的索引，旧值表示尚未追平当前内容。
- 可等待在线自动重试，也可停止使用同一数据卷的 API 后运行 `chublog-api rebuild-index`。重建只修改文章派生表，不清空账户或 ENUM 类型。账户不能从 Git 或阅读快照重建。

## 备份与恢复

停止写入并备份 `.env`、`content`、`ingress`、`database` 和 `db-logs`。备份包含密码哈希和数据库口令，需要加密并限制访问权限。恢复时先停止服务，从同一一致备份恢复各卷，再启动并检查账户、权限和文章。

`collection.json` 保存生效版本、暂存状态和服务端生成的时间，必须备份。丢失后可从有效作者仓库重新导入正文，但无法还原原始系统时间；作者当前索引损坏时还需要上一个生效版本。不要将它当作可随意丢弃的缓存。

进程重启会注销所有会话。文件写入采用原子替换与 fsync，Git 引用采用受控更新；磁盘故障和备份恢复仍需真实环境演练。不要对含正式数据的项目执行 `docker compose down -v`。

## 数据库 CLI

需要交互式 SQL 时，使用 `scripts/deploy.sh --with-cli`，连接方法见[部署文档](deployment.md#可选安装数据库-cli)。数据库不映射宿主机端口。
