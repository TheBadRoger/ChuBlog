# Astro 前端

[返回项目首页](../README.md)

前端位于 `apps/web`，使用 Astro、TypeScript 和 Node 服务端渲染。首页按要求展示居中的“个人博客正在施工中”，下方居中列出已完成和未完成部分。

## 页面

| 地址 | 功能 |
| --- | --- |
| `/` | 施工说明、进度与导航，内容全部居中 |
| `/articles` | 文章列表、标题/简介/作者/标签搜索、标签筛选和分页 |
| `/posts/作者名~文章ID` | 阅读入口文档、图片和文章元信息 |
| `/posts/作者名~文章ID/页面名.md` | 阅读同文章的其他可达页面，保留相对链接和锚点 |
| `/login`、`/login?mode=register` | 登录、注册与错误反馈 |
| `/account` | 公钥登记、密码修改、退出和 Git 发布说明；管理员可授权账户并查看待修复索引 |

未实现的评论、互动、内部文章阅读、审核和热度排序不提供占位操作按钮。文章写入继续通过 Git，不提供在线编辑。

## 本地开发

需要 Node.js 22.22 或更高版本，Docker 前端镜像使用 Node.js 24。先启动 API，并将其 `PUBLIC_ORIGIN` 设为浏览器使用的地址，例如 `http://127.0.0.1:4321`。

```sh
cd apps/web
npm ci
cp .env.example .env
npm run dev
```

打开 `http://127.0.0.1:4321`。`API_BASE_URL` 默认 `http://127.0.0.1:3001`，开发服务器将 `/api` 请求转发到该地址。`PUBLIC_ORIGIN` 用于显示正确的服务器 SSH 主机名，应与后端配置及浏览器地址一致。`SSH_PORT` 控制发布说明中的端口。

```sh
npm run build
npm test
```

构建同时检查 Astro 和 TypeScript。生产运行 `npm start` 只启动页面服务，API 同源转发由 Caddy 提供，见[部署说明](deployment.md)。

## 安全与访问

浏览器使用后端 HttpOnly Cookie，CSRF 只保存在内存，不写入 localStorage。修改请求携带当前会话的 CSRF，Origin 由浏览器发送；后端继续执行全部权限检查。

Markdown 禁用原始 HTML，并在渲染后清理输出。文章内链接只映射到 API 返回的同文章页面和图片；普通外部链接仍受协议限制。纯文本通过转义输出显示。页面设置 CSP、禁用缓存和内容类型嗅探，不加载第三方字体、脚本或跟踪服务。

文章服务故障返回可重试提示，不填充示例数据。注册被关闭、会话失效和限流错误直接显示后端反馈。

## 浏览器检查

以下检查用于开发电脑，需要 Python Playwright 及 Chromium；生产部署无需安装这些工具：

```sh
python3 -m pip install playwright
python3 -m playwright install chromium
python3 scripts/smoke.py --key-type rsa --browser-test \
  --sources-env-file deploy/sources.public.env
```

该命令使用隔离的 Docker 项目，验证真实注册登录、密码修改、公钥保存、站长授权、多页阅读及手机布局，测试后清理自己创建的容器与卷。截图默认放在 `/tmp/chublog-web-screenshots`。系统缺少浏览器库时，按 Playwright 安装说明准备运行环境。

## 项目技能

按维护者要求下载到本项目 `.agents/skills/`，作为开发参考，不是运行时依赖：

- [frontend-design](../.agents/skills/frontend-design/SKILL.md)，来源为 [Anthropic skills](https://github.com/anthropics/skills/tree/main/skills/frontend-design)。
- [webapp-testing](../.agents/skills/webapp-testing/SKILL.md)，来源为 [Anthropic skills](https://github.com/anthropics/skills/tree/main/skills/webapp-testing)。
- [web-design-guidelines](../.agents/skills/web-design-guidelines/SKILL.md)，来源为 [Vercel agent skills](https://github.com/vercel-labs/agent-skills/tree/main/skills/web-design-guidelines)。

后续会话可继续发现这些技能。本轮已直接读取，按项目要求采用居中施工页、克制排版、键盘焦点、表单标签和真实浏览器检查。
