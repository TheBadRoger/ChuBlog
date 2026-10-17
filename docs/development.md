# 开发

[返回项目首页](../README.md)

## 代码与依赖

克隆步骤见 [README](../README.md#快速开始)。开发时获取固定版本依赖：

```sh
python3 scripts/fetch-deps.py
```

`external/ChuSQL` 与 `external/ChuSQL-RustDriver` 为 Git 子模块。脚本优先使用 GitHub HTTPS，失败后回退 `https://repo.chunana.cn/ChuNa7a/` 下的同名仓库；已有正确版本直接复用，有本地修改时提示处理。

本机已有依赖仓库时，可指定来源路径：

```sh
python3 scripts/fetch-deps.py \
  --database-repo ../ChuSQL --driver-repo ../ChuSQLDriver
```

源仓库应包含项目固定的提交。更新依赖时提交子模块指针，依赖源码和构建产物留在外部仓库。

## 本地检查

本地使用源码检查、单元测试和命令替身测试，容器联调安排在部署环境。源码检查需要 Rust；前端命令见[前端文档](frontend.md)。

```sh
cargo test --all-targets
cargo fmt --check
cargo clippy --all-targets -- -D warnings
python3 scripts/test-prepare-deploy.py
python3 scripts/test-fetch-deps.py
python3 scripts/test-deployment.py
```

账户集成测试读取 `CHUBLOG_TEST_DB_ADDRESS` 和 `CHUBLOG_TEST_DB_PASSWORD`，需要支持 ENUM 的独立 ChuSQL 测试实例及 root 凭据。默认 Cargo 测试会跳过这些用例；设置测试环境后，`cargo test -- --include-ignored` 可执行全部测试。测试在实例中创建随机命名数据库。

## 部署环境联调

以下命令供指定部署环境手动执行，会使用 Docker：

```sh
python3 scripts/test-accounts-db.py --database-image chublog-db:latest
python3 scripts/smoke.py --https --sources-env-file deploy/sources.public.env
python3 scripts/smoke-deployment.py --sources-env-file deploy/sources.public.env
```

联调使用临时项目与数据，覆盖安装、管理员入库、Git 权限隔离、文章同步和故障恢复，结束后清理自身资源。网站使用本机 1741 端口，HTTPS 测试另用 18443 端口和临时 Caddy 私有 CA，并保持证书校验。

安装 Playwright 与 Chromium 后，`smoke.py --browser-test` 可追加页面检查，结合相同的源配置参数使用。

## 内置文章

模板位于 `apps/api/starter/`，首次写入由 `apps/api/src/starter.rs` 完成。模板变更影响后续新安装，已有站点由管理员通过 Git 更新文章。数据库字段与上游类型参考见[数据库文档](database.md)。
