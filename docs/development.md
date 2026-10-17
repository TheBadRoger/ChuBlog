# 开发与依赖

[返回项目首页](../README.md)

```text
ChuBlog/
├── apps/api/
├── external/
│   ├── ChuSQL/             # Git 子模块，主仓库只记录版本指针
│   └── ChuSQL-RustDriver/  # Git 子模块
└── scripts/fetch-deps.py
```

## 获取代码

项目提供两个 HTTPS 仓库，任选其一克隆：

```sh
git clone https://github.com/TheBadRoger/ChuBlog.git
# 或：git clone https://repo.chunana.cn/ChuNa7a/ChuBlog.git
cd ChuBlog
python3 scripts/fetch-deps.py
```

## 依赖来源与回退

数据库和驱动均提供两个 HTTPS 源：

| 依赖 | GitHub | 自建仓库 |
| --- | --- | --- |
| ChuSQL | https://github.com/TheBadRoger/ChuSQL.git | https://repo.chunana.cn/ChuNa7a/ChuSQL.git |
| ChuSQL-RustDriver | https://github.com/TheBadRoger/ChuSQL-RustDriver.git | https://repo.chunana.cn/ChuNa7a/ChuSQL-RustDriver.git |

- 初始化先尝试 GitHub HTTPS，连接、访问或固定提交获取失败时回退自建 HTTPS，无需 SSH 身份。
- 已正确初始化的固定版本直接复用，有未提交修改时拒绝覆盖；回退不改为任意最新分支。
- 项目不设置全局 `insteadOf`、localhost 映射、OS 用户或服务器 SSH 配置。

## 使用本地依赖

开发电脑仍可显式选择已有本地来源（仅开发覆盖参数，不是服务器默认流程）；只读取已提交版本：

```sh
python3 scripts/fetch-deps.py --database-repo ../ChuSQL --driver-repo ../ChuSQLDriver
```

- 子模块固定到已提交版本；源仓库必须包含这些提交。
- 依赖源码和构建产物不进入博客仓库，更新依赖时只提交 `.gitmodules`/配置及子模块指针。
- 不要对主仓库使用 `git add --force` 重新收录依赖文件。

## 开发检查

本地验证不运行 Docker 测试或测试镜像构建。日常只执行源码检查、单元测试和使用命令替身的部署脚本测试；需要真实容器的联调由部署环境按需手动执行。

```sh
cargo test --all-targets
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

前端开发和构建见 [Astro 前端](frontend.md)。

以下命令仅供部署环境手动验证，不在本地自动运行。账户集成测试需要支持 ENUM 的真实 ChuSQL 镜像，在临时容器和数据库中验证，不读取 JSON 夹具：

```sh
python3 scripts/test-accounts-db.py --database-image chublog-db:latest
```

测试涵盖 ENUM 约束、注册登录、权限变更及账户持久化；无数据库时默认的 Cargo 测试跳过这些用例。更新 ChuSQL 依赖只提交子模块指针，不将外部源码纳入主仓库。

## 部署环境联调

部署镜像在 Docker 内编译，不需要宿主机 Rust 或 Haskell 工具链；运行上面的源码开发检查时仍须自行安装 Rust。准备脚本的轻量回归检查：

```sh
python3 scripts/test-prepare-deploy.py
python3 scripts/test-fetch-deps.py
python3 scripts/test-deployment.py
```

```sh
python3 scripts/prepare-deploy.py --sources-env-file deploy/sources.public.env
python3 scripts/smoke.py --sources-env-file deploy/sources.public.env
python3 scripts/smoke.py --https --sources-env-file deploy/sources.public.env
# 安装 Python Playwright 和 Chromium 后，可追加前端真实浏览器验证。
python3 scripts/smoke.py --browser-test \
  --sources-env-file deploy/sources.public.env
```

- 联调使用独立随机命名的 Compose 项目、本机 1741 端口，测试结束清理自己创建的容器与卷。
- 覆盖真实 HTTP Basic 身份与发布授权、作者间 Git 对象隔离、非法推送、TOML 暂存与修复、多页链接、系统时间、索引删除、ChuSQL 元信息创建/删除、发布撤回、断库阅读、重启与撤权。
- `--https` 使用临时 Caddy、私有测试 CA 和本机 18443 端口，Git/HTTP 客户端显式信任该测试 CA，保持证书校验开启；需要本机可用的 Caddy 镜像。该容器不进入生产编排，测试后删除。
- 隔离联调通过首次安装接口创建真实入库的测试站长并初始化两篇文章；生产站点同样在 `/setup` 完成初始化，不配置管理员环境变量。
- 该测试会拉取基础镜像，需要 Docker 可用。

## 历史清理后的协作

- 驱动的旧源码副本已从主仓库可达历史移除，后续只保留外部子模块配置。
- 历史重写后，已有克隆建议重新克隆并初始化依赖；不要把旧 main 合并回来，否则可能重新引入已删除的依赖历史。
- 自建服务器的不可达对象清理与其他人的旧克隆需要分别处理，GitHub 缓存和旧提交直链的回收由平台管理。

## 内置文章与类型文档

内置文章源文件位于 `apps/api/starter/`，以 `index.toml` 和 Markdown 保存。修改模板会影响之后的新安装，不会覆盖已安装站长仓库；已有站点由站长通过 Git 更新文章。`apps/api/src/starter.rs` 负责首次写入与失败恢复，普通文章校验器负责验收和导入。

表结构选型依据固定依赖的 `external/ChuSQL/docs/types.md`、`docs/syntax.md`；本机对应上游文档为 `../ChuSQL/docs/`。字段与时间格式汇总见[数据库结构](database.md)。原教程 Astro 页面已删除并忽略，开发进度也不再硬编码在首页。
