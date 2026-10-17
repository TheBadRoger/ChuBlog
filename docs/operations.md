# 运维与备份

[返回项目首页](../README.md)

## 持久化数据

| 卷 | 内容 |
| --- | --- |
| content | 作者 Git 缓存、collection.json、published.json、setup-token |
| ingress | 作者接收仓库 authors/用户名.git |
| database | 账户、权限、网站设置、安装记录和文章索引 |
| db-logs | 数据库日志 |

应用日志输出 stdout，由 Docker 轮换保存。安装和数据库初始化见[部署文档](deployment.md)，表结构见[数据库文档](database.md)。

## 备份与恢复

备份流程是停止写入，再保存 `.env` 和以上持久卷的一致副本。备份包含数据库口令及密码哈希，应加密并限制访问。恢复流程是停止服务、恢复同一批备份，再启动并核对账户、权限、文章和系统时间。

`collection.json` 保存生效版本、暂存状态和原始系统时间，需要完整保留。Git 可以恢复有效正文，原始时间及错误索引前的生效版本依赖快照备份。默认卸载保留数据卷，`--purge` 永久删除项目数据。

`blog_site.starter_commit` 记录内置文章首次释放状态。保留该字段可继续维护或撤回两篇文章；值为 NULL 时后台重试，遇到已有其他内容时保留仓库并报告冲突。站长账户和安装记录应一并恢复。

## 数据库故障与索引

数据库故障时，账户和 Git 认证返回失败，已导入公共文章继续通过快照阅读。服务恢复后自动重试同步；索引失败至少间隔 30 秒再试。进程重启会清空内存会话，用户重新登录即可。

`/api/admin/status` 中 indexed_commit 为 null 表示等待确认索引，值落后于 commit 表示同步中。在线同步可以自动重建派生索引；手动重建时先停止占用同一数据卷的 API，再运行 `chublog-api rebuild-index`。

重建范围为 blog_posts、blog_tags、blog_articles、blog_pages、blog_article_tags，账户和安装记录由数据库备份恢复。文件原子写入、fsync 和 Git 引用更新提供写入保护，磁盘故障及备份恢复仍需在实际环境演练。

## 数据库 CLI

安装与连接方法见[可选数据库 CLI](deployment.md#可选安装数据库-cli)。CLI 通过容器内回环地址连接，数据库端口使用内部网络。

## 安装错误定位

账户接口按原因区分连接、权限、凭据、表结构和数据校验错误。安装要求空账户表，已有账户与安装状态冲突时返回 409。部署阶段会校验 nickname 等必需字段，便于在服务启动前发现表结构差异。

查看安装请求附近的日志：

```sh
docker compose --env-file .env -f deploy/compose.yaml logs --tail 80 api
```

错误记录包含 reason 分类、operation 阶段和 database_detail 细分原因，例如 create_account 配合 id_not_generated 表示用户 ID 自增配置需要核对，column_type_mismatch 表示字段类型与写入值存在差异。日志以固定分类代替 SQL、密码和昵称原文。数据库结构以独立 SQL 文件为准，现有数据处理前先完成备份。

部署初始化会对空安装库执行账户和网站信息写入预检，使用固定检查数据并回滚整个事务。失败时部署输出 ChuSQL 原始错误，便于核对字段类型、自增和约束；已有账户或安装记录时跳过写入预检。
