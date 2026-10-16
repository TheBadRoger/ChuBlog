# ChuBlog

Rust/Axum 博客后端。作者在独立 Git 仓库中编辑文本或 Markdown，通过受限 SSH Git 推送发布；`index.txt` 管理文章清单。HTTP API 提供阅读、账户与发布授权，ChuSQL 保存可重建索引。

当前仅提供后端，暂无前端和在线文章编辑。

## 文档

| 文档 | 内容 |
| --- | --- |
| [开发与依赖](docs/development.md) | 获取代码、双 HTTPS 依赖源、开发检查与联调 |
| [部署](docs/deployment.md) | Debian 13、Docker Compose、首次初始化 |
| [文章发布](docs/publishing.md) | 账户授权、个人仓库、文章清单与 Git 工作流 |
| [API](docs/API.md) | HTTP 接口、认证、请求与错误响应 |
| [内容与隔离](docs/security.md) | 文件白名单、容量限制、SSH 与进程边界 |
| [运维与迁移](docs/operations.md) | 持久化、备份恢复、旧共享仓库迁移 |
| [架构决策](docs/decisions/README.md) | 已记录的设计选择及演进 |

源码：[GitHub](https://github.com/TheBadRoger/ChuBlog.git) · [自建仓库](https://repo.chunana.cn/ChuNa7a/ChuBlog.git)

规划：[当前范围与待办](TODO.md) · [原始规划](PLAN.md)
