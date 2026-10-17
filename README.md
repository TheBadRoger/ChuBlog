# ChuBlog

Rust/Axum 博客后端。作者在独立 Git 仓库中编辑文本或 Markdown，通过账号密码认证的 HTTPS Git 推送发布；`index.toml` 管理文章清单、元信息和多页入口。HTTP API 提供阅读、账户与发布授权，ChuSQL 保存可重建索引。

提供 Rust/Axum 后端和 Astro + TypeScript 简单前端。首页为居中的施工说明与导航，支持文章阅读、注册登录和账户设置；文章继续通过 Git 发布。

在项目根目录一键部署（默认腾讯云内网源）：

```sh
scripts/deploy.sh
```

博客默认监听 `127.0.0.1:1741`，域名和 HTTPS 由服务器现有反向代理管理，无需填写域名环境变量。

宿主机只需 Docker Compose、Python 3 和 Git。脚本自动完成依赖获取、容器内编译、数据库初始化和启动，不创建初始管理员。更新使用 `scripts/update.sh`，卸载使用 `scripts/uninstall.sh`，详细配置见[部署文档](docs/deployment.md)。

## 文档

| 文档 | 内容 |
| --- | --- |
| [开发与依赖](docs/development.md) | 获取代码、双 HTTPS 依赖源、开发检查与联调 |
| [前端](docs/frontend.md) | 页面、本地开发、浏览器联调和项目技能 |
| [部署](docs/deployment.md) | 容器内构建、Docker Compose、首次初始化 |
| [文章发布](docs/publishing.md) | 账户授权、个人仓库、文章清单与 Git 工作流 |
| [API](docs/API.md) | HTTP 接口、认证、请求与错误响应 |
| [内容与隔离](docs/security.md) | 文件白名单、容量限制、HTTPS Git 与进程边界 |
| [运维与迁移](docs/operations.md) | 持久化、备份恢复、旧共享仓库迁移 |

源码：[GitHub](https://github.com/TheBadRoger/ChuBlog.git) · [自建仓库](https://repo.chunana.cn/ChuNa7a/ChuBlog.git)

