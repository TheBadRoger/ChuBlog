# 运维与备份

[返回项目首页](../README.md)

## 持久化数据

| 卷 | 内容 |
| --- | --- |
| content | 文章快照、管理设置和安装密钥 |
| ingress | 作者 Git 仓库 |
| database | 账户、权限、网站设置和文章索引 |
| db-logs | 数据库日志 |

## 备份与恢复

停止服务后，备份 `.env` 和全部持久卷。备份包含口令和账户信息，应加密并限制访问。

恢复时停止服务，恢复同一批备份，再启动并检查账户、权限和文章。仅恢复 Git 仓库会丢失管理设置和授权，可能使受限文章重新公开。默认卸载保留数据，`--purge` 永久删除数据卷。

## 故障处理

先查看服务状态和日志：

```sh
docker compose --env-file .env -f deploy/compose.yaml ps
docker compose --env-file .env -f deploy/compose.yaml logs --tail 100 api db
```

数据库故障时，登录和发布会失败，已导入的公共文章仍可阅读。数据库恢复后自动重试同步。API 重启后，用户需要重新登录。

管理员可通过 `/api/admin/status` 查看同步状态。需要手动重建文章索引时，先停止使用同一数据卷的 API，再运行 `chublog-api rebuild-index`。索引重建不能恢复账户和授权，应从备份恢复。

安装冲突或表结构错误时，先检查日志并备份数据。不要直接清空已有数据库。进程或内存不足时，可运行 `docker stats --no-stream` 检查资源。

## 数据库 CLI

连接方法见[可选数据库 CLI](deployment.md#可选安装数据库-cli)。
