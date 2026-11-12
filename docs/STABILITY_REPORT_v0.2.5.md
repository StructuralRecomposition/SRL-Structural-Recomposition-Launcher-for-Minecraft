# SRL v0.2.5 工程收尾报告

日期：2026-11-13  
基准：v0.2.4（commit `2c224719`）  
目标：v0.2.5（Stability, Performance & Bug Fix Release）

## A. 项目总体情况

| 项目 | 数值 |
|---|---|
| 基准提交 | `2c2247198d077f831f737d58e99f3c42e451dc49`（v0.2.4） |
| 工作分支 | `migrate/52frp` |
| 架构调整 | 无 |
| Rust 源文件 | 73 个，约 38.2k 行 |
| TS/TSX 源文件 | 61 个，约 18.5k 行 |
| 版本号 | `Cargo.toml` / `Cargo.lock` / `package.json` / `tauri.conf.json` 全部为 0.2.5 |

最终提交号与标签在发布阶段记录于文末。

## B. Bug 修复清单

所有条目均来自对当前源码的检查，并有对应测试。

### B1. 非 UTF-8 游戏输出会永久中断日志采集（P0）

- 位置：`crates/sr-core/src/launch.rs`，`spawn_reader` / `AsyncBufReadExt::lines`。
- 原因：`lines()` 要求整行是合法 UTF-8；Java 在中文 Windows 上可能输出 GBK。遇到一个非法字节就返回 `InvalidData`，读取任务直接 `break`，后续 stdout/stderr 全部丢失，玩家事件和联机状态随之失效。
- 修复：新增 `read_line_bounded`，按字节 `fill_buf`/`consume`，有损解码；单行上限 256 KiB，超长行丢弃剩余部分并标记。
- 测试：`launch::tests::captures_non_utf8_output_without_stopping`（PASS）。

### B2. 玩家检查点按字符长度推进，导致偏移漂移（P1）

- 位置：`crates/sr-core/src/multiplayer/minecraft.rs`，`read_player_events_incremental`。
- 原因：`String::from_utf8_lossy` 后使用 `text.len()` 作为字节偏移；替换字符长度与原字节不同，下一次读取会重复或跳过内容。`read_to_string` 还会在非法 UTF-8 时整个失败。
- 修复：读取字节，按实际读取长度推进；定位到行中间时跳过残片；偏移正好落在换行后时不丢弃首行。
- 测试：`checkpoint_tolerates_invalid_utf8_and_keeps_advancing`（PASS）、既有 `checkpoint_reads_only_appended_bytes`（PASS）。

### B3. 日志轮转后出现跨会话幽灵玩家（P1）

- 位置：同上，`read_player_events_incremental` 的 `checkpoint.log_path` 分支。
- 原因：路径变化时只重置偏移和序号，保留 `online_inferred` 状态；新会话日志里没有离场事件，玩家永久显示在线。
- 修复：路径变化时把所有 `online_inferred` 降级为 `offline_inferred`，保留历史时间。
- 测试：`new_log_session_clears_inferred_online_players`（PASS）。手机版/基岩版无 Java 日志，仍返回不可用，未改变行为。

### B4. 依赖分析取消与整体 deadline 实际无效（P1）

- 位置：`crates/sr-core/src/dependency/resolve.rs`，`analyze_with_cancel`、`prefetch_dependencies`。
- 原因：取消只在请求前检查；在途 `adapter.fetch` 不可中断。整体 deadline 只在循环顶部检查，单请求 15 秒和 4 路预取可让总耗时远超 120 秒。取消产生的 `Err(Cancelled)` 还会被当作“未知依赖”节点吞掉，最终返回成功分析。
- 修复：新增 `bounded`/`fetch_bounded`，用 `select!` 同时等待请求、整体 deadline 和取消令牌；取消直接返回 `Cancelled`；deadline 命中时 `break` 并设置 `complete=false`。根模组请求也受 30 秒和整体 deadline 约束。
- 测试：`cancellation_interrupts_an_in_flight_fetch`（PASS，100 ms 内取消 10 秒延迟请求）、既有 `cancellation_stops_before_dependency_requests`（PASS）。

### B5. 下载等待响应头时不可取消（P1）

