# 前端

[返回项目首页](../README.md)

前端位于 `apps/web`，使用 Astro、TypeScript 和 Node 服务端渲染。网站名称和默认描述来自安装配置，页面正文使用统一阅读组件。

## 页面

| 地址 | 功能 |
| --- | --- |
| `/` | 安装前进入安装页，安装后提供导航 |
| `/setup` | 网站信息与唯一站长初始化，完成后进入登录页 |
| `/articles` | 文章列表、搜索、标签筛选和分页 |
| `/posts/作者名~文章ID` | 文章入口文档 |
| `/posts/作者名~文章ID/页面名.md` | 同文章的其他页面 |
| `/posts/站长名~publishing`、`/posts/站长名~development` | 两篇内置文章 |
| `/login`、`/login?mode=register` | 登录与注册 |
| `/account` | 密码、发布指引、站长授权和暂存状态 |

## 本地开发

需要 Node.js 22.22 或更高版本，并启动后端 API：

```sh
cd apps/web
npm ci
cp .env.example .env
npm run dev
```

页面位于 `http://127.0.0.1:4321`。`API_BASE_URL` 默认指向 `http://127.0.0.1:3001`，Astro 同源转发 `/api/*` 和 `/git/*`。

```sh
npm run build
npm test
```

构建包含 Astro 和 TypeScript 检查。生产入口运行 `npm start`，Docker 端口见[部署文档](deployment.md)。

## 渲染与验证

浏览器使用 HttpOnly Cookie 和内存 CSRF。Markdown 输出经过清理，纯文本及元数据经过转义，页面设置 CSP。文章内链接按后端返回的页面与资源映射，服务故障显示重试提示。

真实浏览器联调在指定部署环境运行，命令见[开发文档](development.md#部署环境联调)。项目开发技能位于 `.agents/skills/`：[设计](../.agents/skills/frontend-design/SKILL.md)、[浏览器测试](../.agents/skills/webapp-testing/SKILL.md)、[界面检查](../.agents/skills/web-design-guidelines/SKILL.md)。
