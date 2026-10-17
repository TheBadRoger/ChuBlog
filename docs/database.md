# 数据库

[返回项目首页](../README.md)

类型选型依据 ChuSQL 的[类型文档](../external/ChuSQL/docs/types.md)和 [SQL 语法](../external/ChuSQL/docs/syntax.md)，本机上游文档位于 `../ChuSQL/docs/`。账户、网站配置和权限需要持久保存，文章索引可以从 Git 与快照重建。

## 字段类型

| 数据 | 类型与约束 |
| --- | --- |
| 用户 ID、站长 ID、安装 ID | INT，用户 ID 自增 |
| 昵称 | VARCHAR(128)，可重复的 UTF-8 展示属性 |
| 账号、密码 PHC | VARCHAR(32)、VARCHAR(255)，账号由 3–32 位小写英文字母组成且唯一，PHC 保存完整参数、盐和哈希 |
| 网站名称、简介 | VARCHAR(120)、VARCHAR(1000) |
| 账户与安装时间 | TIMESTAMP，上次登录可为 NULL |
| 权限 | ENUM blog_permission：BANNED、MEMBER、AUTHOR、MASTER |
| 文章 ID、完整 slug | VARCHAR(40)、VARCHAR(73) |
| 标题、简介、标签 | VARCHAR(240)、VARCHAR(1000)、VARCHAR(80) |
| 路径、文件名 | VARCHAR(192)、VARCHAR(100) |
| Git 提交、首次文章释放提交 | CHAR(40)，首次释放前为 NULL |
| 文章状态、格式 | ENUM blog_article_status、blog_document_format |
| 文章时间、封面 | TIMESTAMP、VARCHAR(100)，首次公开前或缺少封面时为 NULL |

VARCHAR 和 CHAR 校验字符长度，应用另行校验 UTF-8 字节数。CHAR 保留原字符串长度。索引键另受数据库字节上限约束，最大 slug 为 73 个 ASCII 字节。默认存储页为 4096 字节，整行也需满足页容量；正文和图片保存在 Git 与快照中。

## 时间格式

ChuSQL TIMESTAMP 使用秒级 `YYYY-MM-DD HH:MM:SS`，也接受 T 分隔形式。应用写入时统一转换为 UTC 秒级，账户 API 返回 `YYYY-MM-DDTHH:MM:SSZ`。文章快照可保留毫秒，派生索引按秒保存。首次公开前的数据库时间使用 SQL NULL。

## 初始化与站长

SQL 编译进 API 镜像，修改后重新构建部署：

- [create-database.sql](../apps/api/sql/create-database.sql)：数据库。
- [accounts.sql](../apps/api/sql/accounts.sql)：账户、安装记录和权限 ENUM。
- [complete-empty-install.sql](../apps/api/sql/complete-empty-install.sql)：账户与安装记录都为空时，重建这两张空表，再由 accounts.sql 创建当前结构。
- [articles.sql](../apps/api/sql/articles.sql)：文章索引、状态和格式 ENUM。

安装事务向 blog_users 插入 MASTER，再用真实用户 ID 写入 blog_site.master_user_id。提交后，系统读取站长账号，为其 Git 仓库释放两篇文章，提交记录保存在 blog_site.starter_commit。索引重建保留账户和安装数据。

初始化按身份分工：数据库 root 创建 ENUM 并清理空安装表，博客账号 chublog_index 创建表并执行日常读写。普通账号保留 CONNECT、CREATE 和自身表权限，root 凭据仅供初始化任务使用。

重复初始化复用已有对象；账户与安装记录都为空时，暂停 API 写入后按当前 accounts.sql 重建这两张空表，使权限列使用 blog_permission ENUM；已有账户或安装记录时保留表与数据。部署会检查账户与安装表的必需字段，包括 nickname。首次安装以空账户表和本版 SQL 结构为前提，开发中的类型变更使用新的测试数据库验证。备份要求见[运维文档](operations.md)。