- 位置：`crates/sr-core/src/download.rs`，`try_download_once`。
- 原因：`request.send().await` 不受取消令牌约束；连接建立但服务器不返回响应头时，最长要等客户端 1800 秒超时。
- 修复：`send()` 与取消令牌 `select!`；`Retry-After` 等待同样可取消；响应流增加 60 秒无数据停滞超时。
- 测试：`cancel_interrupts_a_pending_response`（PASS，150 ms 内返回 `Cancelled`）。

### B6. Windows 覆盖文件先删除旧文件（P0）

- 位置：`download.rs::replace_file`、`mods.rs`、`shaders.rs`、`resourcepacks.rs`、`modpack_export.rs`、`transfer.rs`、`frpc.rs`。
- 原因：`#[cfg(windows)]` 分支先 `remove_file(dest)` 再 `rename(tmp, dest)`；重命名因杀毒/权限失败时，原文件已经不存在，且无法恢复。
- 修复：新增 `util::replace_file_sync/async`，Windows 使用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`；失败时保留原文件。
- 验证：代码路径审查 + Windows 交叉编译；真实 Windows 设备验证标记为 BLOCKED（见 E）。

### B7. 日志轮转失败后字节计数归零（P1）

- 位置：`launch.rs`，`SessionLogWriter::rotate`。
- 原因：所有 rename 错误被忽略，随后重开当前文件并把 `bytes` 设为 0。若重命名持续失败（Windows 文件占用），会每 32 MiB 重试一次并反复移位分片；若重开失败则静默丢行。
- 修复：轮转失败时保留真实 `bytes`，将下次重试阈值提高到 +8 MiB；只有成功轮转或文件确实消失时才归零；写入前若文件句柄不可用返回错误而不是静默成功。
- 测试：`failed_rotation_keeps_byte_counter`（PASS，Unix 只读目录模拟 rename 失败）。

### B8. 通用 ZIP 整合包直接覆盖用户文件（P0）

- 位置：`crates/sr-core/src/resources.rs`，`extract_generic_blocking`。
- 原因：逐条 `File::create(dest)`，无暂存、无备份；中途失败会留下半完成实例，并覆盖同名配置/模组。
- 修复：改为复用 overrides 事务：完整校验 → 解压到 `.srl-import-staging-*` → 备份到 `.srl-import-backup-*` → 逐条提交 → 失败回滚。仅保留“单根目录剥离”的原有行为。
- 验证：既有 `extracts_overrides_transactionally`、`refuses_to_write_through_existing_symlink`（PASS）。

### B9. 事务清单写入与恢复缺少校验（P0/P1）

- 位置：`resources.rs`，`write_import_manifest`、`recover_import_transaction`。
- 原因：清单先删旧文件再 rename，崩溃窗口内会丢失唯一恢复记录；恢复直接 `root.join(relative)`，不校验路径；`existed=true` 但备份缺失时静默跳过。
- 修复：`util::atomic_write_sync` 原子替换；恢复用 `safe_relative` + `ensure_path_within_root` 重新校验；事务目录名限定 `.srl-import-*` 且不得包含分隔符/`..`；备份缺失视为恢复失败并保留材料。
- 测试：`recovery_rejects_manifest_path_traversal`、`recovery_rejects_unsafe_transaction_directories`、既有恢复测试（PASS）。

### B10. FRPC 构造路径阻塞异步运行时（P2→P1）

- 位置：`crates/sr-core/src/multiplayer/frpc.rs`，`FrpcManager::new_with_bundle`。
- 原因：构造函数同步执行 `ps`、`SIGTERM`、`sleep(800ms)`、`SIGKILL`，并在每次 `connect()` 创建 manager 时重复。启动和连接都会被阻塞。
- 修复：构造函数只做内存初始化；新增 `cleanup_stale_dir`，启动时在 `spawn_blocking` 中执行一次；SIGKILL 前重新扫描命令行，避免 PID 复用误杀。
- 测试：既有 FRPC 单元测试通过；无新增时序测试（依赖真实进程）。

### B11. FRPC 缓存校验整文件读取（P2）

- 位置：`frpc.rs::cached`。
- 原因：每次 `prepare()` 都用 `std::fs::read` 读取完整二进制（约 20–40 MB）再计算 SHA-256。
- 修复：`util::sha256_file_sync` 64 KiB 分块流式校验；新增 `executable_magic_file` 只读前 4 字节。
- 验证：`cargo test -p sr-core multiplayer::frpc`（PASS）。

### B12. 下载目标锁表无界增长（P2）

- 位置：`download.rs`，`target_locks`。
- 原因：每个下载目标路径的 `Arc<Mutex<()>>` 永久保留；安装多个版本后条目持续增加。
- 修复：改为 `TargetGuard`，Drop 时在无等待者的情况下删除条目；用 `Arc::strong_count` 避免误删仍在等待的锁。
- 测试：`target_lock_entries_are_removed_after_use`（PASS）。

## C. 性能优化清单

| 优化项 | 优化前 | 优化后 | 环境 | 结果 |
|---|---|---|---|---|
| 游戏日志 IPC/React 更新 | 每条日志一次全局 `setState` | 200 ms 批量刷新，缓冲上限 400 | React 19 / WebView | 代码级优化，状态提交从 O(行数) 降为 ≤5 次/秒；未做真实帧率对比 |
| 模组列表渲染 | 全部行参与布局/绘制 | `.mod-list li { content-visibility: auto }` | WebView | 代码级优化；不改变筛选/交互 |
| FRPC 缓存校验内存 | 每次读取 20–40 MB | 64 KiB 流式 SHA-256 | Intel i9-9880H | 峰值内存显著降低；未单独计时 |
| FRPC 启动阻塞 | 构造时同步 800 ms + ps | 启动后台一次 | macOS | 代码审查确认异步路径不再阻塞 |
| 下载目标锁 | 永久增长 | Drop 清理 | 单元测试 | 条目数在释放后归零 |
| 日志解析 | — | 未结构改动 | i9-9880H / release | 50,000 行 33.4 ms |
| 增量日志读取 | — | 未结构改动 | 同上 | 4.6 MB / 52.9 ms |
| 实例扫描 | — | 未结构改动 | 同上 | 100 个实例 5.7 ms |

结论：Rust 侧日志解析、增量读取和实例扫描在目标规模下均远低于 100 ms，没有依据做进一步结构优化。前端只做了有明确依据的事件批处理与跳过屏外绘制；未声称未经测量的帧率提升。

## D. 测试报告

| 测试项 | 状态 | 结果 | 备注 |
|---|---|---|---|
| `cargo test --workspace --all-targets` | PASS | 264 unit + 2 integration；5+13 ignored（真实网络/环境） | 忽略项均为需要外网或真实服务端 |
| `npx vitest run` | PASS | 7 files / 50 tests | |
| `npx tsc -b` | PASS | 无错误 | |
| `npm run build` | PASS | Vite 7 生产构建 | 612.7 kB JS，gzip 186.2 kB |
| `cargo fmt --all -- --check` | PASS | | |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS | 0 warning | |
| `scripts/verify-no-secrets.mjs` | PASS | | |
| `cargo audit`（2026-11-13 advisory DB，1490 条） | PASS | 0 vulnerability；5 个 warning | glib/zBus 等 unmaintained/unsound 警告见 E |
| `npm audit --audit-level=high` | PASS | 2 moderate（vitest 开发依赖） | 无 high/critical；升级 vitest 5 为破坏性变更，未在本次执行 |
| GitHub Actions v0.2.5 | PASS | quality / audit / Linux / Windows / macOS / release 全部成功 | https://github.com/StructuralRecomposition/SRL-Structural-Recomposition-Launcher-for-Minecraft/actions/runs/43314777627 |
| `npx tauri build --no-bundle`（release） | PASS | macOS 本地 release 构建于 8m 完成 | 未生成 DMG；CI 生成三平台产物 |
| macOS release 二进制启动 smoke | PASS | 启动 8 秒，发现 5 个 Java，无 panic | 仅启动；未执行完整安装/启动游戏 |
| 真实 Windows 构建 | BLOCKED | 当前 macOS 主机 | 由 GitHub Actions `windows-latest` 执行 |
| Windows 实机文件替换/优雅退出 | BLOCKED | 无 Windows 设备 | 需人工验证 |
| 50,000 条日志 UI 滚动 | SKIPPED | 不适用 | UI 只读尾部 400 行；全量在文件 |
| 2,000 条模组真实 WebView 帧率 | SKIPPED | 未做自动化帧率测量 | 仅浏览器级跳过绘制 |
| 2 小时长时间运行 | SKIPPED | 本轮未执行 | 需发布前人工浸泡 |
| 磁盘满/权限不足 | SKIPPED | 未在本机安全模拟 | 代码路径返回错误，未做破坏性测试 |

## E. 遗留问题

1. **Windows 实机验证缺失。** `MoveFileExW`、优雅退出、事务崩溃恢复只做了代码路径和交叉编译检查。
2. **Modrinth/CurseForge 整合包下载不是完整事务。** `install_modrinth_pack` / `install_curseforge_pack` 会逐个下载并原子替换 `mods/` 文件；单个文件不会损坏，但中途失败会留下部分模组，且同名旧模组可能已被替换。overrides 与通用 ZIP 已事务化；此路径未在本次扩大改造，避免高风险重构。
3. **UI 日志仍逐条跨 IPC。** 前端已批处理 React 更新，但后端仍为每行发送一个 Tauri 事件；极端日志洪流下 IPC 开销仍在。
4. **`lifecycle` 实例锁表不主动清理。** 删除实例后对应 `Arc<Mutex>` 仍保留；数量与历史实例数同阶，暂未处理。
5. **launcher.log 仅按天轮转。** `tracing_appender::rolling::daily` 不做单文件大小切分；启动时按 14 天/200 MB 清理。长时间不重启且日志量极大时单日文件可能偏大。
6. **未签名/未公证。** macOS 与 Windows 产物均无代码签名；未伪造签名验证结果。
7. **RustSec 警告。** `glib 0.18.5` 的 `VariantStrIter` unsound（RUSTSEC-2024-0429）来自 Tauri Linux/WebKitGTK 依赖；升级到 0.20 会牵动整条 GTK 栈，本次不改。`proc-macro-error`、`zbus_names`、`zvariant` 为 unmaintained warning，无已知可利用漏洞。`rustix` 已从 1.1.5 升级到 1.1.6 修复 RUSTSEC-2026-0486。
8. **npm 开发依赖告警。** `npm audit` 报告 2 个 moderate（`@vitest/mocker` 路径穿越/代码注入），仅影响测试工具链；修复需要 vitest 5 的破坏性升级，本次未执行，`--audit-level=high` 通过。
9. **RustSec 数据库获取方式。** 本机 `git` 访问 RustSec 仓库会超时；改用 GitHub codeload 下载 2026-11-13 快照（1490 条）并执行 `cargo audit --no-fetch`，结果 0 vulnerability。CI 会自行拉取最新数据库。

## F. 发布情况

- 版本：v0.2.5
- 最终提交：`569df0e002e41eb653e2d9d23cd29b36b4b71d7f`
- Tag：`v0.2.5`（annotated tag `bd3b4db9279e895e03d766861bb59ad0af37e93f`）
- GitHub Release：https://github.com/StructuralRecomposition/SRL-Structural-Recomposition-Launcher-for-Minecraft/releases/tag/v0.2.5
- CI run：https://github.com/StructuralRecomposition/SRL-Structural-Recomposition-Launcher-for-Minecraft/actions/runs/43314777627
- CI 结果：quality ✅、Security audit ✅、Linux ✅、Windows ✅、macOS universal ✅、release ✅

### 发布产物

| 文件 | 大小 |
|---|---:|
| `SRL_0.2.5_universal.dmg` | 40,380,691 |
| `SRL_0.2.5_windows-x64-portable.zip` | 21,350,532 |
| `SRL_0.2.5_amd64.deb` | 20,545,826 |
| `SRL_0.2.5_amd64.AppImage` | 98,662,904 |
| `SRL-0.2.5-source.tar.gz` | 5,340,998 |
| `SHA256SUMS` | 458 |

### 校验

- `SHA256SUMS` 覆盖全部 5 个发布文件（含源码归档）。
- 逐项比对 GitHub Release API 返回的 `sha256` digest 与 `SHA256SUMS`：全部一致。
- 额外下载 AppImage 并本地 `shasum -a 256`：`0f4a3aa4…7c1c`，与清单一致。

### CI 过程说明

发布前 tag 共重打两次，均发生在 Release 创建之前：

1. 首次：`rustsec/audit-check` 缺少 `checks: write`，无法创建 check run。已补权限。
2. 第二次：诊断页“删除并清理日志”前端测试存在时序竞态（删除未完成时点击清理按钮）。已改为等待按钮可用。
3. 第三次：全部 job 成功并发布。

没有覆盖任何已发布的正式版本标签；v0.2.5 Release 是在最终成功 CI 对应提交上创建的。
