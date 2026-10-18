# 数据库

[返回项目首页](../README.md)

ChuSQL 保存账户、网站设置、权限和文章索引。正文与图片保存在 Git 和文章快照中。

## 表结构

表结构以 SQL 文件为准，修改后需要重新构建 API：

| 文件 | 内容 |
| --- | --- |
| [create-database.sql](../apps/api/sql/create-database.sql) | 创建数据库 |
| [accounts.sql](../apps/api/sql/accounts.sql) | 账户、安装记录、文章策略和访问授权 |
| [account-indexes.sql](../apps/api/sql/account-indexes.sql) | 账户查询索引 |
| [articles.sql](../apps/api/sql/articles.sql) | 文章、页面和标签索引 |
| [complete-empty-install.sql](../apps/api/sql/complete-empty-install.sql) | 重建空安装结构 |

账户权限分为 BANNED、MEMBER、AUTHOR、MASTER。首次安装创建唯一管理员，普通注册创建 MEMBER。数据库 root 用于初始化，日常访问使用博客专用账号。

## 数据与授权

`collection.json` 保存文章管理设置，`blog_article_policy` 保存对应的数据库记录。`blog_article_access` 保存持久授权，区分口令解锁和指定用户授权。

修改或清除查阅口令会撤销旧的口令授权，文章离开绝密等级后会撤销指定用户授权。备份必须同时保留文章快照和数据库，见[运维文档](operations.md)。

时间统一使用 UTC，API 返回 RFC 3339 格式。文章索引可以重建，账户、安装记录和授权需要从备份恢复。

## 升级

部署脚本会初始化所需对象并检查结构，但不会自动迁移所有旧表。升级前备份已有数据，结构差异以当前 SQL 文件为准。旧授权表若使用 INT 类型的 `id`，需要迁移为当前的字符串主键。
