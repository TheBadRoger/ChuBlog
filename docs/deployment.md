# 部署

[返回项目首页](../README.md)

首次部署见 [README 快速开始](../README.md#快速开始)。服务器需要 Docker、Compose、Python 3 和 Git，无需安装编译工具。ChuBlog 从源码构建，ChuSQL 安装失败时自动回退。

## 配置与镜像源

部署脚本自动生成 `.env` 和数据库口令，重复部署保留配置与数据。常用配置如下：

| 配置 | 用途 |
| --- | --- |
| `WEB_PORT` | 监听端口，默认 `1741` |
| `ALLOW_REGISTRATION` | 是否允许注册，默认 `true` |
| `CHUSQL_ROOT_PASSWORD` | 数据库管理员口令 |
| `CHUSQL_PASSWORD` | 博客数据库账号口令 |

默认使用腾讯云内网源，其他网络可使用：

```sh
scripts/deploy.sh --sources-env-file deploy/sources.public.env
```

部署、更新和卸载支持 `--env-file PATH`、`--project-name NAME`、`--sources-env-file PATH`。同一项目沿用相同参数。

## 首次安装网站

按 README 获取安装密钥，打开 `/setup` 创建管理员并填写网站信息。安装后入口关闭，管理员可以在账户页授权作者。

## 对接服务器反向代理

将整个站点转发到 `127.0.0.1:1741`，并配置域名与 HTTPS。修改 `WEB_PORT` 后使用对应端口。

代理保留原始 Host，并将 `X-Forwarded-Host`、`X-Forwarded-Proto` 覆盖为实际公网主机和协议。为 `/git/*` 允许至少 9 MiB 的请求体。

## 更新与卸载

先提交或保存工作区修改，再执行更新：

```sh
scripts/update.sh
```

卸载默认保留数据，添加 `--purge` 会永久删除数据卷。删除前先[备份](operations.md#备份与恢复)。

```sh
scripts/uninstall.sh
```

## 可选安装数据库 CLI

发行包自带 `csql`。使用 `--with-cli` 可确保源码回退时也安装客户端，后续更新继续携带该参数。

```sh
scripts/deploy.sh --with-cli
docker compose --env-file .env -f deploy/compose.yaml \
  exec db csql --config /etc/chusql/cli.toml \
  --user root --database chublog --history /tmp/chusql_history
```

按提示输入 `.env` 中的 `CHUSQL_ROOT_PASSWORD`。

## 检查服务

```sh
docker compose --env-file .env -f deploy/compose.yaml ps
docker compose --env-file .env -f deploy/compose.yaml logs --tail 100 api db
```

`api`、`web`、`db` 应运行，`init` 成功退出。故障处理见[运维文档](operations.md)。
