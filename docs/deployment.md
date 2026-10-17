# 部署

[返回项目首页](../README.md)

## 构建与启动

- 构建与运行镜像基于 Debian 13；宿主机只需满足 Docker 运行要求。
- `scripts/deploy.sh` 会使用同一个依赖初始化脚本，自动按[依赖源顺序](development.md)进行 HTTPS 回退，再构建外部数据库与 Docker 镜像；沿用运行者现有的 Git 配置，不在项目内覆盖服务器环境。

```sh
scripts/deploy.sh
```

## 宿主机要求

宿主机只需 Docker Engine（含 BuildKit）、Docker Compose 插件、Python 3 和 Git，以及执行 Docker 命令的权限。不需要安装 Cargo、Rust、GHC、Stack、Node.js 或 C 编译工具。

Rust API、ChuSQL 存储库和 Haskell 服务端在 Docker 多阶段构建中编译，Astro 前端也在 Docker 中构建。构建工具和源码只保留在构建阶段，运行镜像仅包含应用产物及运行依赖。首次构建会拉取预装工具链的镜像并下载软件包，需要网络、足够的内存及磁盘空间；后续构建复用 Docker 缓存。

## 服务与路由

Compose 编排 Astro 前端 `web`、Rust API `api`、文章 SSH 接收 `ssh`、ChuSQL 数据库 `db` 和 Caddy。`init` 初始化持久卷，`db-init` 用于首次数据库授权。

`web` 是博客前端，不是 ChuSQL Web。数据库只构建 `chusql-server`、`csql-bootstrap` 与存储库，不构建或启动 `chusql-web`、`csql-web` 或数据库管理网站。

Caddy 将 `/api/*` 转发到 `api:3001`，其余请求转发到 `web:4321`。Node、API 和数据库只加入内部网络，不映射宿主端口；公网入口为 Caddy 的 HTTP/HTTPS 和文章 SSH。前端容器使用非 root 用户、只读根文件系统和独立资源上限。

## 腾讯云镜像

