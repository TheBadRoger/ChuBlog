# 账户授权与文章发布

[返回项目首页](../README.md)

## 用户与发布授权

完整接口见 [API 文档](API.md)。

1. 注册：`POST /api/auth/register`，正文 `{ "username": "alice", "password": "至少12字节的口令" }`。默认角色 `reader`，绝不接受客户端指定角色。`ALLOW_REGISTRATION=false` 可关闭注册。
2. 登录：`POST /api/auth/login`，相同字段。浏览器保存 HttpOnly Cookie；返回 `csrf`。所有修改请求必须携带站点 `Origin` 和 `X-CSRF-Token`。
3. 本地生成 `ssh-keygen -t ed25519` 或 `ssh-keygen -t rsa -b 3072`；登录后 `PUT /api/account/ssh-key` 提交 `{ "public_key": "ssh-ed25519 AAAA..." }`，只交公钥。RSA 使用标准 `ssh-rsa AAAA...` 公钥文本，支持 2048–8192 位；实际认证使用 RSA/SHA-256 或 RSA/SHA-512，不启用旧的 SHA-1 签名。见 [OpenSSH 说明](https://www.openssh.com/txt/release-8.8)。
4. 管理员登录后调用 `PATCH /api/admin/users/alice`，正文 `{ "enabled": true, "role": "publisher" }`。服务器按公钥绑定的用户分配独立仓库。
5. 此时 SSH 才接受该用户的密钥；`reader`、停用用户、未绑密钥的用户均不能访问 Git。管理员也需登记公钥才能发布。旧 `prefixes` 字段仅为兼容保留，不再用于文章隔离。

- 授权与密钥独立：知道服务器地址、有网站账户、上传了公钥都不足以推送。
- 设回 `reader` 或 `enabled:false` 会撤回发布授权，并使现有网站会话失效。
- 禁止管理员停用自己或取消自己的管理角色。

## 本地 Git 工作流

- 先通过可信渠道核对服务器 Ed25519 主机密钥指纹；不要禁用主机密钥检查。
- 主机密钥保存在独立持久卷，重建容器不会更换。
- 所有发布者使用同一个 OS 账户 `git`，身份由对应公钥的强制命令绑定，不由 Git author/email 决定。

```sh
# ~/.ssh/config
Host chublog
    HostName blog.example.com
    Port 2222
    User git
    IdentityFile ~/.ssh/id_ed25519
    IdentitiesOnly yes

# 无论作者是谁，别名都是 content.git；服务端按认证身份映射个人仓库。
# 首次 clone 自动创建服务器裸仓库，作者无须自己创建裸仓库。
git clone chublog:content.git my-articles
cd my-articles
# 仓库已有内容时，每轮编辑前先 git pull --ff-only。
# 创建 index.txt 和正文，格式见下文。
git add index.txt posts/first
git commit -m 'Publish first article'
git push -u origin main
```

## 文章目录与清单

仓库结构示例：

```text
index.txt
posts/
└── first/
    ├── article.md
    └── cover.png
```

`index.txt` 为 UTF-8 三列文本，以空格或 Tab 分隔，无表头，允许空行：

```text
first posts/first article.md
notes posts/my-notes notes.markdown
```

- 三列依次为**文章 ID、相对文件夹、正文文件名**。
- ID 在作者内唯一，支持 1–40 位小写字母、数字及中间短横线，另须避开保留名称。
- 目录必须是 `posts/<目录名>`，目录名遵循 slug 规则，最多 80 字节；文件名只允许 ASCII 字母、数字、短横线、下划线与点，不能以点开头或含 `..`。
- 同一目录不能重复登记；正文可以任意命名，扩展名为 `.md`、`.markdown` 或 `.txt`。
- 索引所指目录和正文必须存在，路径穿越和子目录正文被拒绝。
- 索引上限 64 KiB、1000 项。

## 正文与图片

- 正文沿用 YAML Frontmatter（参考[正文样例](../content/posts/welcome/index.md)），包含 `title/date/updated/status/tags/description`；`slug` 可省略，如填写则必须等于索引 ID。
- `.txt` 正文也需要 Frontmatter，API 通过 `format: text` 表明正文应作为纯文本展示；其他正文为 `format: markdown`。
- 图片置于同一目录，通过 `![说明](cover.png)` 引用。

## 新增、修改与撤回

| 作者操作 | 系统处理 |
| --- | --- |
| 新建正文并在索引新增一行 | 校验后导入文章，在 ChuSQL 创建作者、ID、正文路径、状态等元信息 |
| 修改正文或索引中的目录/文件名，保留 ID | 更新同一文章；站内 ID 不随目录移动而变化 |
| 删除索引中的一行 | 移除当前阅读内容、图片入口及对应数据库元信息，无需先删除本地正文 |
| 把索引清空 | 移除该作者所有当前文章，不影响其他作者 |

- 未被索引选中的正文及目录视为无效内容，不导入阅读快照或数据库；图片只有被选中正文引用才作为公开资源导入；仍计算仓库容量并执行安全校验。
- 删除正文但保留其索引项属于错误，整次推送会被拒绝。
- `status: draft` 可保留登记但不公开；物理删除文件和撤回文章均不会擦除 Git 历史。

## 提交与并发限制

- MVP 每次只接受**一个直接继承个人远端 main 的提交**，首次仅允许一个根提交。
- 禁止 merge、force push、删除 main、标签及其他分支；多次本地提交应先整理为一次提交，再变基到最新远端。
- 同一作者在多台电脑操作仍需处理并发冲突，不同作者互不争用同一个仓库锁。

## 作者隔离与文章地址

- 站内文章 ID 为 `作者名~文章ID`，例如 Alice 的 `first` 对应 `/api/posts/alice~first`；不同作者可以使用相同的本地 ID。
- 服务端分别保存每位作者的 Git 对象与历史，SSH 不能指定别人的仓库路径；作者无法通过 clone 或按提交哈希 fetch 读取其他作者草稿。
- 作者自己已克隆的历史仍可在本地保留，不能靠撤回收回副本。

## 发布状态

- API 自动同步，一般约 2 秒，数据库重建、失败重试或较多作者可能延迟。
- `GET /api/admin/status` 的 `commit` 是作者提交映射的集合摘要，不是任一作者 Git 提交；`indexed_commit` 表示数据库已同步的集合版本。
- Git 接受表示接收校验完成，不保证数据库已同步。
- 当前不做站长内容审核，审核计划见 [TODO](../TODO.md)。
