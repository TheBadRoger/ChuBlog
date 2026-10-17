# 数据库结构与类型

[返回项目首页](../README.md)

类型选型依据 ChuSQL 的[类型文档](../external/ChuSQL/docs/types.md)和 [SQL 语法](../external/ChuSQL/docs/syntax.md)。本机上游源码文档位于 `../ChuSQL/docs/`。ChuSQL 预装整数、有限长字符串、文本、布尔、浮点、十进制、日期、时间戳、二进制等类型，同时支持自定义 ENUM 和 DOMAIN。博客只使用当前字段需要的类型，不用 TEXT 代替所有列。

## 字段选型

| 数据 | 声明与约束 |
| --- | --- |
| 用户 ID、站长 ID、安装单例 ID | INT；用户 ID 自增，账号唯一 |
| 账号 | VARCHAR(32)，应用限制 ASCII 和用户名规则 |
| 完整密码 PHC | VARCHAR(255)，保留 Argon2id 参数、盐与哈希，不截断 |
| 网站名称、简介 | VARCHAR(120)、VARCHAR(1000) |
| 账户与安装时间 | TIMESTAMP；上次登录可为 NULL |
| 权限 | 命名 ENUM `blog_permission`：BANNED、MEMBER、AUTHOR、MASTER |
| 文章 ID / slug | VARCHAR(40) / VARCHAR(73)，后者覆盖 32 位作者名、分隔符和文章 ID |
| 标题、简介、标签 | VARCHAR(240)、VARCHAR(1000)、VARCHAR(80) |
| 路径 | VARCHAR(192)，覆盖受校验的 posts/目录/文件名 |
| 入口、页面名、封面 | VARCHAR(100)；无封面为 NULL |
| Git 提交 / 首次释放提交 | CHAR(40)，当前 Git SHA-1 对象 ID；未释放时为 NULL |
| 文章状态、文档格式 | ENUM `blog_article_status` 和 `blog_document_format` |
| 文章时间 | TIMESTAMP；从未公开时 published_at 为 NULL |

VARCHAR 和 CHAR 的声明限制字符长度，应用的 UTF-8 字节限制仍独立保留，二者均需满足。ChuSQL CHAR 不自动补空格。主键和唯一索引键还受数据库的字节上限限制，不能只依赖 VARCHAR 长度。当前 slug 为最多 73 个 ASCII 字节，账号最多 32 个 ASCII 字节。

文章正文与图片保存在 Git 仓库及阅读快照中，不作为数据库长文本或 BLOB 写入。数据库目前没有长行溢出存储，默认页为 4096 字节，因此单字段上限不能代替整行限制。未实现的评论和互动不预建虚假表。

## 时间边界

当前 ChuSQL TIMESTAMP 接受 `YYYY-MM-DD HH:MM:SS` 或相同秒级格式的 T 分隔形式，不接受带毫秒、Z 或时区偏移的 RFC 3339。写入时统一转换为 UTC 秒级；账户查询结果再转换为 `YYYY-MM-DDTHH:MM:SSZ`。网站不依赖服务器本地时区。

文章发布快照使用 UTC RFC 3339，并可保留毫秒；数据库索引转换为秒级，不反向覆盖文章快照。无发布时间使用 SQL NULL，不写空字符串或伪造日期。

## 安装与站长

`apps/api/sql/accounts.sql` 创建账户、网站安装记录和权限类型。安装 API 在一个事务中向 `blog_users` 插入站长，权限为 MASTER，再用查询返回的真实用户 ID 写入 `blog_site.master_user_id`。没有文件账户、默认虚拟站长或浏览器内建号。单例安装主键与授权规则保持站长唯一。

事务提交后，从数据库读取站长账号，为其独立 Git 仓库释放两篇内置文章。成功提交记入 `blog_site.starter_commit`，用于重试恢复与防止重复覆盖；两篇文章的作者与其他文章一样由仓库所属账户确定。文章索引重建不会删除账户、安装记录或该字段。

## 初始化文件

- `apps/api/sql/create-database.sql`：数据库。
- `apps/api/sql/accounts.sql`：持久账户、安装记录与权限 ENUM。
- `apps/api/sql/articles.sql`：可重建索引，以及状态、文档格式 ENUM。

SQL 编译到镜像，修改后重新构建部署。重复初始化保留已有对象，不执行旧类型转换或历史数据迁移。开发中的旧表请使用新的空数据库验证，脚本不会擅自清空旧数据。
