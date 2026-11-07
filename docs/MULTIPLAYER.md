# Minecraft 联机中心（SRL Multiplayer）

SRL 通过 52FRP 开放 API v1 建立 Minecraft 联机隧道。用户不需要公网 IP、路由器端口映射或了解 FRP。

## 使用流程

1. 登录 <https://www.52frp.com/user/#/user-center>。
2. 在「API 访问令牌」创建一个以 `hz_` 开头的令牌；明文只显示一次。
3. 打开 SRL →「Minecraft 联机」，把令牌粘贴到连接框，点击「连接 52FRP」。
4. 启动 Minecraft，Java 版在游戏内「对局域网开放」。
5. 选择实例与版本类型，SRL 会自动检测动态端口；也可以手动填写。
6. 点击「创建联机房间」，等待逐步检查完成。
7. 复制公网地址发给好友。
8. 停止时选择「停止联机」（保留隧道）或「停止并删除临时隧道」。

令牌只保存在系统凭据库，不写入 `settings.json`、日志或 Git 仓库。

## API

基础地址：`https://www.52frp.com/api/v1`

请求头：`Authorization: Bearer hz_...`

机器可读规格：<https://www.52frp.com/api/v1/openapi.json>

| 用途 | 接口 |
|---|---|
| 账户 | `GET /account` |
| 实名状态 | `GET /identity` |
| 节点列表 | `GET /nodes` |
| 节点负载 | `GET /nodes/load` |
| 隧道列表 | `GET /tunnels` |
| 名称检查 | `GET /tunnels/check-name` |
| 端口检查 | `GET /tunnels/check-port` |
| 已占用端口 | `GET /tunnels/occupied-ports` |
| 创建隧道 | `POST /tunnels`（JSON） |
| 隧道详情/在线状态 | `GET /tunnels/{id}` |
| 删除隧道 | `DELETE /tunnels/{id}` |
| 完整 FRPC TOML | `GET /nodes/{node_id}/config/file?proxy_ids={id}&format=toml` |

Java 使用 `tcp`，Bedrock 使用 `udp`。远程端口必须落在节点 `proxies_allowed` 范围内，并避开 `proxies_denied` 与已占用端口；创建前调用 `check-port`。

## 模块

- `MultiplayerManager`：业务调度、状态机、房间持久化、回滚。
- `Frp52Provider`：52FRP API 实现。
- `Frp52TokenStore`：API 令牌的系统凭据库存储。
- `TunnelProvider`：提供商接口，便于未来替换。
- `NodeSelector`：延迟 + 节点负载 + 隧道配额评分。
- `MinecraftDetector`：日志动态端口、进程监听端口、TCP/UDP 可达性。
- `FrpcManager`：上游 frp 客户端下载、SHA-256 校验、启动、日志、停止。

## FRPC

52FRP 教程使用 fatedier/frp 官方客户端。SRL 从 GitHub Releases 获取最新版：

- 通过 GitHub Release API 读取 `digest`（SHA-256）或 `frp_sha256_checksums.txt`。
- 下载对应平台的 `frpc` / `frpc.exe`，校验 SHA-256 后才运行。
- 下载失败时优先尝试 `ghproxy.com` / `gh-proxy.com` / `mirror.ghproxy.com` 镜像，单个源 15 秒超时，总上限 50 秒；校验值不变。
- 已下载的客户端优先复用，创建房间不再每次请求 GitHub；连接账户后会在后台预下载，24 小时检查一次更新。
- 下载过程通过联机步骤显示已下载大小和速度。
- 配置来自 52FRP 的完整 TOML（含 `serverAddr`、`user`、`auth.token`），写入 `0600` 临时文件，停止时删除。

## 安全边界

- API 令牌只存系统凭据库；不写入普通配置、日志或仓库。
- FRPC TOML 含连接凭据，使用私有临时文件；日志中的 token/secret/password 行会被隐藏。
- 删除隧道只允许 `SRL_` 前缀且属于当前账户的隧道；不批量删除用户其他隧道。
- 应用退出时停止 FRPC；异常退出留下的 SRL 隧道会在联机页面显示并可单独清理。
- 创建接口没有幂等键，SRL 通过单飞锁、名称检查和房间记录防止重复提交。

## 已知限制

- 需要真实 52FRP 账户与 API 令牌完成在线联调；没有令牌时无法伪造测试成功。
- 账户必须完成实名认证，且套餐覆盖目标节点，`available` 才为 true。
- Bedrock 需要本机已有 UDP 服务监听；SRL 检查端口占用，但不启动 Bedrock 服务端。
- 52FRP 节点列表的 `available` 表示“现有套餐已覆盖”，不是“可首购”；SRL 不自动购买套餐。

## 测试

```bash
cargo test -p sr-core multiplayer::
npm test -- --run
```

覆盖：52FRP 响应解析、端口范围、节点评分、Minecraft LAN 日志、`lsof`/`ss`/`netstat`、FRPC SHA-256/平台映射、日志脱敏、FRPC 启停、房间持久化。

## 界面与帮助（0.2.0 更新）

- 主页面只保留账户、创建房间、当前状态三块。字段解释集中到旁边的 `?`，悬停或键盘聚焦时显示；提示浮层挂在 `document.body` 的固定层，不依赖卡片裁切。
- 当前状态卡片右上角有「帮助」按钮，包含 52FRP 注册/实名/套餐、API 令牌权限、房主与朋友操作、离线联机模组和常见错误。
- 账号管理只保留账号名、类型、当前状态与操作；设置页隐藏重复说明，减少视觉噪音。
- 安装包不包含用户账号、实例、皮肤或缓存；每次发布前使用 `scripts/verify-clean-bundle.sh` 检查。
