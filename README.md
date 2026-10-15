# ChuBlog

Rust/Axum 博客后端。文章作者在本地编辑 Markdown，通过受限 SSH Git 推送发布。注册、登录、权限管理使用 HTTP API；文章仅通过 Git 修改。ChuSQL 通过外部子模块 `external/ChuSQL-RustDriver` 接入，只存可重建索引。

本轮按最新需求只实现后端，没有前端或在线文章管理。`PLAN.md` 保留原始规划，当前范围与后续安排见 [TODO.md](TODO.md)。

## 项目与运行

```text
ChuBlog/
├── apps/api/
├── external/
│   ├── ChuSQL/             # Git 子模块，主仓库只记录版本指针
│   └── ChuSQL-RustDriver/  # Git 子模块
├── config/git-local.config
└── scripts/init-deps.py
```

先初始化外部依赖：

```sh
python3 scripts/init-deps.py
```

子模块登记地址为 `ChuGit:/srv/git/ChuSQL.git` 和 `ChuGit:/srv/git/ChuSQL-RustDriver.git`。初始化脚本按以下顺序获取**固定提交**：

1. 本机 SSH：`ssh://git@localhost/srv/git/ChuSQL.git`、`ssh://git@localhost/srv/git/ChuSQL-RustDriver.git`。
2. 本机连接、访问或获取固定提交失败，回退到 `git@github.com:TheBadRoger/ChuSQL.git`、`git@github.com:TheBadRoger/ChuSQL-RustDriver.git`。

已正确初始化的固定版本直接复用；有未提交修改时拒绝覆盖。不会因回退而改为任意最新分支。`config/git-local.config` 提供 `ChuGit`/自建服务器地址到 localhost SSH 的可选 `insteadOf` 映射；刻意不改写 GitHub 地址，以免回退又被映射回本机。脚本不启用全局 `file` 协议，也不递归初始化未知子模块。

开发电脑仍可显式选择已有本地来源（仅开发覆盖参数，不是服务器默认流程）；只读取已提交版本：

```sh
python3 scripts/init-deps.py --database-repo ../ChuSQL --driver-repo ../ChuSQLDriver
```

子模块固定到已提交版本；源仓库必须包含这些提交。依赖源码和构建产物不进入博客仓库，更新依赖时只提交 `.gitmodules`/配置及子模块指针。不要对主仓库使用 `git add --force` 重新收录依赖文件。


开发检查：

