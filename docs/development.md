# 开发与依赖

[返回项目首页](../README.md)

```text
ChuBlog/
├── apps/api/
├── external/
│   ├── ChuSQL/             # Git 子模块，主仓库只记录版本指针
│   └── ChuSQL-RustDriver/  # Git 子模块
└── scripts/init-deps.py
```

## 获取代码

项目提供两个 HTTPS 仓库，任选其一克隆：

```sh
git clone https://github.com/TheBadRoger/ChuBlog.git
# 或：git clone https://repo.chunana.cn/ChuNa7a/ChuBlog.git
cd ChuBlog
python3 scripts/init-deps.py
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
python3 scripts/init-deps.py --database-repo ../ChuSQL --driver-repo ../ChuSQLDriver
```

- 子模块固定到已提交版本；源仓库必须包含这些提交。
- 依赖源码和构建产物不进入博客仓库，更新依赖时只提交 `.gitmodules`/配置及子模块指针。
- 不要对主仓库使用 `git add --force` 重新收录依赖文件。

## 开发检查

```sh
cargo test --all-targets
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

前端开发和构建见 [Astro 前端](frontend.md)。

## 联调

部署镜像在 Docker 内编译，不需要宿主机 Rust 或 Haskell 工具链；运行上面的源码开发检查时仍须自行安装 Rust。准备脚本的轻量回归检查：

```sh
python3 scripts/test-prepare-deploy.py
```

```sh
python3 scripts/prepare-deploy.py --sources-env-file deploy/sources.public.env
python3 scripts/smoke.py --sources-env-file deploy/sources.public.env
python3 scripts/smoke.py --key-type rsa \
  --sources-env-file deploy/sources.public.env
# 安装 Python Playwright 和 Chromium 后，可追加前端真实浏览器验证。
python3 scripts/smoke.py --key-type rsa --browser-test \
  --sources-env-file deploy/sources.public.env
```

- 联调使用独立随机命名的 Compose 项目、本机 18080/18443/12222 端口，测试结束清理自己创建的容器与卷。
- 覆盖真实 SSH 身份与授权、作者间 Git 对象隔离、非法推送、TOML 暂存与修复、多页链接、系统时间、索引删除、ChuSQL 元信息创建/删除、发布撤回、断库阅读、重启与撤权。
- 该测试会拉取基础镜像，需要 Docker 可用。

## 历史清理后的协作

- 驱动的旧源码副本已从主仓库可达历史移除，后续只保留外部子模块配置。
- 历史重写后，已有克隆建议重新克隆并初始化依赖；不要把旧 main 合并回来，否则可能重新引入已删除的依赖历史。
- 自建服务器的不可达对象清理与其他人的旧克隆需要分别处理，GitHub 缓存和旧提交直链的回收由平台管理。
