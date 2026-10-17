# 部署

[返回项目首页](../README.md)

首次部署的完整步骤见 [README 快速开始](../README.md#快速开始)。服务器需要 Docker Engine、Compose 插件、Python 3 和 Git；Rust、Haskell 和前端生产构建均在 Docker 内完成。

## 配置与镜像源

部署脚本生成权限为 `600` 的 `.env`，保存两份独立的随机数据库口令。重复部署保留已有配置与数据。构建仅包含生产目标，测试由开发流程单独执行。数据库初始化先由 root 创建 ENUM，再由 chublog_index 建表；日常服务使用博客数据库账号。

| 配置 | 默认值与用途 |
| --- | --- |
| `WEB_PORT` | `1741`，同时控制容器监听、宿主机回环映射和健康检查 |
| `ALLOW_REGISTRATION` | `true`，普通账户注册开关 |
| `CHUSQL_ROOT_PASSWORD` | 自动生成的数据库 root 口令 |
| `CHUSQL_PASSWORD` | 自动生成的博客数据库账号口令 |
| `REGISTRY_PREFIX` | `mirror.ccs.tencentyun.com/library/`，腾讯云内网镜像前缀 |
| `DEBIAN_MIRROR` | `mirrors.tencentyun.com` |
| `NPM_REGISTRY` | `https://mirrors.tencentyun.com/npm/` |
| `CARGO_MIRROR_INDEX` | `sparse+https://rsproxy.cn/index/`，空值选择 crates.io |

公网环境可使用 `--sources-env-file deploy/sources.public.env`。该文件覆盖下载来源，口令继续由 `.env` 管理；shell 导出的同名变量具有更高优先级。Haskell 包使用 Stack 默认来源，GHC 来自固定版本工具链镜像。

部署、更新和卸载均接受 `--env-file PATH`、`--project-name NAME`、`--sources-env-file PATH`。同一项目沿用相同参数。

## 首次安装网站

访问 `/setup`，填写网站名称、简介、管理员账号、昵称和密码。安装密钥通过服务器获取：

```sh
docker compose --env-file .env -f deploy/compose.yaml \
  exec -T api sh -c 'cat /data/setup-token && printf "\n"'
```

密钥以权限 `600` 保存在私有卷中。安装事务将唯一 `MASTER` 账户和网站配置写入 ChuSQL，成功后入口关闭。普通注册创建 `MEMBER`，管理员可在账户页授予作者权限。

安装后，发文教程和开发进度自动写入管理员仓库并随后台同步公开。管理员通过 Git 维护这两篇文章；首次释放记录用于重试和保留后续修改。账户、安装记录与 SQL 类型见[数据库文档](database.md)。

## 对接服务器反向代理

`web` 提供网站、`/api/*` 和 `/git/*`；`api` 与 `db` 使用内部网络。服务器自己的反向代理将整个站点转发到 `127.0.0.1:1741`，并负责 DNS、HTTPS 证书和公网入口。

代理保留原始 Host，并覆盖 `X-Forwarded-Host`、`X-Forwarded-Proto` 为实际公网主机和协议。API 按这些值校验 Origin。回环端口供可信本机进程使用；容器中的代理需要配置到宿主机的可达地址。

Git 请求体额度为 9 MiB，代理应为 `/git/*` 保留相应容量。博客域名和证书由服务器代理配置管理。

## 更新与卸载

保存或提交工作区修改后，执行更新：

```sh
scripts/update.sh
```

脚本以 `git pull --ff-only` 更新源码并重新部署，保留配置与持久卷。有本地修改时，脚本退出并提示处理。默认卸载也保留数据：

```sh
scripts/uninstall.sh
```

`--purge` 会永久删除本项目的数据卷，使用前完成[备份](operations.md#备份与恢复)：

```sh
scripts/uninstall.sh --purge
```

卸载处理本项目容器、网络和本地构建镜像，源码、`.env`、共享基础镜像及构建缓存继续保留。

## 可选安装数据库 CLI

部署和后续更新携带 `--with-cli` 即可安装 `csql`：

```sh
scripts/deploy.sh --with-cli
scripts/update.sh --with-cli
```

该选项按本次命令生效。进入数据库 CLI：

```sh
docker compose --env-file .env -f deploy/compose.yaml \
  exec db csql --config /etc/chusql/cli.toml \
  --user root --database chublog --history /tmp/chusql_history
```

按提示输入 `.env` 中的 `CHUSQL_ROOT_PASSWORD`。客户端连接容器回环地址，历史保存在临时目录。单独构建镜像可运行 `python3 scripts/prepare-deploy.py --with-cli`。

## 检查服务

安装后可检查首页导航、两篇内置文章和账户登录，并查看服务状态：

```sh
docker compose --env-file .env -f deploy/compose.yaml ps
docker compose --env-file .env -f deploy/compose.yaml logs --tail 100 api db
```

`api`、`web`、`db` 应运行，`init` 成功退出；`db-init` 为一次性数据库初始化任务。故障处理见[运维文档](operations.md)，部署环境联调见[开发文档](development.md#部署环境联调)。