```sh
cargo test --all-targets
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

部署基于 Debian 13。项目和依赖工作目录应归 `deploy` 用户所有；为该用户配置可访问 `git@localhost` 与 GitHub 的 SSH 密钥，并预先核对两端主机密钥。私钥留在主机，不能放进镜像。

推荐入口（以 root 调 Docker，Git 与编译均以 deploy 运行）：

```sh
sudo /srv/apps/ChuBlog/scripts/deploy.sh
```

脚本先以 `sudo -H -u deploy` 运行 `init-deps.py`，再构建外部项目与 Docker 镜像。默认依赖工作树位于 `/srv/apps/ChuBlog/external/`。其数据库拉取等价于你指定的方式：

```sh
sudo -u deploy git clone ssh://git@localhost/srv/git/ChuSQL.git /srv/apps/chusql
# 本机来源失败时：
sudo -u deploy git clone git@github.com:TheBadRoger/ChuSQL.git /srv/apps/chusql
```

如选择这个独立数据库目录，可用 `prepare-deploy.py --chusql-source /srv/apps/chusql --build-database` 构建；默认部署入口使用子模块固定版本。不要将数据库单独目录的源代码或产物加入博客提交。

首次部署先准备 `.env`，再运行入口。也可以手动执行以下同架构构建步骤；远程服务器上的 Git/编译命令应以 `deploy` 身份执行，Docker 命令由 root 执行：

```sh
# ChuSQL 尚未编译时，先按其项目说明构建 Rust 存储库、server 与 bootstrap。
python3 scripts/prepare-deploy.py --build-database
cp .env.example .env
chmod 600 .env
# 从标准输入输入管理员口令，Ctrl-D 结束；不要把明文写进命令行历史。
./deploy/bin/chublog-api hash-password
# 将输出以单引号包裹写入 .env 的 ADMIN_PASSWORD_HASH；同时配置域名及两份不同的随机 DB 口令。
docker compose --env-file .env -f deploy/compose.yaml up -d --build
# db 启动就绪后，创建 chublog 库和非超级用户 chublog_index；失败时检查日志后重试。
docker compose --env-file .env -f deploy/compose.yaml run --rm db-init
```

部署入口启动服务后，首次仍需执行上面的 `db-init` 命令；入口不会在数据库启动竞态中静默吞掉初始化错误。

构建时若 Debian 官方下载源较慢，可在 `.env` 配置 `DEBIAN_MIRROR=mirrors.ustc.edu.cn`，仍使用 Debian 签名校验。

`prepare-deploy.py` 默认从数据库子模块读取编译产物；`--build-database` 会先构建其 Rust 存储库、Haskell server 和 bootstrap。也可用 `--chusql-source /已有构建的/ChuSQL` 选择已有外部构建，没有产物会明确失败。编译产物只暂存在被忽略的 `deploy/bin/`。跨平台部署须先在目标 Debian/架构构建，不能复制 macOS、Windows 或其他架构二进制。镜像不含编译器、源码、`.env` 或密码文件。首次启动从环境中的哈希创建管理员，后续账号与权限以持久文件为准，修改初始哈希不会覆盖已有账户。

Caddy 根据 `SITE_ADDRESS` 提供 HTTPS 和 API 反向代理；`PUBLIC_ORIGIN` 必须与外部站点完全一致，不带结尾斜杠。HTTP 仅对 localhost 开发场景允许。只需公网 TCP 80/443 和 Git SSH 2222（可改）；主机管理 SSH 另行管理，数据库和 API 不映射宿主端口。既有 UDP 7001 不受影响。参考 [Caddy 请求体限制](https://caddyserver.com/docs/caddyfile/directives/request_body) 与 [反向代理文档](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy)。

## 用户与发布授权

完整接口见 [docs/API.md](docs/API.md)。

1. 注册：`POST /api/auth/register`，正文 `{ "username": "alice", "password": "至少12字节的口令" }`。默认角色 `reader`，绝不接受客户端指定角色。`ALLOW_REGISTRATION=false` 可关闭注册。
2. 登录：`POST /api/auth/login`，相同字段。浏览器保存 HttpOnly Cookie；返回 `csrf`。所有修改请求必须携带站点 `Origin` 和 `X-CSRF-Token`。
3. 本地生成 `ssh-keygen -t ed25519` 或 `ssh-keygen -t rsa -b 3072`；登录后 `PUT /api/account/ssh-key` 提交 `{ "public_key": "ssh-ed25519 AAAA..." }`，只交公钥。RSA 使用标准 `ssh-rsa AAAA...` 公钥文本，支持 2048–8192 位；实际认证使用 RSA/SHA-256 或 RSA/SHA-512，不启用旧的 SHA-1 签名。见 [OpenSSH 说明](https://www.openssh.com/txt/release-8.8)。
4. 管理员登录后调用 `PATCH /api/admin/users/alice`，正文 `{ "enabled": true, "role": "publisher", "prefixes": ["alice-"] }`。不同有效发布者的前缀不能重叠。
5. 此时 SSH 才接受该用户的密钥；`reader`、停用用户、未绑密钥的用户均不能访问 Git。管理员也需显式分配前缀和公钥才能发布。

授权与密钥独立：知道服务器地址、有网站账户、上传了公钥都不足以推送。设回 `reader` 或 `enabled:false` 会撤回发布授权，并使现有网站会话失效。禁止管理员停用自己或取消自己的管理角色。

## 本地 Git 工作流

先通过可信渠道核对服务器 Ed25519 主机密钥指纹；不要禁用主机密钥检查。主机密钥保存在独立持久卷，重建容器不会更换。所有发布者使用同一个 OS 账户 `git`，身份由对应公钥的强制命令绑定，不由 Git author/email 决定。

```sh
# ~/.ssh/config
Host chublog
    HostName blog.example.com
    Port 2222
    User git
    IdentityFile ~/.ssh/id_ed25519
    IdentitiesOnly yes

# 首次发布，远端是空仓库时：
git init --initial-branch=main my-articles
cd my-articles
git remote add origin chublog:content.git
# 创建 posts/alice-first/index.md，规范参考 content/posts/welcome/index.md。
# 样例 slug welcome 需要改成自己获授权的 alice-first。
git add posts/alice-first
git commit -m 'Publish first article'
git push -u origin main

