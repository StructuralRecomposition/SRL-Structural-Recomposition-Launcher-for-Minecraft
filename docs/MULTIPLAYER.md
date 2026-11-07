# Minecraft 联机中心（SRL Multiplayer）

SRL 通过 LoCyanFrp 官方 API v3 建立 Minecraft 联机隧道。用户不需要公网 IP、路由器端口映射或了解 FRP。

## 使用流程

1. 在设置页填写 LoCyanFrp OAuth 应用 ID。
2. 打开「Minecraft 联机」，点击「连接 LoCyanFrp 账户」。
3. 在系统浏览器完成授权，SRL 通过本地回环地址接收 refresh token。
4. 启动 Minecraft，Java 版在游戏内「对局域网开放」。
5. 选择实例与版本类型，SRL 会自动检测动态端口；也可以手动填写。
6. 点击「创建联机房间」，等待逐步检查完成。
7. 复制公网地址发给好友。
8. 停止时选择「停止联机」（保留隧道）或「停止并删除临时隧道」。

## OAuth 应用配置

在 <https://dashboard.locyanfrp.cn/app/create> 创建 OAuth2.0 应用：

- 回调地址：`https://dashboard.locyanfrp.cn/auth/oauth/redirect-localhost`
- 权限节点（默认）：
  `Node.Read,Node.Read.Stat,Node.RandomPort,Tunnel.Read,Tunnel.Read.Config,Tunnel.Read.Status,Tunnel.Write.Create,Tunnel.Write.Delete`

SRL 只保存公开的 `client_id`。它不会要求用户粘贴 Access Token，也不会把 refresh token 写入 `settings.json`、日志或仓库。

授权流程：

1. SRL 在本机随机端口启动回环 HTTP 监听。
2. 打开 `https://dashboard.locyanfrp.cn/auth/oauth/authorize?client_id=...&scopes=...&redirect_uri=...&mode=callback`。
3. 浏览器授权后，LoCyanFrp 重定向到本地监听，带回 `refresh_token`。
4. SRL 调用 `POST /v3/auth/oauth/access-token` 换取 1 小时有效的 access token。
5. refresh token 与 user id 存入系统凭据库；access token 仅驻留内存。

## API 使用

所有请求以 <https://api.locyanfrp.cn/v3> 为主，GET 请求在网络层失败时回退到 `https://backup.api.locyanfrp.cn/v3`。创建/删除等非幂等请求不自动重试，避免重复隧道。

| 用途 | 接口 |
|---|---|
| 用户信息 | `GET /user?user_id=` |
| 节点列表 | `GET /nodes?user_id=&page=&size=` |
| 节点统计 | `GET /nodes/stats?user_id=&nodes[]=` |
| 随机端口 | `GET /node/port?user_id=&node_id=` |
| 隧道列表 | `GET /tunnels?user_id=&page=&size=` |
| 创建隧道 | `PUT /tunnel`（`application/x-www-form-urlencoded`） |
| 隧道状态 | `GET /tunnel/status?user_id=&tunnel_id=` |
| 隧道配置 | `GET /tunnel/config?user_id=&tunnel_id=&format=TOML` |
| 删除隧道 | `DELETE /tunnel?user_id=&tunnel_id=` |
| FRPC 资源 | `GET /software/frp?os=&arch=` |

Java 使用 `TCP`，Bedrock 使用 `UDP`。远程端口使用 `/node/port` 分配，并校验节点 `port_range`。

## 模块

- `MultiplayerManager`：业务调度、状态机、房间持久化、回滚。
- `LoCyanAuth`：OAuth、token 刷新、系统凭据库。
- `TunnelProvider`：提供商接口；当前实现 `LoCyanFrpProvider`。
- `NodeSelector`：延迟 + 负载 + 节点标记评分。
- `MinecraftDetector`：日志动态端口、进程监听端口、TCP/UDP 可达性。
- `FrpcManager`：官方资源解析、下载、格式校验、启动、日志、停止。
- `TunnelLifecycleManager`：创建、停止、删除、异常恢复。

## 安全边界

- refresh token 与 user id 仅存系统凭据库；access token 只在内存。
- FRPC 配置包含临时凭据，写入 `0600` 文件，停止时删除；日志中的 token/secret/password 行会被隐藏。
- FRPC 下载只接受来自 LoCyanFrp/LoCyan 官方域名或 GitHub 的 HTTPS 地址，并校验可执行文件格式；官方 `/software/frp` 没有 SHA-256 字段，SRL 记录本地 SHA-256，但不能声称与官方哈希比对。
- 删除隧道只允许 `SRL-MP-` 前缀且属于当前账户的隧道；不批量删除用户其他隧道。
- 应用退出时停止 FRPC；异常退出留下的 SRL 隧道会在联机页面显示并可单独清理。

## 已知限制

- 当前开发环境无法解析 `api.locyanfrp.cn`，真实账户下的在线联调尚未完成。
- Bedrock 需要本机已有 UDP 服务监听；SRL 检查端口占用，但不启动 Bedrock 服务端。
- Minecraft 专用接口 `PUT /v3/game/minecraft/game` 标记为测试中，未作为生产依赖。
- 节点地理位置的 API 字段不统一，评分不把地理位置作为硬条件。

## 测试

```bash
cargo test -p sr-core multiplayer::
npm test -- --run
```

覆盖：OAuth 响应解析、用户字段变体、节点评分与 UDP 过滤、端口范围、Minecraft LAN 日志解析、`lsof`/`ss`/`netstat` 解析、FRPC 来源校验、日志脱敏、FRPC 启停生命周期、房间持久化。