`.env.example` 默认配置 `REGISTRY_PREFIX=mirror.ccs.tencentyun.com/library/`，为 Rust、Debian、Node 与 Caddy 使用腾讯云内网 Docker 镜像。该地址仅供腾讯云内网访问，见[腾讯云官方说明](https://cloud.tencent.com/document/practice/1207/45596)。

在腾讯云内网部署时保留该值；其他环境将 `REGISTRY_PREFIX` 留空以使用原始镜像名，或改为可访问且受信的镜像前缀。项目不修改宿主机 Docker 配置、不关闭 TLS，也不重启 Docker 守护进程。

镜像注册表与软件包源分别配置：

| 配置 | 默认值与用途 |
| --- | --- |
| `REGISTRY_PREFIX` | `mirror.ccs.tencentyun.com/library/`，用于 Rust、GHC/Haskell、Debian、Node 与 Caddy 镜像 |
| `DEBIAN_MIRROR` | `mirrors.tencentyun.com`，容器内 Debian 软件包源，保留 Debian 签名校验 |
| `NPM_REGISTRY` | `https://mirrors.tencentyun.com/npm/`，前端依赖源 |
| `CARGO_MIRROR_INDEX` | 默认 `sparse+https://rsproxy.cn/index/`；显式留空可使用 crates.io |

腾讯云的[公开软件源目录](https://mirrors.tencent.com/)没有列出 Cargo 与 Hackage 镜像，因此不拼接未经确认的腾讯云地址。Cargo 使用 [RsProxy](https://rsproxy.cn/) 等可配置外部源；Haskell 包沿用 Stack 默认配置。腾讯云的[软件源说明](https://cloud.tencent.com/document/product/213/8623)区分了公网和内网域名。

Rust 工具链直接来自 Rust 镜像；GHC 9.10.3 和 Stack 来自匹配 ChuSQL resolver 的 Haskell 镜像。构建时禁止 Stack 再下载 GHC，也不调用 rustup 安装或更新编译器。编译仍在 Debian 13 中进行。

### 使用外网源验证

项目提供不含密码的 `deploy/sources.public.env`，使用公开 ECR Docker Library 镜像、USTC Debian 源、官方 npm 源及 RsProxy Cargo 源。外网构建与隔离联调时运行：

```sh
python3 scripts/prepare-deploy.py --sources-env-file deploy/sources.public.env
python3 scripts/smoke.py --key-type rsa \
  --sources-env-file deploy/sources.public.env
```

第二份配置只覆盖下载来源，不改写 `.env` 的域名、口令等生产设置。Docker Compose 会优先使用当前 shell 中已导出的同名环境变量；切换测试配置前须检查这些覆盖。公网源连通性不能证明腾讯云内网镜像可用，内网源仍需在腾讯云服务器验证。

## 首次部署

在项目根目录执行。先按[开发与依赖](development.md)初始化子模块，再准备配置和构建镜像：

```sh
cp .env.example .env
chmod 600 .env
# 编辑 .env，配置域名、镜像前缀及两份不同的随机 DB 口令。
python3 scripts/prepare-deploy.py
# 在临时容器内生成管理员密码哈希；不启动其他服务。
# 输入口令后按 Ctrl-D 结束，不要把明文写进命令行历史。
docker compose --env-file .env -f deploy/compose.yaml \
  run --rm --no-deps -T api hash-password
# 将输出以单引号包裹写入 .env 的 ADMIN_PASSWORD_HASH。
docker compose --env-file .env -f deploy/compose.yaml up -d --no-build
# db 启动就绪后，创建 chublog 库和非超级用户 chublog_index；失败时检查日志后重试。
docker compose --env-file .env -f deploy/compose.yaml run --rm db-init
```

后续部署可运行 `scripts/deploy.sh`，自动初始化依赖、在 Docker 内构建并启动服务。

部署入口启动服务后，首次仍需执行上面的 `db-init` 命令；入口不会在数据库启动竞态中静默吞掉初始化错误。

## 构建选项与平台

需要切换默认腾讯云内网 Debian 包源时，可在 `.env` 配置 `DEBIAN_MIRROR=mirrors.ustc.edu.cn`，仍使用 Debian 签名校验。

- `prepare-deploy.py` 调用 Compose 构建全部应用镜像，不执行宿主机编译、不生成 `deploy/bin/`。
- `--build-database` 保留为兼容参数；数据库始终在 Docker 中构建。旧的 `--no-build` 和 `--chusql-source` 不再支持。
- `--sources-env-file PATH` 可追加只含下载源配置的 Compose 环境文件；优先级高于第一份环境文件。
- `--env-file PATH` 可选择配置文件；默认读取项目 `.env`，不存在时读取 `.env.example`（仅用于构建，不用于生产启动）。
- 外部依赖仍由宿主机 `init-deps.py` 初始化，沿用现有 Git 配置；构建过程不将 SSH 私钥或 Git 凭据复制到镜像。
- GHC 工具链镜像支持 amd64、arm64；更新 ChuSQL resolver 时，须同步核对 GHC 镜像版本与 Dockerfile 中的复制路径。
- 镜像默认按 Docker 主机架构构建，不复用宿主机已有二进制；构建与运行阶段均使用 Debian 13 系列镜像。
- Cargo、Stack 软件包缓存保存在 Docker BuildKit 中。镜像前缀控制编译器镜像来源，Cargo 与 Haskell 包下载独立于镜像注册表。
- 运行镜像不含编译器、源码、`.env` 或密码文件。

## 管理员初始化

浏览器打开配置的站点即可看到前端。`SITE_TITLE` 控制页眉名称。首次启动从环境中的哈希创建管理员，后续账号与权限以持久文件为准，修改初始哈希不会覆盖已有账户。

## 域名与网络

- Caddy 根据 `SITE_ADDRESS` 提供 HTTPS 和 API 反向代理；`PUBLIC_ORIGIN` 必须与外部站点完全一致，不带结尾斜杠。
- HTTP 仅对 localhost 开发场景允许。
- 只需公网 TCP 80/443 和 Git SSH 2222（可改）；主机管理 SSH 另行管理，数据库和 API 不映射宿主端口。
- 既有 UDP 7001 不受影响。
- 参考 [Caddy 请求体限制](https://caddyserver.com/docs/caddyfile/directives/request_body) 与 [反向代理文档](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy)。
