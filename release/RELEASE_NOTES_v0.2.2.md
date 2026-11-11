# SRL v0.2.2 — 联机管理中心 · 依赖可视化 · Java 自动补全

> 这是 v0.2.1 之后的下一阶段功能更新，当前为**本地测试版**，尚未发布到 GitHub Releases。

## 亮点

### 1. Minecraft 联机管理中心

- 联机页新增房主 / 游客分段模式，保留原 52FRP 账户、节点、创建、停止、重启与临时隧道清理流程。
- 房主房间卡分开显示：Minecraft 本地服务端口、52FRP 云端隧道在线状态、FRPC 客户端状态、玩家日志确认状态。
- 通过 52FRP `GET /tunnels/{id}` 读取 `is_online` / `enabled` / `cur_conns` / `status`；`cur_conns` 只表示 TCP 连接数，不推断玩家人数。
- 游客成员只解析 SRL 捕获日志中真实的 `joined the game` / `left the game` / `logged in with entity id` 事件。
- 观测 IP 区分 loopback / LAN / public / unknown；公网地址默认脱敏，可手动显示。
- 新增 `multiplayer/room-log.jsonl` 房间日志（2 MB 上限裁剪，写入前脱敏），支持筛选、排序与清理。
- 游客模式使用 Rust 实现 Minecraft Server List Ping：地址解析、DNS、TCP、VarInt、状态 JSON、MOTD、版本、协议、在线人数、玩家样本、RTT 与分阶段错误。
- 游客网络监测每 12 秒采样（10–60 秒可调），可随时停止并取消后台任务；明确区分“网络监测状态”和“游戏内加入状态（无法确认）”。
- 正版验证 / 正版隔离保持“未知 / 无法检测”：SLP 不公开 `online-mode`，52FRP API 没有该字段。

### 2. 模组依赖关系可视化

- 新增统一依赖模型与 Modrinth / CurseForge 适配器，支持递归解析、已安装检测与安装执行。
- Modrinth 识别 `required` / `optional` / `incompatible` / `embedded`；CurseForge 识别 `relationType` 1–6。
- 从 JAR 内 `fabric.mod.json`、`quilt.mod.json`、`META-INF/mods.toml`、`META-INF/neoforge.mods.toml` 读取真实 mod id；CurseForge 使用 Murmur2 指纹。
- 资源详情新增“依赖”入口：SVG 关系图 + 列表视图、节点详情、已安装 / 待安装 / 不兼容状态、可选依赖勾选、预计下载大小。
- 分析结果保存到 `cache/dependencies/<id>.json`；安装时前端只提交 `analysisId + nodeIds`，后端重新校验实例、加载器、Minecraft 版本、必需依赖与不兼容关系。
- 安装复用 `mod_install::apply_files`：SHA-1 校验、临时文件、同名备份、原子替换、失败回滚、进度与取消。

### 3. Java 自动运行环境

- 新增 `java::choose_for_launch`：实例固定 Java → 精确 major → 用户默认（兼容时）→ 安全范围内最接近版本。
- 实例显式指定的 Java 不会被静默替换；不兼容时返回明确错误并提供改回自动选择的路径。
- 新增 `java_launch_check`：返回 required major、兼容范围、当前选择、是否可安装、是否需要重置实例 Java。
- 新增 `install_java_for_version`：缺失时下载安装 Temurin，安装后重新扫描；自动补全不修改全局默认 Java。
- 首页 / 实例页在启动返回 `JAVA_INVALID` / `JAVA_INCOMPATIBLE` 时打开安装提示，安装成功后只继续启动一次。

## 验证

- Rust：241 项通过（239 库测试 + 2 启动集成），19 项真实网络 / 下载测试默认忽略。
- 前端：50 项测试通过（Vitest + Testing Library），含依赖图分层布局与空图。
- `cargo fmt --check`、`tsc -b`、`npm run build` 通过；macOS universal DMG 构建成功。
- 真实网络验证：Modrinth Iris → Sodium required（2 节点 / 2 文件 / 4,264,786 字节）；`mc.hypixel.net:25565` Server List Ping 返回协议 47、26854/200000、355 ms。

## 安装包

当前本地已生成：

| 平台 | 文件 |
|---|---|
| macOS 10.15+ | `SRL_0.2.2_universal.dmg`（Intel + Apple Silicon） |

Windows 便携 ZIP、Linux deb / AppImage 需由 GitHub Actions 三平台矩阵构建；本次为本地测试版，未发布 Release。

## 已知限制

- Microsoft 正版登录仍需自行注册 Azure 应用并填写 `client id`。
- 玩家 UUID、真实公网 IP、游戏内 OP / 踢出 / 封禁 / 白名单在没有可靠来源时显示未知。
- 依赖安装与联机管理中心的真实网络 / 真实账户联调仍需实机验证。
