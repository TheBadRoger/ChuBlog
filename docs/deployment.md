# 部署

[返回项目首页](../README.md)

## 一键部署

在项目根目录执行，将示例域名替换为自己的域名，并事先配置 DNS：

```sh
scripts/deploy.sh --site https://blog.example.com
```

脚本依次生成配置、获取固定版本的外部依赖、在 Docker 内构建镜像、启动并初始化数据库，最后启动博客并等待服务就绪。不需要另行执行 `hash-password`、`db-init` 或 `docker compose up`。

首次运行自动创建权限为 `600` 的 `.env`，生成两份不同的随机数据库口令。重复部署保留已有口令和数据；需要备份 `.env` 及持久卷。不传 `--site` 时首次默认使用 `http://localhost`，已有配置则继续沿用。

部署不创建博客管理员，也不要求填写管理员密码。首页继续显示施工说明和导航；文章阅读、注册登录、账户设置及 SSH 服务照常启动。新注册用户均为 `reader`，第一个用户也不会自动提权。已有账户与发布授权保持有效；空站点的发布授权仍需后续确定的管理员管理流程，注册本身不授予发布权。

## 更新与卸载

更新前先提交或自行保存工作区修改，然后执行：

```sh
scripts/update.sh
```

脚本拒绝覆盖有未提交修改的工作区，以 `git pull --ff-only` 更新当前分支，再重新获取依赖、构建、初始化数据库并启动服务。保留 `.env`、账户、文章和数据库持久卷。构建失败会退出，不先停止已有服务；启动失败时检查日志，脚本不提供自动版本回滚。

卸载当前项目的容器、网络和本地构建镜像，保留数据卷：

```sh
scripts/uninstall.sh
```

**永久删除文章、账户、数据库、授权和 SSH 主机密钥等项目持久数据**时，使用：

```sh
scripts/uninstall.sh --purge
```

卸载保留项目源码、`.env`、共享基础镜像和 Docker 构建缓存，不执行全局 prune。保留数据卷时，重新运行部署脚本即可恢复。删除数据卷后恢复只能依靠备份。

三个脚本均支持 `--env-file PATH`、`--project-name NAME` 和 `--sources-env-file PATH`；同一套部署的更新与卸载必须沿用相同配置路径和项目名。下载源覆盖文件只接受镜像及软件包来源，不能覆盖数据库口令。

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

项目提供不含密码的 `deploy/sources.public.env`，使用公开 ECR Docker Library 镜像、USTC Debian 源、官方 npm 源及 RsProxy Cargo 源。外网部署与隔离联调时运行：

```sh
scripts/deploy.sh --site https://blog.example.com \
  --sources-env-file deploy/sources.public.env
python3 scripts/smoke.py --key-type rsa \
  --sources-env-file deploy/sources.public.env
```

第二份配置只覆盖下载来源，不改写 `.env` 的域名、口令等生产设置。Docker Compose 会优先使用当前 shell 中已导出的同名环境变量；切换测试配置前须检查这些覆盖。公网源连通性不能证明腾讯云内网镜像可用，内网源仍需在腾讯云服务器验证。

## 构建选项与平台

服务器部署固定只构建生产目标，无需额外参数：Rust 只构建 `chublog-api` 可执行文件与 ChuSQL 存储库，Haskell 只构建服务端和 bootstrap，并显式禁用测试与基准目标。前端只执行类型检查与生产构建，不调用测试命令。`deploy.sh` 与 `prepare-deploy.py` 不会自动运行联调脚本；开发测试仍可独立执行。

需要切换默认腾讯云内网 Debian 包源时，可在 `.env` 配置 `DEBIAN_MIRROR=mirrors.ustc.edu.cn`，仍使用 Debian 签名校验。

- `prepare-deploy.py` 调用 Compose 构建全部应用镜像，不执行宿主机编译、不生成 `deploy/bin/`。
- `--build-database` 保留为兼容参数；数据库始终在 Docker 中构建。旧的 `--no-build` 和 `--chusql-source` 不再支持。
- `--sources-env-file PATH` 可追加只含下载源配置的 Compose 环境文件；优先级高于第一份环境文件。
- `--env-file PATH` 可选择配置文件；默认读取项目 `.env`，不存在时读取 `.env.example`（仅用于构建，不用于生产启动）。
- 外部依赖仍由宿主机 `fetch-deps.py` 初始化，沿用现有 Git 配置；构建过程不将 SSH 私钥或 Git 凭据复制到镜像。
- GHC 工具链镜像支持 amd64、arm64；更新 ChuSQL resolver 时，须同步核对 GHC 镜像版本与 Dockerfile 中的复制路径。
- 镜像默认按 Docker 主机架构构建，不复用宿主机已有二进制；构建与运行阶段均使用 Debian 13 系列镜像。
- Cargo、Stack 软件包缓存保存在 Docker BuildKit 中。镜像前缀控制编译器镜像来源，Cargo 与 Haskell 包下载独立于镜像注册表。
- 运行镜像不含编译器、源码、`.env` 或密码文件。

## 验证部署

浏览器打开配置的站点，确认首页居中显示施工说明，且可以进入文章列表与账户页面。`SITE_TITLE` 控制页眉名称。检查服务状态与日志：

```sh
docker compose --env-file .env -f deploy/compose.yaml ps
docker compose --env-file .env -f deploy/compose.yaml logs --tail 100 api db
```

`api`、`web`、`ssh`、`db` 和 `caddy` 应运行，`init` 成功退出。`db-init` 是一次性初始化任务。部署脚本对数据库初始化设有重试和超时，失败时退出，不会报告部署完成。

用于开发环境的隔离部署回归检查：

```sh
python3 scripts/smoke-deployment.py --sources-env-file deploy/sources.public.env
```

该检查使用独立项目与本机 18080/18443/12222 端口，验证无管理员启动、普通用户注册登录、保留数据卸载后重新部署及彻底卸载。测试结束删除自己的容器和卷，不操作正式项目。

## 域名与网络

- Caddy 根据 `SITE_ADDRESS` 提供 HTTPS 和 API 反向代理；`PUBLIC_ORIGIN` 必须与外部站点完全一致，不带结尾斜杠。
- HTTP 仅对 localhost 开发场景允许。
- 只需公网 TCP 80/443 和 Git SSH 2222（可改）；主机管理 SSH 另行管理，数据库和 API 不映射宿主端口。
- 既有 UDP 7001 不受影响。
- 参考 [Caddy 请求体限制](https://caddyserver.com/docs/caddyfile/directives/request_body) 与 [反向代理文档](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy)。
