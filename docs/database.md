# 数据库

[返回项目首页](../README.md)

类型选型依据 ChuSQL 的[类型文档](../external/ChuSQL/docs/types.md)和 [SQL 语法](../external/ChuSQL/docs/syntax.md)，本机上游文档位于 `../ChuSQL/docs/`。账户、网站配置和权限需要持久保存，文章索引可以从 Git 与快照重建。

## 字段类型

| 数据 | 类型与约束 |
| --- | --- |
| 用户 ID、管理员 ID、安装 ID | INT，用户 ID 自增 |
| 昵称 | VARCHAR(128)，可重复的 UTF-8 展示属性 |
| 账号、密码 PHC | VARCHAR(32)、VARCHAR(255)，账号由 3–32 位小写字母、数字、下划线或减号，首位为小写字母、下划线或减号组成且唯一，PHC 保存完整参数、盐和哈希 |
| 网站名称、简介 | VARCHAR(120)、VARCHAR(1000) |
| 账户与安装时间 | TIMESTAMP，上次登录可为 NULL |
| 权限 | ENUM blog_permission：BANNED、MEMBER、AUTHOR、MASTER |
| 文章 ID、完整 slug | VARCHAR(40)、VARCHAR(113)（`作者名~文章ID`） |
| 标题、简介、标签 | VARCHAR(240)、VARCHAR(1000)、VARCHAR(80) |
| 路径、文件名 | VARCHAR(192)、VARCHAR(100) |
| Git 提交、首次文章释放提交 | CHAR(40)，首次释放前为 NULL |
| 文章状态、格式 | ENUM blog_article_status、blog_document_format |
| 文章时间、封面 | TIMESTAMP、VARCHAR(100)，首次公开前或缺少封面时为 NULL |
| 可见性、隐藏、审核 | ENUM blog_visibility（DEFAULT、SECRET、CONFIDENTIAL、TOP）、blog_hidden（SEMI、FULL）、blog_review（OPEN、BLOCKED） |
| 访问来源 | ENUM blog_access_source（PASSWORD、GRANTED） |

VARCHAR 和 CHAR 校验字符长度，应用另行校验 UTF-8 字节数。CHAR 保留原字符串长度。索引键另受数据库字节上限约束，最大 slug 为 113 个 ASCII 字节。默认存储页为 4096 字节，整行也需满足页容量；正文和图片保存在 Git 与快照中。

## 时间格式

ChuSQL TIMESTAMP 使用秒级 `YYYY-MM-DD HH:MM:SS`，也接受 T 分隔形式。应用写入时统一转换为 UTC 秒级，账户 API 返回 `YYYY-MM-DDTHH:MM:SSZ`。文章快照可保留毫秒，派生索引按秒保存。首次公开前的数据库时间使用 SQL NULL。

## 初始化与管理员

SQL 编译进 API 镜像，修改后重新构建部署：

- [create-database.sql](../apps/api/sql/create-database.sql)：数据库。
- [account-indexes.sql](../apps/api/sql/account-indexes.sql)：用户名列的查询索引，保留 username 唯一约束。
- [accounts.sql](../apps/api/sql/accounts.sql)：账户、安装记录、权限/可见性/隐藏/审核/访问来源 ENUM，文章策略投影表和持久访问授权表。
- [complete-empty-install.sql](../apps/api/sql/complete-empty-install.sql)：账户与安装记录都为空时，清空账户、安装、策略投影和访问授权表，再由 accounts.sql 创建当前结构。
- [articles.sql](../apps/api/sql/articles.sql)：文章索引、状态和格式 ENUM。

安装事务向 blog_users 插入 MASTER，再用真实用户 ID 写入 blog_site.master_user_id。提交后，系统读取管理员账号，为其 Git 仓库释放两篇文章，提交记录保存在 blog_site.starter_commit。索引重建保留账户和安装数据。

初始化按身份分工：数据库 root 创建 ENUM 并清理空安装表，博客账号 chublog_index 创建表并执行日常读写。普通账号保留 CONNECT、CREATE 和自身表权限，root 凭据仅供初始化任务使用。

重复初始化复用已有对象；账户与安装记录都为空时，暂停 API 写入后按当前 accounts.sql 重建这两张空表，使权限列使用 blog_permission ENUM；已有账户或安装记录时保留表与数据。部署会检查账户与安装表的必需字段，包括 nickname。首次安装以空账户表和本版 SQL 结构为前提，开发中的类型变更使用新的测试数据库验证。备份要求见[运维文档](operations.md)。

## 文章策略与访问授权

`collection.json` 是文章策略的权威来源，`blog_article_policy` 是可由数据库查询的投影。应用在启动、后台导入变更和每次管理保存后全量重写投影；写入失败时保存请求失败，启动时失败仅记录 `policy_projection_pending`，待下次同步。投影表结构：

```sql
CREATE TABLE blog_article_policy (
    article VARCHAR(113) PRIMARY KEY,
    visibility blog_visibility NOT NULL DEFAULT 'DEFAULT',
    hidden blog_hidden NOT NULL DEFAULT 'FULL',
    review blog_review NOT NULL DEFAULT 'OPEN',
    blocked_hash CHAR(64),
    password_hash VARCHAR(255)
);
```

`blog_article_access` 保存用户与受限文章的多对多授权关系，使用普通关系行而非 List 列：

```sql
CREATE TABLE blog_article_access (
    id VARCHAR(134) PRIMARY KEY,
    user_id INT NOT NULL,
    article VARCHAR(113) NOT NULL,
    source blog_access_source NOT NULL,
    created_at TIMESTAMP NOT NULL
);
```

选择关系表而不是 `[VARCHAR(113)]` List 的原因：List 整行受 4096 字节页限制、没有唯一约束或索引、也没有追加操作，更新必须读改写；关系表以 `用户ID~文章ID` 作为单列主键，保证每位用户对每篇文章最多一条授权，按文章定向查询，并原子增删单条记录。授权来源区分口令解锁（PASSWORD）和作者/管理员指定（GRANTED）；前者只对机密文章生效，后者只对绝密文章生效。修改或清除口令撤销 PASSWORD 行，文章离开绝密时撤销 GRANTED 行。

ChuSQL 支持列级 UNIQUE，但当前不支持 `UNIQUE (user_id, article)` 形式的表级联合约束，因此授权表使用组合字符串主键。若测试或旧部署已手工创建 INT 类型的授权表 id，升级前应备份并迁移为上述结构；初始化不会自动改写已有表或授权记录。

已有部署升级时，需要以 root 执行 `provision-db` 流程创建新 ENUM 和表；旧 `collection.json` 中的 `public/internal/hidden` 与 `deleted` 会在反序列化时迁移为 `default/secret/top` 与 `review=blocked`；旧的“内部文章 + 口令”迁移为 `confidential` + `full`。
