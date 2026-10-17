# 文章发布

[返回项目首页](../README.md)

完整操作教程保存在[内置发文文章](../apps/api/starter/posts/publishing/main.md)。首次安装后，它以站长名义发布，首页提供阅读入口。本页汇总作者仓库与索引规则。

## 账户与仓库

站长在账户页授予 AUTHOR 权限，作者使用网站账号密码克隆账户页给出的 HTTPS 地址。所有作者共享 `/git/content.git` 入口，服务端按账号分配独立仓库；克隆和推送均校验身份与发布权。

安全凭据管理器可以保存密码。保持 HTTPS 证书验证开启，账号密码保存在系统凭据管理器中；Git 的 `credential.helper store` 使用明文存储。

## TOML 索引

根目录 `index.toml` 为 UTF-8，每篇文章对应一个 `[[articles]]`：

```toml
[[articles]]
id = "first"
folder = "posts/first"
title = "第一篇文章"
description = "文章简介"
tags = ["随笔"]
public = true
# cover = "cover.png"
# entry = "main.md"
```

| 字段 | 规则 |
| --- | --- |
| id | 作者内唯一，1–40 位小写字母、数字和中间短横线 |
| folder | `posts/目录名`，目录名最多 80 字节且满足 slug 规则 |
| title | 必填，最多 240 字节 |
| description | 默认空，最多 1000 字节 |
| tags | 默认空，最多 12 项，每项最多 80 字节 |
| cover | 可选，同目录图片文件名 |
| public | 默认 false，true 时公开 |
| entry | 默认 main.md，支持 md、markdown、txt |

同一目录登记一次。ID 和目录使用普通 slug，系统保留名称见源码校验器。文件名使用 ASCII 字母、数字、短横线、下划线和点，最多 100 字节，以普通字符开头，并通过连续点与路径校验。作者与系统时间由服务器生成。

入口通过同目录相对链接连接其他页面，可携带锚点。系统导入可达页面、页面引用图片及封面；其余文件仍受上传安全检查。示例见 [index.toml](../content/index.toml) 和[多页正文](../content/posts/welcome/main.md)。

## 提交与撤回

每次推送包含一个直接接续远端 main 的提交，空仓库首次推送为根提交。协议范围为 main 的线性历史；多次本地提交整理为一个提交后推送。

| 内容 | 处理结果 |
| --- | --- |
| 有效索引与入口 | 发布或更新文章，保留首次公开时间 |
| 有效索引移除条目，或入口缺失 | 撤回该文章及页面资源 |
| `articles = []` | 撤回全部文章 |
| 索引缺失、语法或元信息错误 | 暂存提交，沿用上次生效版本 |
| 文件、内容或容量违规 | 整次推送退回，保留远端分支 |

站长仓库初始包含教程与开发进度，之后按普通文章维护。撤回后 Git 历史和已克隆副本继续保留。public=false 的文章、页面和资源在公共接口返回 404。站长可通过 `/api/admin/status` 查看暂存原因与同步进度，具体配额见[安全文档](security.md)。
