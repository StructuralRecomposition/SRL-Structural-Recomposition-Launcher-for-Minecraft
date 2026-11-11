# SRL 下一阶段实施报告

日期：2026-11-12
基线：`migrate/52frp` @ `1a51f59`，v0.2.1
工作区：`/Users/mac/.openclaw/workspace/sr-launcher`

## 1. 原有功能审计

- Git 工作区在开始时为干净状态，没有覆盖或回滚用户修改；未执行 `git reset --hard` / `git clean -fd`。
- 保留了 52FRP 账户连接、节点评分、端口分配、隧道创建/删除、FRPC 内置/下载/启动/停止/重启、房间持久化、本地 LAN 端口检测。
- 保留了 Java 扫描、版本解析、兼容性算法、Temurin 下载/镜像/SHA-256、下载任务与进度。
- 保留了 Modrinth/CurseForge 搜索、详情、安装、SHA-1 校验、原子写、备份/回滚、资源页 UI。
- 未引入 Redis、Elasticsearch、独立 Node 服务、自建后端、SQLite、图形可视化库或前端状态管理库。

## 2. 实际新增功能

### 2.1 联机管理中心

- 房主 / 游客双模式（同一侧边栏入口）。
- 房主端：
  - 房间状态拆分为 Minecraft 本地服务、52FRP 云端隧道、FRPC 客户端、玩家日志确认状态。
  - 读取 52FRP `GET /tunnels/{id}` 的 `is_online` / `enabled` / `cur_conns` / `status`，20 秒缓存，`cur_conns` 明确标注为 TCP 连接数。
  - 游客成员列表来自真实 Minecraft 日志事件；IP 分类为 loopback / LAN / public / unknown；公网 IP 默认脱敏。
  - 房间创建时间、持续时间、历史与统一日志；JSONL 2 MB 上限、脱敏、筛选、排序、清理。
  - SRL 自身权限模型（Owner、能力矩阵）；游戏内踢出/封禁/白名单/OP 明确不可用。
- 游客端：
  - Rust Server List Ping：地址/DNS/TCP/VarInt/状态 JSON/MOTD/版本/协议/人数/样本/延迟/错误阶段。
  - 可停止的网络监测：12 秒采样，后端 Tokio 任务 + CancellationToken，离开游客模式时停止。
  - 明确区分“网络监测状态”和“游戏内加入状态（无法确认）”。
  - 正版验证显示“无法检测”，正版隔离显示“未知”，并给出原因。

### 2.2 模组依赖可视化与安装

- 统一依赖模型：`required` / `optional` / `incompatible` / `embedded` / `tool` / `include` / `unknown`。
- Modrinth 适配器使用官方版本依赖字段。
- CurseForge 文件模型补齐 `dependencies`，映射 `relationType` 1–6，并保留原始值。
- 递归解析：精确版本优先、按 Minecraft 版本/加载器筛选、稳定版优先、循环/重复/缺失/不兼容处理、深度 16/请求 120/节点 200 上限。
- 已安装检测：读取真实 JAR 元数据（Fabric/Quilt/Forge/NeoForge），CurseForge 使用官方 Murmur2 指纹；不使用文件名或标题相似度。
- 资源详情新增“依赖”对话框：SVG 图形 + 列表、节点详情、已安装/待安装/不兼容、可选依赖勾选、阻止问题、预计大小。
- 安装：服务端保存分析，前端只提交 `analysisId + nodeIds`；复用 `mod_install::apply_files` 的 SHA-1、临时文件、备份、原子替换、失败回滚、任务进度/取消。

### 2.3 Java 自动运行环境

- `java::choose_for_launch`：实例固定 Java（必须兼容）→ 精确 major → 用户默认（兼容时）→ 安全范围内最接近版本。
- 实例固定 Java 不再被自动替换；不兼容时明确报错。
- `java_launch_check`：动态返回 required major、兼容范围、当前选择、可安装性、是否需要重置实例 Java。
- `install_java_for_version`：缺失时安装 Temurin，安装后重新扫描；默认不修改全局默认 Java。
- 首页/实例页启动失败时打开安装提示；安装成功后只继续启动一次。
- Java 环境页手动安装仅在无默认 Java 时设置默认。

## 3. 修改的核心文件

新增：

