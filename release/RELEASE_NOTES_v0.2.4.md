<a id="zh"></a>

# SRL v0.2.4 — 可靠性修复

**简体中文** · [English](#en)

> 稳定性维护版本，重点修复依赖图身份、本地模组状态、玩家检查点、日志轮转、进程身份、联机恢复和整合包崩溃恢复。

## 亮点

### 依赖管理

- 节点 ID 统一为 `source:project:version`；根节点、依赖节点、边和未解析节点一致。
- 图生成后强制校验节点 ID 唯一、边端点存在，否则阻止安装。
- 本地模组区分 `enabled_satisfied` / `disabled` / `version_mismatch` / `version_unknown` / `conflict` / `not_installed`；只有启用且版本匹配才算满足依赖。
- 前端默认勾选使用后端 `selectedByDefault`，并显示安装状态。
- 受控 4 路并发预取、15 秒请求超时、120 秒整体 deadline、真实请求计数、缓存去重、可取消分析。

### 玩家与日志

- 持久化 `PlayerCheckpoint`：日志路径、偏移、事件序号、玩家推断状态；增量读取，重启恢复。
- 恢复玩家标记“日志推断”，不再受 4 MiB 尾部窗口限制。
- 跨午夜日志日期递增。
- Minecraft 日志 32 MiB 分片、最多 5 个历史分片，写入失败产生诊断。

### 进程与联机

- 进程身份同时校验 PID 和启动时间，降低 PID 复用误判。
- 启动时清理本应用私有配置目录对应的遗留 FRPC 进程。
- 房间增加 `lifecycleState`：`running` / `paused` / `cleanup_required`。
- 52FRP 恢复使用绝对 10 秒 deadline；隧道分页共享 20 秒预算。
- Java SLP / Bedrock RakNet 严格校验；未知节点负载不再按 30% 计算。

### 文件事务

- 整合包 overrides 使用持久化事务清单，状态覆盖 created/prepared/committing/committed/rolling_back/recovery_required。
- SRL 启动时恢复中断事务；回滚失败保留备份并报告路径。

## 验证

- Rust：254 项库测试 + 启动集成通过；前端 50 项通过。
- `cargo fmt`、`cargo clippy -D warnings`、`cargo check --workspace --all-targets` 通过。
- 新增测试覆盖依赖 ID、安装状态、事务恢复、日志轮转、玩家检查点、PID etime、RakNet、跨午夜、取消。
- macOS / Windows / Linux 资产本地构建；Windows 交叉编译检查通过。
- Linux AppImage Xvfb smoke、macOS DMG 启动 smoke 通过。

## 仍需真实验证

- Windows 实机优雅退出、`MoveFileExW`、事务崩溃恢复。
- 真实 52FRP 账户、Bedrock 服务端、跨平台联机。
- 依赖并发为受控预取；取消在批次之间生效，单请求由 15 秒超时约束。

---

<a id="en"></a>

# SRL v0.2.4 — Reliability Update

[简体中文](#zh) · **English**

> A stability release focused on dependency graph identity, local mod state, player checkpoints, log rotation, process identity, multiplayer recovery and crash-safe modpack transactions.

## Highlights

### Dependency management

- Node IDs are unified as `source:project:version` for roots, dependencies, edges and unresolved nodes.
- The graph is validated after generation: duplicate IDs or dangling edges block installation.
- Local mods distinguish `enabled_satisfied` / `disabled` / `version_mismatch` / `version_unknown` / `conflict` / `not_installed`; only an enabled, matching version satisfies a dependency.
- The frontend defaults to backend `selectedByDefault` and shows the installed state.
- Bounded 4-way prefetch, 15-second request timeout, 120-second overall deadline, real request counting, cache de-duplication and cancellable analysis.

### Players and logs

- Persisted `PlayerCheckpoint` stores log path, offset, event sequence and inferred player state; reads only appended bytes and survives restarts.
- Restored players are labelled “inferred from log” and are no longer limited by the 4 MiB tail window.
- Midnight log rollover increments the event date.
- Minecraft logs rotate at 32 MiB with up to five shards; write failures produce diagnostics without blocking stdout/stderr.

### Processes and multiplayer

- Process identity checks both PID and start time to reduce PID-reuse mistakes.
- Startup cleans up SRL-owned orphan FRPC processes by matching this app's private config directory.
- Rooms expose `lifecycleState`: `running` / `paused` / `cleanup_required`.
- 52FRP recovery uses an absolute 10-second deadline; tunnel pagination shares a 20-second budget.
- Strict Java SLP / Bedrock RakNet validation; unknown node load is no longer treated as 30%.

### File transactions

- Modpack overrides use a persistent transaction manifest with created/prepared/committing/committed/rolling_back/recovery_required states.
- SRL recovers interrupted transactions on startup; failed rollback keeps the backup and reports its path.

## Verification

- Rust: 253 library tests + startup integration passed; frontend 50 passed.
- `cargo fmt`, `cargo clippy -D warnings`, `cargo check --workspace --all-targets` passed.
- New tests cover dependency identity, installed states, transaction recovery, log rotation, player checkpoints, PID etime, RakNet, midnight rollover and cancellation.
- macOS / Windows / Linux assets built locally; Windows cross-check passed.
- Linux AppImage Xvfb smoke and macOS DMG launch smoke passed.

## Still requires real-world verification

- Windows graceful exit, `MoveFileExW` and transaction crash recovery on a real device.
- A real 52FRP account, Bedrock server and cross-platform multiplayer.
- Dependency concurrency is bounded prefetch; cancellation takes effect between batches and each request is capped at 15 seconds.
