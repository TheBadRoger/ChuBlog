# ChuBlog

使用 Astro + TypeScript、Rust/Axum 和 ChuSQL 的个人博客。文章通过作者独立的 HTTPS Git 仓库发布，`index.toml` 管理文章元信息和多页正文。账户、权限及网站设置保存在 ChuSQL。

## 快速开始

服务器需要 Docker Engine、Docker Compose 插件、Python 3 和 Git，以及执行 Docker 命令的权限。编译在容器内完成，域名和 HTTPS 由服务器已有的反向代理管理。

1. 克隆项目：

   ```sh
   git clone https://github.com/TheBadRoger/ChuBlog.git
   cd ChuBlog
   ```

2. 部署博客：

   ```sh
   scripts/deploy.sh
   ```

   默认使用腾讯云内网源。其他网络可使用公网源配置：

   ```sh
   scripts/deploy.sh --sources-env-file deploy/sources.public.env
   ```

   脚本自动获取依赖、构建镜像、初始化数据库并启动服务，入口为 `127.0.0.1:1741`。

3. 获取安装密钥：

   ```sh
   docker compose --env-file .env -f deploy/compose.yaml \
     exec -T api sh -c 'cat /data/setup-token && printf "\n"'
   ```

4. 通过服务器反向代理打开网站的 `/setup`，填写网站名称、简介、管理员账号、昵称、密码和安装密钥。服务器本机也可访问 `http://127.0.0.1:1741/setup`。

安装完成后，首页提供导航，管理员仓库自动发布“发文章教程”和“开发进度”。登录管理员账户即可授权作者，作者从账户页获取个人仓库地址。

更新使用 `scripts/update.sh`，卸载使用 `scripts/uninstall.sh`。可选数据库客户端通过 `scripts/deploy.sh --with-cli` 安装。配置、代理和完整命令见[部署文档](docs/deployment.md)。

## 文档

| 文档 | 内容 |
| --- | --- |
| [部署](docs/deployment.md) | 首次安装、镜像源、代理、更新与卸载 |
| [文章发布](docs/publishing.md) | 作者授权、索引格式与发布规则 |
| [API](docs/API.md) | 接口、认证和响应 |
| [数据库](docs/database.md) | 字段类型、时间格式和安装记录 |
| [运维](docs/operations.md) | 备份、恢复和故障处理 |
| [安全限制](docs/security.md) | 内容白名单、容量和隔离边界 |
| [开发](docs/development.md) | 依赖、源码检查和联调 |
| [前端](docs/frontend.md) | 页面与本地开发 |

源码：[GitHub](https://github.com/TheBadRoger/ChuBlog.git) · [自建仓库](https://repo.chunana.cn/ChuNa7a/ChuBlog.git)