- `crates/sr-core/src/dependency/{mod,model,resolve,installed,apply}.rs`
- `crates/sr-core/src/multiplayer/{server_status,guest,journal}.rs`
- `src/components/{JavaInstallPrompt,ModDependencyDialog}.tsx`
- `src/lib/dependencyGraph.ts`
- `src/lib/i18n.en.manual4.ts`
- `src/test/dependencyGraph.test.ts`
- `docs/DEPENDENCIES.md`、`docs/JAVA_RUNTIME.md`
- `research/IMPLEMENTATION_PLAN_NEXT_STAGE.md`、本报告

主要修改：

- `crates/sr-core/src/java.rs`、`java_runtime.rs`（未改下载实现）、`mod_install.rs`、`curseforge.rs`
- `crates/sr-core/src/multiplayer/{manager,frp52,minecraft,types,mod}.rs`
- `src-tauri/src/{commands,state,lib}.rs`
- `src/pages/{Multiplayer,Resources,Java,Home,Instances}.tsx`
- `src/lib/api.ts`、`src/lib/i18n.en.ts`、`src/styles.css`
- `README.md`、`README.en.md`、`CHANGELOG.md`、`docs/ARCHITECTURE.md`、`docs/MULTIPLAYER.md`、`research/FEATURE_MATRIX.md`

## 4. 新增 IPC 命令

| 命令 | 作用 |
|---|---|
| `java_launch_check` | 启动前 Java 检查 |
| `install_java_for_version` | 自动补全 Java，默认不改全局默认 |
| `analyze_mod_dependencies` | 保存并返回依赖分析 |
| `install_mod_dependencies` | 按分析 ID + 节点 ID 安装 |
| `multiplayer_members` | 读取并归档可确认玩家事件 |
| `multiplayer_logs` | 读取房间日志 |
| `multiplayer_clear_logs` | 清理房间日志 |
| `multiplayer_guest_ping` | Rust Server List Ping |
| `multiplayer_guest_monitor_start` | 启动游客监测 |
| `multiplayer_guest_monitor_stop` | 停止游客监测 |
| `multiplayer_guest_monitor_status` | 读取监测状态 |

## 5. 新增/扩展数据结构

- `dependency::DependencyNode` / `DependencyEdge` / `DependencyAnalysis` / `DependencyStats` / `PlannedEntry` / `ResolvedItem` / `ResolvedDependency`。
- `dependency::InstallSelection`、`InstalledMod` / `InstalledIndex`。
- `multiplayer::ServerStatusReport` / `ServerStatusError` / `OnlineMode` / `TunnelStateView`。
- `multiplayer::GuestMonitorStatus` / `GuestSample`。
- `multiplayer::MembersView` / `PlayerMember` / `RoomCapabilities` / `RoomLogEntry` / `PlayerEvent`。
- `CurseForgeFile` 增加 `mod_id`、`sha512`、`dependencies`；`CurseForgeDependency` / `CurseForgeRelation`。
- `RoomView` 增加 `local_service_ok`；`MultiplayerStatus` 增加 `tunnel` / `capabilities`。

## 6. UI 变化

- 联机页：房主/游客分段切换；房间卡状态矩阵；成员、历史/日志；游客诊断、监测、手动加入说明。
- 资源页：模组版本行新增“依赖”按钮；依赖对话框含目标实例/版本/加载器、统计、冲突、图形/列表、节点详情、安装进度。
- 首页/实例页：Java 缺失/不兼容时弹出安装提示；显示真实下载进度。
- Java 页：不在已有默认 Java 时覆盖默认。
- 所有新增字符串接入中英 i18n；未硬编码中文到界面。
- 未修改 Logo、主题变量、侧边栏结构或无关页面。

## 7. 测试命令与真实结果

在项目根目录执行：

```bash
cargo fmt --all -- --check          # 通过
cargo test -p sr-core               # 241 项测试通过（239 项库测试 + 2 项启动集成测试）；19 项网络/真实安装在默认运行中 ignored
npx vitest run                      # 7 个测试文件、50 项测试通过
npm run build                       # 通过；仅有 Vite >500 kB chunk 警告
cargo clippy --workspace --all-targets -- -D warnings  # 通过
git diff --check                    # 通过
cargo test -p sr-core --test dependency_network -- --ignored --nocapture
# 通过：真实 Modrinth Iris → Sodium required，2 节点 / 1 边 / 2 文件 / 4,264,786 字节
cargo test -p sr-core --test server_status_network -- --ignored --nocapture
# 通过：mc.hypixel.net:25565 真实 Server List Ping，协议 47，26854/200000，355 ms
```

