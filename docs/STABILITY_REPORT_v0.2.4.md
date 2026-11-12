# SRL v0.2.4 可靠性修复记录

基线：v0.2.3。以下状态只描述当前代码与已执行测试；真实 Windows、52FRP、Bedrock 未验证的项目明确标为 BLOCKED。

## 修复矩阵

| ID | 状态 | 修改文件 | 方案 | 测试 |
|---|---|---|---|---|
| P1-01 | 已修复并通过回归测试 | `dependency/resolve.rs`, `apply.rs`, 前端类型 | 统一 `source:project:version` 节点 ID；图完整性校验 | 根节点/边 ID、循环、去重 |
| P1-02 | 已修复并通过回归测试 | `dependency/installed.rs`, `resolve.rs`, `ModDependencyDialog.tsx` | 6 种安装状态；仅 enabled+satisfied 满足依赖；前端使用 selectedByDefault | 6 种状态测试 |
| P1-03 | 已修复；故障注入未覆盖全部阶段 | `resources.rs`, `lib.rs` | 持久化事务清单；启动恢复；失败保留备份 | 提交中断恢复测试 |
| P2-01 | 已修复并通过回归测试 | `ModDependencyDialog.tsx`, `resolve.rs` | 默认选择使用 selectedByDefault；可选后代不自动选中 | 解析测试 |
| P2-02 | 已修复但缺少真实网络实测 | `resolve.rs`, `commands.rs`, `api.ts`, 前端 | 4 路并发预取、120 秒 deadline、15 秒超时、请求预算、缓存去重、取消键 | 取消测试、请求去重断言 |
| P2-03 | 已修复并通过回归测试 | `minecraft.rs`, `manager.rs`, `types.rs` | 持久化检查点、增量读取、重启恢复、日志推断标记 | 追加字节、跨午夜、检查点 |
| P2-04 | 已修复并通过回归测试 | `launch.rs` | 32 MiB 分片、5 个历史分片、写入失败诊断 | 轮转测试 |
| P2-05 | 已修复但缺少 Windows 实测 | `manager.rs`, `types.rs` | 保存启动时间；PID + 启动时间校验 | etime 解析测试 |
| P2-06 | 已修复 | `nodes.rs`, `manager.rs`, `Multiplayer.tsx` | 删除未知负载=30%；增加 loadAvailable | 编译/类型检查 |
| P2-07 | 已修复但缺少真实 52FRP 实测 | `manager.rs`, `frp52.rs` | 恢复绝对 10 秒 deadline；分页 20 秒预算 | 代码路径 |
| P2-08 | 已修复但缺少 Windows 实测 | `frpc.rs`, `manager.rs`, `types.rs` | 启动清理本应用遗留 FRPC；lifecycleState | 代码路径 |
| P2-09 | 已修复并通过回归测试 | `minecraft.rs` | RakNet 完整字段校验 | 6 类畸形包测试 |
| P2-10 | 已修复并通过回归测试 | `minecraft.rs` | 相邻事件跨午夜递增日期 | 跨午夜测试 |
| P3-01 | BLOCKED | - | Windows 实机优雅退出/日志轮转未测 | - |
| P3-02 | BLOCKED | - | Windows MoveFileExW 实机未测 | - |
| P3-03 | BLOCKED | - | macOS ARM / Windows 实机未测 | - |
| P3-04 | 部分完成 | `error.rs` 等 | 错误码和脱敏摘要；未统一全部诊断 ID | - |
| P3-05 | 已修复 | `frp52.rs` | HTTP 状态/业务码/限长脱敏摘要 | - |

## 已执行测试

- `cargo fmt --all -- --check` → 通过
- `cargo check --workspace --all-targets` → 通过
- `cargo test -p sr-core --lib` → 254 通过，5 ignored
- `cargo clippy --workspace --all-targets -- -D warnings` → 通过
- `npm test` → 50 通过
- `npx tsc -b` → 通过

## 未执行 / BLOCKED

- Windows 实机：优雅退出、MoveFileExW、孤儿 FRPC、事务恢复、中文路径。
- 真实 52FRP：创建/恢复/断网/429/500/隧道删除。
- 真实 Bedrock：IPv4/IPv6、畸形响应、超时。
- 完整 10 个崩溃注入点；目前只覆盖“committing 中断后恢复”。

这些项目不会在发布说明中标记为 PASS。
