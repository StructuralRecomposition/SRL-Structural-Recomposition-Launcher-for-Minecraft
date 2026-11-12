<a id="zh"></a>

# SRL v0.2.5 — 稳定性、性能与 Bug 修复

**简体中文** · [English](#en)

> 工程收尾版本。不新增功能，重点修复日志采集、任务取消、文件事务与 Windows 文件替换中的可靠性问题。

## 修复

### 日志与玩家状态

- 游戏 stdout/stderr 改为按字节读取、按需有界截断，并使用有损 UTF-8 解码。中文 Windows 下 Java 的 GBK 控制台输出不再触发 `InvalidData`，也不会再让后续日志全部丢失。
- 玩家检查点改为按实际读取字节推进偏移；定位到多字节字符或半行时跳过残片，避免重复/截断事件。
- 捕获日志切换到新会话时，上一会话中“推断在线”的玩家会被降级为离线，消除跨会话幽灵玩家。

### 取消与超时

- 依赖分析的取消现在会直接中断在途 HTTP 请求；120 秒整体截止时间会终止整个分析，而不只是阻止下一次请求。
- 下载在等待响应头阶段即可取消，`Retry-After` 等待同样可取消；新增 60 秒无数据停滞超时，避免连接长时间挂起。

### 文件安全

- 通用 ZIP 整合包导入改用与 overrides 相同的事务：完整校验 → 暂存解压 → 备份现有文件 → 提交；失败自动回滚。导入失败不再直接覆盖用户原有文件。
- Windows 的下载替换、归档导出、模组/材质包/光影导入和 FRPC 释放统一使用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`；重命名失败不再先删除旧文件。
- 整合包事务清单改为临时文件 + fsync + 原子替换；恢复时重新校验相对路径、事务目录名和备份完整性，发现异常保留恢复材料并报错。
- 解压同时校验实际写出字节与压缩比，限制伪造头部的 ZIP 炸弹。

### 日志轮转

- 轮转重命名失败时保留真实字节计数，改为 8 MiB 后重试，不再每行重试，也不会错误归零后让日志无限增长。
- 单个日志行超过 256 KiB 时有界截断并标记，避免异常输出占用无界内存。

## 性能与资源

- FRPC 遗留进程清理移出构造函数，启动时一次性在阻塞线程执行；不再在账户连接路径阻塞异步运行时 800 ms。
- FRPC 二进制校验改为流式 SHA-256，不再每次创建房间读取完整 20–40 MB 文件。
- 下载目标锁在最后一个使用者释放后移除，长期运行不再累计路径锁条目。
- 前端游戏日志按 200 ms 批量刷新并限制 400 条缓冲，避免每条日志触发一次全局 React 更新；模组列表启用 `content-visibility` 跳过屏外绘制。
- 基线（Intel i9-9880H / macOS 26.7 / release）：50,000 行日志解析 33.4 ms，增量读取 52.9 ms，100 个实例扫描 5.7 ms。

## 验证

- Rust：264 项单元测试 + 2 项启动集成测试通过。
- 前端：50 项测试通过，`tsc -b` 与 `vite build` 通过。
- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通过。
- 新增测试覆盖非 UTF-8 日志、字节偏移、跨会话玩家状态、轮转失败、取消中断在途请求、下载取消、目标锁释放和事务路径校验。

## 仍需实机验证

- Windows 实机上的 `MoveFileExW`、优雅退出与事务崩溃恢复。
- 2,000 条模组在真实 WebView 中的滚动帧率；当前仅实施浏览器级跳过绘制优化。
- 大型整合包导入需要等量临时磁盘空间，这是事务式导入的已知代价。

---

<a id="en"></a>

# SRL v0.2.5 — Stability, Performance & Bug Fixes

[简体中文](#zh) · **English**

> Engineering wrap-up release. No new features; focused on log capture, cancellation, file transactions and Windows replacement safety.

## Fixed

- Game stdout/stderr is read as bytes with bounded per-line buffering and lossy UTF-8 decoding. GBK console output on Chinese Windows no longer aborts log capture.
- Player checkpoints advance by bytes actually consumed and skip partial multi-byte/partial-line prefixes, preventing duplicated or truncated events.
- When capture moves to a new session log, players inferred online from the previous session are downgraded to offline; no ghost online players.
- Cancelling dependency analysis now aborts in-flight HTTP requests; the 120-second overall deadline terminates the whole analysis instead of only blocking the next request.
- Downloads can be cancelled while waiting for response headers; `Retry-After` waits are cancellable; a 60-second no-data stall timeout was added.
- Generic ZIP modpack imports now use the same validate → stage → backup → commit transaction as overrides, with rollback on failure.
- Windows replacements use `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` for downloads, archive export, mod/resource-pack/shader import and FRPC extraction. A failed rename no longer deletes the old file first.
- Transaction manifests are written atomically; recovery re-validates relative paths, transaction directory names and backup completeness, preserving recovery material and reporting errors.
- Extraction enforces actual written bytes and compression ratios, not only declared ZIP header sizes.
- Failed log rotation keeps the true byte counter and retries after 8 MiB instead of every line; individual captured lines are capped at 256 KiB.

## Performance & resources

- FRPC orphan cleanup moved out of the constructor to a one-time blocking-thread task at startup; no more 800 ms async-runtime stall on account connect.
- FRPC binary verification streams SHA-256 instead of reading the full 20–40 MB binary on every room creation.
- Download target locks are removed when the last user releases them, so the map no longer grows for the process lifetime.
- Frontend launch logs flush at 5 Hz with a 400-entry buffer; long mod lists use `content-visibility` to skip offscreen layout/paint.
- Baseline (Intel i9-9880H / macOS 26.7 / release): 50,000 log lines parsed in 33.4 ms, incremental read 52.9 ms, 100 instances scanned in 5.7 ms.

## Verification

- Rust: 264 unit tests + 2 launch integration tests pass.
- Frontend: 50 tests pass; `tsc -b` and `vite build` pass.
- `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
- New tests cover non-UTF-8 logs, byte offsets, cross-session player state, rotation failure, cancellation of in-flight requests, download cancellation, target-lock release and transaction path validation.

## Requires real-device validation

- Windows `MoveFileExW`, graceful exit and transaction crash recovery.
- Frame rate for 2,000 mods in a real WebView; only browser-level paint skipping is implemented.
- Large modpack imports require temporary disk space equal to the extracted size; this is the cost of transactional import.