新增测试重点：

- Server List Ping：地址解析（域名/IPv4/IPv6/端口）、VarInt 往返、MOTD 展开、mock TCP 状态响应、非 Minecraft 服务、连接拒绝、总超时。
- 玩家日志：joined/left/login 行、观测 loopback IP、IP 分类、聊天文本误匹配排除。
- 日志：脱敏、按级别筛选、清理。
- 依赖：required/optional/incompatible、循环、缺失必需依赖、深度限制、CurseForge 1–6、Modrinth 类型、JAR 元数据、已安装匹配。
- Java：8/17/21/25 选择、实例固定、全局默认不兼容、无候选。
- 前端：依赖图分层/空图/边路径。

本次额外实际运行的真实网络测试：

- Modrinth：Iris `zsoi0dso` → Sodium `AANobbMI` required，默认计划 2 个文件、4,264,786 字节。
- Minecraft Server List Ping：`mc.hypixel.net:25565`，返回版本名、协议 47、在线 26906 / 最大 200000、RTT 355 ms。

仍未运行的真实网络/实机测试：

- 真实 52FRP 账户与隧道创建、FRPC 联机。
- 真实 CurseForge 依赖安装（需要可用 API key 与网络条件）。
- 真实 Modrinth 依赖文件下载/安装/回滚（本次只验证分析与计划，未执行下载）。
- 真实缺失 Java 的启动补全（本机已有 Java）。
- Windows/Linux 实机与打包。

## 8. 未完成或被外部条件阻塞

- 52FRP 没有玩家列表、真实玩家 IP、WebSocket 事件或 Minecraft 服务端管理接口；玩家 UUID、真实公网 IP、踢出/封禁/白名单/OP 显示未知或不可用。
- Server List Ping 不公开 `online-mode`；正版验证只能“无法检测”。“正版隔离”在 52FRP OpenAPI 中没有字段，显示“未知”。
- Modrinth 没有按哈希反查已安装项目的公开接口；已安装检测依赖 JAR 声明的 mod id 与项目 slug 一致，否则不判定为已安装（不会猜）。
- 真实 Minecraft 单人 LAN 日志是否总是输出玩家加入/登录行取决于版本与模组；没有事件时界面显示“日志未显示玩家”，不推断。
- 游客状态检测暂不查询 `_minecraft._tcp` SRV；只配置 SRV 的服务器需要填写实际 `host:port`。
- 真实联调与跨平台验证仍需要人工执行。

## 9. 性能设计

- 游客监测单个 Tokio 任务，默认 12 秒，停止时 cancel + abort；离开游客模式停止。
- 52FRP 隧道详情 20 秒缓存；页面状态轮询维持 15 秒；日志 20 秒刷新，均为页面内定时器并在卸载时清理。
- 玩家日志只读最新实例日志尾部 4 MB；JAR 只读元数据条目，条目数/大小上限；CurseForge 指纹最多 400 个 JAR。
- 房间日志 2 MB 上限裁剪至最近 2000 条。
- 依赖解析有深度/请求/节点上限；安装复用现有并发与任务系统。
- 未新增常驻后台高频轮询、无限缓存或大型可视化依赖。

## 10. 安全处理

- 前端不直接联网、不执行系统命令；所有新网络操作在 Rust。
- 依赖安装只接受服务端保存的分析记录中的 URL/哈希；前端不能提交任意下载地址。
- 下载仅 HTTPS + 可信域名白名单；SHA-1 校验；临时文件、备份、原子替换、失败回滚。
- JAR 读取限制大小/条目/元数据字节，拒绝符号链接；文件名校验。
- 房间日志写入前脱敏 token/password/secret/Authorization；IP 默认脱敏、仅本机展示、不写日志。
- 不提供没有真实后端支持的踢出/封禁/OP 按钮。
- 实例固定 Java 不会被自动替换；自动补全不覆盖全局默认。

## 11. 仍需人工实机验证

1. 真实 52FRP Token 下创建/停止/重启房间与隧道详情字段。
2. 真实 Minecraft 局域网世界日志是否产生玩家加入事件与来源地址。
3. 游客对真实 52FRP 公网地址的 Ping/监测结果。
4. 真实 Modrinth/CurseForge 依赖图与安装、回滚。
5. 缺失 Java 的完整“安装 → 重新扫描 → 继续启动”流程。
6. macOS/Windows/Linux 打包与深色/浅色主题视觉检查。
