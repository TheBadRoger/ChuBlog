# 前端

[返回项目首页](../README.md)

前端位于 `apps/web`，使用 Astro、TypeScript 和 Node 服务端渲染。网站名称和默认描述来自安装配置，页面正文使用统一阅读组件。

## 页面

| 地址 | 功能 |
| --- | --- |
| `/` | 安装前进入安装页，安装后提供全屏 Hero 和下滑导航 |
| `/setup` | 网站信息、管理员账号及昵称初始化，完成后进入登录页 |
| `/articles` | 文章列表、搜索、标签筛选和分页 |
| `/posts/作者名~文章ID` | 文章入口文档 |
| `/posts/作者名~文章ID/页面名.md` | 同文章的其他页面 |
| `/posts/管理员名~publishing`、`/posts/管理员名~development` | 两篇内置文章 |
| `/login`、`/login?mode=register` | 登录与注册（含昵称） |
| `/admin` | 管理员专属的账户状态、权限和暂存索引管理 |
| `/account` | 昵称展示、密码管理和发布指引 |

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

外观默认跟随系统，右下角的太阳、月亮和齿轮按钮分别选择浅色、深色和跟随系统，选择保存在本机。Hero 下方导航区域至少占满一屏，向下浏览可将 Hero 完整移出视口。Hero 动效尊重减少动态效果的系统设置。文章作者显示昵称；账户管理使用同一行的“昵称（用户名）”。文章正文与账户发布指引共用正文排版及 Shiki 代码高亮，使用受限 CSS 类适配两种主题。
