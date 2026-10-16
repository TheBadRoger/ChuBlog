# 部署

[返回项目首页](../README.md)

## 构建与启动

- 部署基于 Debian 13。
- `scripts/deploy.sh` 会使用同一个依赖初始化脚本，自动按[依赖源顺序](development.md)进行 HTTPS 回退，再构建外部数据库与 Docker 镜像；沿用运行者现有的 Git 配置，不在项目内覆盖服务器环境。

```sh
scripts/deploy.sh
```

## 首次部署

先按[开发与依赖](development.md)初始化子模块，再执行以下同架构构建步骤。使用 `scripts/deploy.sh` 时，也须先准备 `.env`。

```sh
# ChuSQL 尚未编译时，先按其项目说明构建 Rust 存储库、server 与 bootstrap。
python3 scripts/prepare-deploy.py --build-database
cp .env.example .env
chmod 600 .env
# 从标准输入输入管理员口令，Ctrl-D 结束；不要把明文写进命令行历史。
./deploy/bin/chublog-api hash-password
# 将输出以单引号包裹写入 .env 的 ADMIN_PASSWORD_HASH；同时配置域名及两份不同的随机 DB 口令。
docker compose --env-file .env -f deploy/compose.yaml up -d --build
# db 启动就绪后，创建 chublog 库和非超级用户 chublog_index；失败时检查日志后重试。
docker compose --env-file .env -f deploy/compose.yaml run --rm db-init
```

部署入口启动服务后，首次仍需执行上面的 `db-init` 命令；入口不会在数据库启动竞态中静默吞掉初始化错误。

## 构建选项与平台

构建时若 Debian 官方下载源较慢，可在 `.env` 配置 `DEBIAN_MIRROR=mirrors.ustc.edu.cn`，仍使用 Debian 签名校验。

- `prepare-deploy.py` 默认从数据库子模块读取编译产物；`--build-database` 会先构建其 Rust 存储库、Haskell server 和 bootstrap。
- 也可用 `--chusql-source /已有构建的/ChuSQL` 选择已有外部构建，没有产物会明确失败。
- 编译产物只暂存在被忽略的 `deploy/bin/`。
- 跨平台部署须先在目标 Debian/架构构建，不能复制 macOS、Windows 或其他架构二进制。
- 镜像不含编译器、源码、`.env` 或密码文件。

## 管理员初始化

首次启动从环境中的哈希创建管理员，后续账号与权限以持久文件为准，修改初始哈希不会覆盖已有账户。

## 域名与网络

- Caddy 根据 `SITE_ADDRESS` 提供 HTTPS 和 API 反向代理；`PUBLIC_ORIGIN` 必须与外部站点完全一致，不带结尾斜杠。
- HTTP 仅对 localhost 开发场景允许。
- 只需公网 TCP 80/443 和 Git SSH 2222（可改）；主机管理 SSH 另行管理，数据库和 API 不映射宿主端口。
- 既有 UDP 7001 不受影响。
- 参考 [Caddy 请求体限制](https://caddyserver.com/docs/caddyfile/directives/request_body) 与 [反向代理文档](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy)。
