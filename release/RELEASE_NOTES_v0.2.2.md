<a id="zh"></a>

# SRL v0.2.2 — 联机管理中心 · 依赖可视化 · Java 自动补全

**简体中文** · [English](#en)

> v0.2.1 之后的功能更新。本次重点是把「Minecraft 联机」从创建隧道扩展为可诊断、可观测的联机管理中心，并补齐模组依赖关系与 Java 运行环境。

## 亮点

### 1. Minecraft 联机管理中心

- 联机页新增房主 / 游客分段模式，保留原 52FRP 账户、节点、创建、停止、重启与临时隧道清理流程。
- 房主房间卡分开显示：Minecraft 本地服务端口、52FRP 云端隧道在线状态、FRPC 客户端状态、玩家日志确认状态；四者不再混为一个「在线」标签。
- 通过 52FRP `GET /tunnels/{id}` 读取 `is_online` / `enabled` / `cur_conns` / `status`；`cur_conns` 只表示 TCP 连接数，不推断玩家人数。
- 游客成员只解析 SRL 捕获日志中真实的 `joined the game` / `left the game` / `logged in with entity id` 事件，只接受房间创建后的事件。
- 观测 IP 区分 loopback / LAN / public / unknown；`127.0.0.1` 显示为「本地代理连接」，公网地址默认脱敏，可手动显示。
- 新增 `multiplayer/room-log.jsonl` 房间日志（2 MB 上限裁剪，写入前脱敏），支持筛选、时间排序与清理。
- 游客模式使用 Rust 实现 Minecraft Server List Ping：地址解析、DNS、TCP、VarInt、状态 JSON、MOTD、版本、协议、在线人数、玩家样本、RTT 与分阶段错误。
- **正版验证探测**：状态查询后额外执行一次登录握手（不发送账号凭据、不进入游戏）。服务器返回加密请求 → `online-mode=true`，离线账号无法加入；直接返回登录成功 → `online-mode=false`，离线账号可以加入；无法确认时显示未知并说明原因。正版隔离仍显示未知（52FRP API 无该字段）。
- 游客网络监测每 12 秒采样（10–60 秒可调），可随时停止并取消后台任务；监测卡片只在连接诊断完成后显示，并明确区分「网络监测状态」和「游戏内加入状态（无法确认）」。

### 2. 模组依赖关系可视化

- 新增统一依赖模型与 Modrinth / CurseForge 适配器，支持递归解析、已安装检测与安装执行。
- Modrinth 识别 `required` / `optional` / `incompatible` / `embedded`；CurseForge 识别 `relationType` 1–6。
- 从 JAR 内 `fabric.mod.json`、`quilt.mod.json`、`META-INF/mods.toml`、`META-INF/neoforge.mods.toml` 读取真实 mod id；CurseForge 使用 Murmur2 指纹，不使用文件名或标题相似度。
- 资源详情新增「依赖」入口：SVG 关系图 + 列表视图、节点详情、已安装 / 待安装 / 不兼容状态、可选依赖勾选、预计下载大小。
- 分析结果保存到 `cache/dependencies/<id>.json`；安装时前端只提交 `analysisId + nodeIds`，后端重新校验实例、加载器、Minecraft 版本、必需依赖与不兼容关系。
- 安装复用 `mod_install::apply_files`：SHA-1 校验、临时文件、同名备份、原子替换、失败回滚、进度与取消。

### 3. Java 自动运行环境

- 新增 `java::choose_for_launch`：实例固定 Java → 精确 major → 用户默认（兼容时）→ 安全范围内最接近版本。
- 实例显式指定的 Java 不会被静默替换；不兼容时返回明确错误并提供改回自动选择的路径。
- 新增 `java_launch_check`：返回 required major、兼容范围、当前选择、是否可安装、是否需要重置实例 Java。
- 新增 `install_java_for_version`：缺失时下载安装 Temurin，安装后重新扫描；自动补全不修改全局默认 Java。
- 首页 / 实例页在启动返回 `JAVA_INVALID` / `JAVA_INCOMPATIBLE` 时打开安装提示，安装成功后只继续启动一次。

## 验证

- Rust：242 项库测试 + 2 项启动集成通过；真实网络 / 下载测试默认忽略。
- 前端：50 项测试通过（Vitest + Testing Library），含依赖图分层布局与空图。
- `cargo fmt --check`、`cargo clippy --workspace --all-targets -D warnings`、`tsc -b`、`npm run build` 通过。
- macOS universal DMG、Windows x64 便携包、Linux deb / AppImage 均本地构建成功；macOS DMG 挂载启动 smoke、Linux AppImage Xvfb smoke 通过。
- 真实网络验证：Modrinth Iris → Sodium required（2 节点 / 2 文件 / 4,264,786 字节）；`mc.hypixel.net:25565` Server List Ping 返回协议 47、26854/200000、355 ms。

## 安装包

| 平台 | 文件 |
|---|---|
| macOS 10.15+ | `SRL_0.2.2_universal.dmg`（Intel + Apple Silicon，未签名、未公证） |
| Windows 10/11 x64 | `SRL_0.2.2_windows-x64-portable.zip` |
| Linux x64（Debian / Ubuntu） | `SRL_0.2.2_amd64.deb` |
| Linux x64（通用） | `SRL_0.2.2_amd64.AppImage` |

校验值见 `SHA256SUMS`。

## 已知限制

- Microsoft 正版登录仍需自行注册 Azure 应用并填写 `client id`。
- 玩家 UUID、真实公网 IP、游戏内 OP / 踢出 / 封禁 / 白名单在没有可靠来源时显示未知。
- 依赖安装与联机管理中心的真实网络 / 真实账户联调仍需实机验证。

---

<a id="en"></a>

# SRL v0.2.2 — Multiplayer Management Center · Dependency Visualization · Java Auto-Install

[简体中文](#zh) · **English**

> A feature update after v0.2.1. It turns Minecraft multiplayer from tunnel creation into a diagnosable management center, and completes mod dependency analysis and Java runtime handling.

## Highlights

### 1. Minecraft multiplayer management center

- Host and guest modes share one entry; the existing 52FRP account, node, create, stop, restart and temporary-tunnel cleanup flows are preserved.
- The host room card separates the local Minecraft service port, 52FRP tunnel state, FRPC client state, and confirmed player-log state instead of merging them into one "online" badge.
- Reads `is_online` / `enabled` / `cur_conns` / `status` from `GET /tunnels/{id}`; `cur_conns` is documented as TCP connections, not a player count.
- Guest members are derived only from real `joined the game` / `left the game` / `logged in with entity id` events in logs captured by SRL, and only after the room was created.
- Observed IPs are classified as loopback / LAN / public / unknown; `127.0.0.1` is shown as a local proxy connection, and public addresses are redacted until revealed.
- Adds `multiplayer/room-log.jsonl` with a 2 MB cap and redaction before writing; the UI supports filtering, sorting and cleanup.
- Guest mode implements Minecraft Server List Ping in Rust: address parsing, DNS, TCP, VarInt, status JSON, MOTD, version, protocol, player counts, sample players, RTT and staged errors.
- **Premium authentication probe**: after the status query, a login handshake is performed without sending credentials or joining the game. An Encryption Request means `online-mode=true` (offline accounts cannot join); a Login Success means `online-mode=false` (offline accounts can join); otherwise the result is unknown with a reason. Premium isolation remains unknown because the 52FRP API does not expose it.
- The guest monitor samples every 12 seconds (10–60 adjustable), can be stopped and cancels its background task. The monitor card appears only after diagnostics complete and keeps network state separate from the unverifiable in-game join state.

### 2. Mod dependency visualization

- Adds a unified dependency model with Modrinth / CurseForge adapters, recursive resolution, installed detection and installation.
- Modrinth relations: `required` / `optional` / `incompatible` / `embedded`; CurseForge: `relationType` 1–6.
- Reads real mod ids from `fabric.mod.json`, `quilt.mod.json`, `META-INF/mods.toml` and `META-INF/neoforge.mods.toml`; CurseForge uses the Murmur2 fingerprint instead of filename or title similarity.
- The resource detail page gains a Dependencies entry: SVG graph plus list view, node details, installed / pending / incompatible states, optional dependency selection and estimated download size.
- Analysis is stored in `cache/dependencies/<id>.json`; installation submits only `analysisId + nodeIds`, and the backend re-validates the instance, loader, Minecraft version, required dependencies and conflicts.
- Installation reuses `mod_install::apply_files`: SHA-1 verification, temporary files, backups, atomic replacement, rollback, progress and cancellation.

### 3. Automatic Java runtime

- Adds `java::choose_for_launch`: pinned instance Java → exact major → compatible user default → closest safe version.
- An explicitly pinned instance Java is never silently replaced; incompatibility returns a clear error and a path back to automatic selection.
- Adds `java_launch_check` with required major, compatible range, current choice, installability and whether the instance Java needs resetting.
- Adds `install_java_for_version`: installs Temurin when missing, rescans afterwards, and never changes the global default during auto-completion.
- Home and instance pages open the install prompt on `JAVA_INVALID` / `JAVA_INCOMPATIBLE`, and continue the launch only once after a successful install.

## Verification

- Rust: 242 library tests + 2 startup integration tests passed; real-network/download tests are ignored by default.
- Frontend: 50 tests passed (Vitest + Testing Library), including dependency graph layout and empty graph.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `tsc -b` and `npm run build` passed.
- macOS universal DMG, Windows x64 portable ZIP, Linux deb / AppImage were built locally; macOS DMG launch smoke and Linux AppImage Xvfb smoke passed.
- Real-network checks: Modrinth Iris → Sodium required (2 nodes / 2 files / 4,264,786 bytes); `mc.hypixel.net:25565` status returned protocol 47, 26854/200000, 355 ms.

## Installers

| Platform | File |
|---|---|
| macOS 10.15+ | `SRL_0.2.2_universal.dmg` (Intel + Apple Silicon, unsigned, not notarized) |
| Windows 10/11 x64 | `SRL_0.2.2_windows-x64-portable.zip` |
| Linux x64 (Debian / Ubuntu) | `SRL_0.2.2_amd64.deb` |
| Linux x64 (universal) | `SRL_0.2.2_amd64.AppImage` |

Checksums are in `SHA256SUMS`.

## Known limitations

- Microsoft sign-in still requires your own Azure application and `client id`.
- Player UUIDs, real public IPs, in-game OP / kick / ban / whitelist are shown as unknown when no reliable source exists.
- Dependency installation and real-account multiplayer flows still need real-world verification.
