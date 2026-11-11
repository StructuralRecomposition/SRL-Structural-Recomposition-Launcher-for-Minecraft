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

## 下一阶段：房主 / 游客双模式（0.3 开发线）

联机页面拆分为两个模式，但共用同一个侧边栏入口：

- **房主模式**：保留 52FRP 账户、节点、创建/停止/重启 FRPC、临时隧道清理。
- **游客模式**：只做服务器连接诊断和网络监测，不代替 Minecraft 加入游戏。

### 房主状态必须分开

界面分别显示四个真实状态：

| 状态 | 数据来源 | 不代表 |
|---|---|---|
| Minecraft 本地服务 | 本地端口 TCP 探测（Java）/ UDP 占用（Bedrock） | 不代表公网可达 |
| 52FRP 云端隧道 | `GET /tunnels/{id}` 的 `is_online` | 不代表 Minecraft 本地服务正常 |
| FRPC 客户端 | SRL 启动的子进程状态 | 不代表隧道已在线 |
| 玩家已进入游戏 | Minecraft 游戏日志中可确认的玩家事件 | 没有日志时显示“无法确认” |

`GET /tunnels/{id}` 的 `cur_conns` 只作为“当前 TCP 连接数”展示，并明确标注“不等于玩家人数”。

### 游客成员与 IP 隐私

成员列表只读取 SRL 捕获的 Minecraft 实例日志：

- `开始服务` / `joined the game` / `left the game` / `logged in with entity id` 等行。
- 只有日志里真实出现的玩家名和事件时间才会写入。
- 玩家 UUID 没有可靠来源时不显示。
- 52FRP 的 TCP 转发会让服务端通常只看到 `127.0.0.1`；此时标记为“本地代理连接”，不会显示成游客公网 IP。
- 观测到的私网地址标记为“局域网地址”，公网地址标记为“公网地址”，没有地址时显示“连接来源 IP：不可获取”。
- 默认对公网地址脱敏；房主可点击“显示完整 IP”。IP 只出现在房主本机界面，不上传第三方，不写入日志。

SRL 自身角色模型为 `Owner` / `Admin` / `Member` / `Guest`。当前本机房主固定为 `Owner`，从游戏日志识别出的玩家为 `Guest`；`Admin` / `Member` 预留给未来的 SRL 房间协作，不冒充 Minecraft OP。

成员事件是**部分可用**能力：Minecraft 单人世界“对局域网开放”没有远程管理通道，SRL 不能踢出、封禁、设置白名单或 OP。房主模式提供 SRL 自身的权限模型占位（`Owner`），但不会显示没有实现的游戏内管理按钮。

### 房间历史与日志

- 追加写入 `<app data>/multiplayer/room-log.jsonl`，不是独立数据库。
- 记录房间创建/关闭、FRPC 启停/重启、隧道删除、网络错误、本地服务停止、可确认的玩家加入/离开。
- 每条包含 `timestamp`、`source`、`level`、`event_type`、`message`、可选的 `tunnel_id`。
- 读取日志时会合并当前 FRPC 日志环形缓冲（`source=frpc`），不重复写入磁盘。
- 写入前调用 `redact_secrets`，隐藏 token/password/secret/Authorization 等值。
- 文件超过 2 MB 时裁剪为最近 2000 条；UI 支持按级别筛选、时间排序和清理。

### 游客连接诊断

全部使用 Rust 网络层（`crates/sr-core/src/multiplayer/server_status.rs`）：

1. 解析 `host`、`host:port`、IPv4、`[IPv6]:port`、裸 IPv6，无端口使用 25565。
2. DNS 解析（A/AAAA；超时 6 秒）。
3. 逐个地址 TCP 连接（总预算 12 秒）。
4. 发送 Minecraft Java Edition Server List Ping 握手（协议版本 -1，next state 1）。
5. 读取状态响应；VarInt 最多 5 字节，包体上限 4 MB，字符串上限 2 MB。
6. 解析 MOTD、版本名、协议号、在线/最大人数、公开玩家样本、图标。
7. 计算状态 RTT，区分地址、DNS、TCP、握手、协议、超时、连接关闭、响应错误。

检测成功只证明“服务器返回了 Minecraft 状态”，**不证明游戏客户端已经加入**。

实网验证（2026-11-12）：`mc.hypixel.net:25565` 返回协议 47、版本名、在线 26854 / 最大 200000、状态 RTT 355 ms。

### 正版验证与正版隔离

- Server List Ping 不公开 `online-mode`，SRL 显示“无法检测”，不会用 MOTD 或人数推断。
- 52FRP 开放 API（`/api/v1/openapi.json`）没有“正版隔离”字段，SRL 显示“未知”，不会把隧道在线或 FRPC 运行当作正版隔离结果。

> DNS 目前解析 A/AAAA 记录，不查询 `_minecraft._tcp` SRV。只配置了 SRV 的服务器需要填写实际的 `host:port`。
- 两个字段分开显示，避免把“正版验证”和“正版隔离”当成同一个概念。

### 游客监测

- 连接检测通过后可以启动网络监测，默认每 12 秒采样一次（10–60 秒范围）。
- 采样由 Rust 后台任务执行；停止监测立即取消并 abort 任务。
- 界面区分“网络监测状态”和“游戏内加入状态”；后者始终显示“无法确认”，除非有可靠游戏侧证据。
- 离开游客模式时前端调用停止接口，不保留永久轮询。

### 52FRP 能力边界

官方 API 没有玩家列表、玩家真实 IP、WebSocket 事件或 Minecraft 服务端控制接口。SRL 不会：
- 根据 `cur_conns` 推断玩家人数；
- 把中继节点 IP 当作游客 IP；
- 伪造“已加入世界”；
- 提供没有真实后端支持的踢出/封禁/OP 按钮。
