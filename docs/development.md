# 开发

[返回项目首页](../README.md)

## 代码与依赖

克隆步骤见 [README](../README.md#快速开始)。开发时获取固定版本依赖：

```sh
python3 scripts/fetch-deps.py
```

依赖使用固定提交，下载失败时自动回退。已有正确版本直接复用，有本地修改时停止。

本机已有依赖仓库时，可指定来源路径：

```sh
python3 scripts/fetch-deps.py \
  --database-repo ../ChuSQL --driver-repo ../ChuSQLDriver
```

源仓库应包含项目固定的提交。更新依赖时提交子模块指针，依赖源码和构建产物留在外部仓库。

## 本地检查

测试位于 `test/`，需要 Rust。前端命令见[前端文档](frontend.md)。

```sh
cargo test --all-targets
cargo fmt --check
cargo clippy --all-targets -- -D warnings
python3 test/test-prepare-deploy.py
python3 test/test-fetch-deps.py
python3 test/test-deployment.py
```

数据库集成测试需要独立 ChuSQL 实例，并设置 `CHUBLOG_TEST_DB_ADDRESS` 和 `CHUBLOG_TEST_DB_PASSWORD`。测试会修改数据库，完整测试应串行执行：

```sh
cargo test --all-targets -- --include-ignored --test-threads=1
```

验证结果见[项目进度](progress.md)。

## 部署环境联调

以下命令供指定部署环境手动执行，会使用 Docker：

```sh
python3 test/test-accounts-db.py --database-image chublog-db:latest
python3 test/smoke.py --https --sources-env-file deploy/sources.public.env
python3 test/smoke-deployment.py --sources-env-file deploy/sources.public.env
```

联调创建并清理临时项目，使用本机 1741 和 18443 端口。

安装 Playwright 与 Chromium 后，`smoke.py --browser-test` 可追加页面检查，结合相同的源配置参数使用。

## 内置文章

模板位于 `apps/api/starter/`，首次写入由 `apps/api/src/starter.rs` 完成。模板变更影响后续新安装，已有站点由管理员通过 Git 更新文章。数据库字段与上游类型参考见[数据库文档](database.md)。
