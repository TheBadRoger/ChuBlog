# 前端

[返回项目首页](../README.md)

前端位于 `apps/web`，使用 Astro、TypeScript 和 Node 服务端渲染。网站名称和默认描述来自安装配置，页面正文使用统一阅读组件。

## 页面

| 地址 | 功能 |
| --- | --- |
| `/` | 安装前进入安装页，安装后提供全屏 Hero 和下滑导航 |
| `/setup` | 网站信息、管理员账号及昵称初始化，完成后进入登录页 |
| `/articles` | 文章列表、搜索、标签筛选和分页 |
| `/posts/用户名/文章ID` | 文章入口文档 |
| `/posts/用户名/文章ID?page=页面名` | 同文章的其他页面 |
| `/posts/管理员用户名/publishing`、`/posts/管理员用户名/development` | 两篇内置文章 |
| `/login`、`/login?mode=register` | 登录与注册（含昵称） |
| `/admin/users` | 管理员专属的用户状态与权限管理 |
| `/admin/articles` | 作者的个人文章管理；管理员额外可见用户文章审核与全部暂存索引 |
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

## 页面行为

前端支持主题切换、代码高亮和多页阅读。文章管理按账户权限显示，受限正文要求登录、输入口令或取得授权。

接口故障时提供重试入口。浏览器联调命令见[开发文档](development.md#部署环境联调)。