# 已有内容时先 clone；每轮编辑前拉取最新内容。
git clone chublog:content.git
cd content
git pull --ff-only
# 编辑 posts/alice-first/index.md
git add posts/alice-first
git commit -m 'Update article'
git push
```

MVP 每次只接受 **一个直接继承远端 main 的提交**，首次仅允许一个根提交。禁止 merge、force push、删除 main、标签及其他分支；本地有多次提交时先整理为单次提交，再拉取/变基到最新远端。发生并发冲突时，拉取并解决后重试。

`status: published` 发布，改为 `draft` 撤回，删除文章目录则删除当前版本。图片置于同一目录，Markdown 使用 `![说明](cover.png)`，不允许外链图片。API 在同步后读取新内容，一般约 2 秒；索引重建期间可能延迟到约 32 秒。`GET /api/admin/status` 查看已导入与已索引 commit；Git 接受只表示接收校验完成，尚不代表索引同步成功。

**授权发布者可以 clone 全部文章仓库及历史，包括其他发布者的草稿。** 草稿只对公开 HTTP 隔离，当前仓库不提供发布者之间的草稿保密；不要提交秘密或私钥。删除/撤回不能抹掉 Git 历史。

## 内容与隔离边界

- 唯一内容入口是 `posts/<slug>/index.md` 和同目录 PNG/JPEG/WebP；禁止额外文件、目录、符号链接、可执行位、子模块和 LFS 指针。
- Markdown 上限 256 KiB；图片单张 2 MiB、最多 12 张、尺寸 4096×4096；文章合计 6 MiB；快照原始文件合计 64 MiB、最多 1000 篇。
- Git 输入 pack 上限 8 MiB；接收仓库达到 256 MiB 后拒绝新 SSH 会话，需要管理员离线维护。此阈值是应用检查，生产还应为 Docker 数据盘配置真实文件系统配额。
- 使用成熟 Markdown 解析器，拒绝原始 HTML、危险链接协议、外链图片、控制符和双向隐藏字符；代码块是惰性文本，绝不执行 MDX、JS、仓库脚本、构建任务或上传内容。
- 支持 Ed25519 和 2048–8192 位 RSA 公钥，拒绝弱密钥、负数/畸形 RSA 参数及额外二进制数据；拒绝把 `authorized_keys` 选项当作公钥上传。SSH 只接受精确的 `git-receive-pack 'content.git'` / `git-upload-pack 'content.git'`，没有 shell、SCP/SFTP、TTY、转发、隧道、用户环境变量。
- 接收仓库由 UID 10002 管理；API 为独立 UID 10001，正式仓库/用户密码文件不挂载给 SSH 容器；SSH 只读授权文件和只读固定 hook。接收内容再次验证后才导入 API 自己的 Git 仓库。
- HTTP 请求体 8 KiB；注册/登录合计全局 10 次/分钟，修改操作全局 60 次/分钟；密码哈希并发最多 2。1000 用户、1000 个有效会话上限，一小时固定会话期限。全局限制偏保守，后续可细化 IP/账户限制以改善公平性。
- 统计不收集 IP/访客身份：公开文章数、标签数、Markdown 字节数、启动以来文章 API 请求数。请求数不是独立访客数，重启后清零。

服务器只返回 JSON Markdown 与图片；后续前端必须把元数据当文本输出，并继续禁用原始 HTML、MDX和危险链接，不能将接口字段直接作为 HTML 执行。隔离不能替代 Git/OpenSSH/解码库的持续安全更新。

## 持久化、故障和恢复

`content` 卷包含正式 `content.git`、`users.json` 和公开 `published.json`；`authorization` 卷是独立发布授权与公钥；`ingress` 是 SSH 接收仓库；`ssh-host-keys` 保留服务器身份。数据库和数据库日志为独立卷。日志输出 stdout，由 Docker 分卷轮换，不记录口令、会话或私钥。

文章索引失败不会修改 Git 或阻塞 HTTP 阅读，后台约每分钟重试并检查索引是否丢失。恢复数据库后可自动重建；离线命令为 `chublog-api rebuild-index`（必须停止同一数据卷上的 API 进程后运行，防止并发），或等待在线自动重试。`indexed_commit:null` 表示尚无已确认可用的索引，旧值表示尚未追平当前内容。应用不依赖 ChuSQL 事务。

备份必须同时包含正式 Git、用户文件、授权卷、SSH 主机密钥；备份包含密码哈希，须加密并限制权限。接收 Git 可额外镜像备份。恢复时先停止写入，从同一备份恢复各卷，再启动；不要只恢复 ChuSQL，也不要直接向正式仓库推送。应用启动从 Git 重新校验并生成快照。进程重启会注销所有会话。

本实现对文件写入执行原子替换与 fsync，Git 引用采用受控更新；磁盘故障、主机断电和备份恢复仍需真实环境演练。不要对含正式数据的项目执行 `docker compose down -v`。

## 联调

```sh
python3 scripts/prepare-deploy.py --build-database
python3 scripts/smoke.py
python3 scripts/smoke.py --key-type rsa
```

联调使用独立随机命名的 Compose 项目、本机 18080/18443/12222 端口，测试结束清理自己创建的容器与卷。覆盖真实 SSH 身份与授权、非法推送、发布撤回、ChuSQL 索引、断库阅读、重启与撤权。该测试会拉取基础镜像，需要 Docker 可用。


## 历史清理后的协作

驱动的旧源码副本已从主仓库可达历史移除，后续只保留外部子模块配置。历史重写后，已有克隆建议重新克隆并初始化依赖；不要把旧 main 合并回来，否则可能重新引入已删除的依赖历史。自建服务器的不可达对象清理与其他人的旧克隆需要分别处理，GitHub 缓存和旧提交直链的回收由平台管理。
