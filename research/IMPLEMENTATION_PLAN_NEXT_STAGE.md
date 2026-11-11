# 下一阶段实施计划（实际执行记录）

日期：2026-11-12
基线：`migrate/52frp` @ `1a51f59`（v0.2.1）

## 1. 基线审计

- `git status --short`：干净，无未提交修改。
- `cargo test -p sr-core`：基线通过（当时 234 项库测试 + 集成测试）。
- 现有 52FRP 房间创建链：`create_room → create_inner → provider.account → detect_local → nodes → node → quota → port → tunnel → config → frpc.prepare/start → tunnel_online`。
- FRPC：`FrpcManager` 负责内置/缓存/下载、SHA-256、启动、日志环形缓冲、停止与重启。
- 本地服务：`minecraft::detect_local` 从 SRL 实例日志与进程监听端口识别动态 LAN 端口。
- Java：`java::select_for_launch` 已读取元数据 `javaVersion.majorVersion`；`install_java_runtime` 会无条件写全局默认（本次修正自动补全路径）。
- 依赖：`mod_install` 已有 Modrinth required 递归、SHA-1、备份、回滚；CurseForge 文件模型缺少依赖字段（本次补齐）。

## 2. 实际实施顺序

1. Java：新增 `choose_for_launch`、`java_launch_check`、`install_java_for_version`，保持旧安装命令兼容；前端新增启动前安装提示。
2. 依赖：新增 `dependency` 模块，扩展 CurseForge 文件依赖；复用 `mod_install::apply_files`。
3. 联机：新增 Server List Ping、游客监测、玩家日志事件、JSONL 日志；扩展 `MultiplayerStatus` / `RoomView`；前端房主/游客模式。
4. 测试、格式化、clippy、前端构建与文档。

## 3. 复用与未重写

- 52FRP API、令牌 keyring、FRPC 生命周期、房间持久化、节点评分、端口分配、本地服务检测全部保留。
- 下载任务、SHA-1 校验、原子写、备份/回滚复用。
- 主题、侧边栏、卡片、Badge、Toast、i18n 体系复用；未新增图形库、状态管理库或数据库。

## 4. 明确阻塞/限制

- 无真实 52FRP Token，无法完成真实账户与隧道联调；HTTP 解析按官方 OpenAPI 实现。
- 52FRP API 不提供玩家列表、真实玩家 IP、WebSocket 事件或 Minecraft 服务端管理接口。
- Minecraft 单人世界“对局域网开放”没有远程管理通道；踢出/封禁/白名单/OP 不提供。
- Server List Ping 不公开 `online-mode`；52FRP API 无“正版隔离”字段。
- 真实 Modrinth/CurseForge 安装需网络与 API key；本地测试覆盖解析与安全路径，不伪造在线成功。
