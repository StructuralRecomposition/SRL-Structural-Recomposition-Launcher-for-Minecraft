<a id="zh"></a>

# SRL v0.2.3 — 稳定性与可靠性修复

**简体中文** · [English](#en)

> 这是一个稳定性维护版本，保留 v0.2.2 的功能与界面，重点修复实例数据安全、整合包事务、联机状态准确性、依赖解析和发布工程问题。

## 亮点

### 安全与数据完整性

- 实例启动、停止、删除、复制、配置修改与资源导入共用后端生命周期锁；运行中的实例不能删除或复制，并发启动只允许一个进程。
- 整合包 overrides 改为“校验 → 暂存 → 备份 → 提交”事务；失败时回滚已提交文件，回滚不完整时保留备份并报告路径。
- 解压前检查目标目录已有符号链接，拒绝写出实例目录；路径校验统一拒绝盘符、UNC、ADS、冒号、尾随点/空格和 Windows 保留名。
- Windows 原子写入改用 `MoveFileExW`，替换失败不会先毁掉旧文件。
- 房间日志并发写入串行化，按 2 MB 字节上限裁剪。

### 实例与进程

- Windows 停止游戏先请求优雅关闭并等待 10 秒，超时后由界面询问是否强制结束，并提示未保存进度可能丢失。
- 前端在实例运行或状态不确定时禁用危险按钮，后端仍执行最终校验。

### 联机状态

- 本地服务改用 Java Server List Ping 与 Bedrock RakNet 检测；普通端口占用不再被当成 Minecraft 服务。
- FRPC 进程、云端隧道在线状态和本地协议健康分别显示；FRPC 存活但隧道离线/未知时不再显示“联机在线”。
- 52FRP 隧道列表读取全部分页；响应缺少 `list` 时返回错误而不是空列表。
- 重启 FRPC 后轮询云端隧道，10 秒内未确认则明确报错。
- 端口检查区分网络/API 错误与真实占用，并设置整体预算。
- SRL 日志的 `[stdout]` / `[stderr]` 前缀在解析前剥离，玩家事件时间戳恢复；去重集合有界，不再在 5,000 条后停止处理。
- 房间状态轮询增加单飞保护与指数退避。

### 依赖与 Java

- 后端独立校验必需依赖闭包；漏选必需依赖会被拒绝。
- 同一节点后续以不兼容关系出现时仍产生 blocker。
- 依赖节点 ID 包含具体版本；不同固定版本不再直接合并。
- 未选中可选依赖的必需子依赖不再自动勾选。
- Java 自动选择增加 CPU 架构校验；已知架构不匹配时不会自动选用。

### 发布工程

- 发布工作流增加 fmt、test、clippy、前端测试/构建、cargo audit、npm audit 门禁。
- 校验 tag 与应用版本一致；拒绝覆盖已有 Release；手动发布默认值不再硬编码旧版本。

## 验证

- Rust：库测试与启动集成测试通过（详见仓库测试记录）。
- 前端：Vitest 通过；`tsc -b` 与生产构建通过。
- `cargo fmt --check`、`cargo clippy --workspace --all-targets -D warnings` 通过。
- macOS universal DMG、Windows x64 便携包、Linux deb / AppImage 本地构建。

## 仍需真实验证

- Windows 优雅退出与 `MoveFileExW` 原子替换尚未在真实 Windows 设备复测。
- 整合包回滚的中途进程崩溃恢复、磁盘满/权限拒绝场景尚未实测。
- 依赖解析仍以串行请求为主，受控并发与整体取消尚未完成。
- 玩家在线状态仍从日志尾部重建，尚未实现持久化增量检查点。
- Minecraft 会话日志尚未实现运行时轮转；PID 复用识别和节点负载不可用标记仍待完善。
- 真实 52FRP 账户、Bedrock 服务器和跨平台联机仍需实机验证。

---

<a id="en"></a>

# SRL v0.2.3 — Stability & Reliability Update

[简体中文](#zh) · **English**

> A maintenance release that keeps the v0.2.2 features and UI, focused on instance data safety, modpack transactions, multiplayer status accuracy, dependency correctness and release engineering.

## Highlights

### Security and data integrity

- Launch, stop, delete, duplicate, reconfiguration and resource import share a backend lifecycle lock. Running instances cannot be deleted or copied, and concurrent launches create only one process.
- Modpack overrides use validate → stage → backup → commit. A failed commit rolls back already-written files; an incomplete rollback keeps the backup directory and reports its path.
- Extraction checks existing target-directory symlinks and refuses writes outside the instance; path validation rejects drive prefixes, UNC, ADS, colons, trailing dots/spaces and Windows reserved names.
- Windows atomic writes use `MoveFileExW`, so a failed replacement never deletes the old file first.
- Room journal writes are serialized and trimmed by a strict 2 MB byte limit.

### Process lifecycle

- On Windows, stopping a game first requests a graceful close and waits 10 seconds. The UI then asks before a force stop and warns that unsaved progress may be lost.
- The UI disables destructive actions while an instance is running or uncertain; the backend still enforces the final check.

### Multiplayer accuracy

- Local services are verified with Java Server List Ping and Bedrock RakNet; an occupied port is no longer treated as a healthy Minecraft server.
- FRPC process state, cloud tunnel state and local protocol health are shown separately. A live FRPC process with an offline/unknown tunnel is not reported as online.
- The 52FRP tunnel list reads every page; a missing `list` field is an error rather than an empty list.
- After restarting FRPC, the tunnel is polled and an explicit error is returned if it is not confirmed within 10 seconds.
- Port checks distinguish network/API errors from real occupation and use an overall budget.
- SRL `[stdout]` / `[stderr]` prefixes are stripped before log parsing, restoring player-event timestamps. De-duplication is bounded and no longer stops at 5,000 events.
- Room status polling has single-flight protection and exponential backoff.

### Dependencies and Java

- The backend independently verifies the required-dependency closure; incomplete selections are rejected.
- Incompatible relations remain blockers even when the same node was seen earlier.
- Dependency node IDs include the concrete version; different pinned versions are no longer merged directly.
- Required children of an unselected optional dependency are no longer auto-selected.
- Java auto-selection validates CPU architecture and will not silently choose a known-incompatible runtime.

### Release engineering

- Releases are gated by fmt, tests, clippy, frontend test/build, cargo audit and npm audit.
- The workflow verifies tag/application version consistency and refuses to overwrite an existing Release.

## Verification

- Rust library and startup integration tests pass (see repository test record).
- Frontend Vitest passes; `tsc -b` and the production build pass.
- `cargo fmt --check` and `cargo clippy --workspace --all-targets -D warnings` pass.
- macOS universal DMG, Windows x64 portable ZIP, Linux deb / AppImage were built locally.

## Still requires real-world verification

- Windows graceful exit and `MoveFileExW` replacement have not been retested on a real Windows device.
- Modpack crash recovery mid-transaction, disk-full and permission-denied scenarios have not been exercised.
- Dependency resolution is still mostly serial; controlled concurrency and whole-operation cancellation are not complete.
- Player presence is still rebuilt from the log tail; a persisted incremental checkpoint is not implemented.
- Minecraft session logs do not rotate at runtime yet; PID reuse detection and node-load unavailable markers still need work.
- A real 52FRP account, Bedrock server and cross-platform multiplayer still need real-device verification.
