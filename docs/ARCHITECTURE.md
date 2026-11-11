# 架构

## 1. 分层

```
┌──────────────────────────────────────────────┐
│ React UI (src/)                              │
│ 页面 / 组件 / 设计系统 / 事件订阅            │
└──────────────────────┬───────────────────────┘
                       │ Tauri IPC (invoke / events)
┌──────────────────────▼───────────────────────┐
│ src-tauri (薄壳)                             │
│ AppState · commands.rs · 日志 · 窗口         │
└──────────────────────┬───────────────────────┘
                       │ Rust API
┌──────────────────────▼───────────────────────┐
│ crates/sr-core (业务核心，无 GUI 依赖)       │
│ version · download · install · java ·        │
│ instance · launch · account · diagnostics    │
└──────────────────────┬───────────────────────┘
                       │
       文件系统 / HTTPS / 子进程 / 系统凭据库
```

设计约束：

- 前端不做版本解析、下载调度、进程管理。
- `sr-core` 不依赖 Tauri，可在无 GUI 环境完整测试。
- Tauri 命令只做参数校验、状态读取和任务编排，不承载核心算法。

## 2. 模块职责

| 模块 | 位置 | 职责 |
|---|---|---|
| `error` | `crates/sr-core/src/error.rs` | 稳定错误码、分类、可重试标记 |
| `paths` | `paths.rs` | 应用数据目录与游戏目录布局 |
| `settings` | `settings.rs` | 设置校验与原子持久化 |
| `version::model` | `version/model.rs` | 官方元数据模型、Maven 坐标 |
| `version::rules` | `version/rules.rs` | os/arch/features 规则求值 |
| `version::resolve` | `version/resolve.rs` | 继承合并、库筛选、classpath 候选 |
| `version::args` | `version/args.rs` | 占位符替换、JVM/游戏参数、旧版分词 |
| `version` | `version/mod.rs` | 清单缓存、远程解析、安装计划、完整性检查 |
| `download` | `download.rs` | 任务管理、并发、取消、重试、校验、原子写入 |
| `install` | `install.rs` | 安装编排、natives 安全解压、最终校验；支持预解析的加载器元数据 |
| `fabric` | `fabric.rs` | Fabric 官方 meta、profile 解析、Maven `.sha1` 补齐、共享原版 JAR |
| `java` | `java.rs` | Java 发现、版本解析、兼容性 |
| `instance` | `instance.rs` | 实例 CRUD、目录隔离、复制/删除 |
| `launch` | `launch.rs` | 预检、参数生成、进程、日志泵、退出状态 |
| `account` | `account.rs` | 离线档案、账号文件、凭据抽象 |
| `diagnostics` | `diagnostics.rs` | 环境检查、日志读取/脱敏、报告导出 |
| `transfer` | `transfer.rs` | 实例 ZIP 导出/导入、归档安全校验 |
| `commands` | `src-tauri/src/commands.rs` | IPC DTO、状态变更、任务启动 |
| `state` | `src-tauri/src/state.rs` | 共享状态、事件 sink |

## 3. 关键流程

### 3.1 安装版本

```
install_version(version_id)
  ├─ VersionService.resolve_remote
  │    ├─ manifest（缓存 6h / 网络回退）
  │    ├─ 逐级抓取父版本 JSON（SHA-1 校验）
  │    └─ resolve_inheritance（循环/深度检查）
  ├─ ensure_asset_index（校验并原子写入 indexes/）
  ├─ build_install_plan（客户端 + 库 + natives + assets + log config）
  ├─ buffer_unordered(并发上限) 下载
  │    └─ download_file：临时文件 → SHA-1 → rename
  ├─ extract_natives（安全 zip 解压）
  ├─ materialize_virtual_assets（旧版）
  ├─ save_installed_metadata（最后一步）
  └─ 再次校验客户端 JAR
```

元数据只有在全部步骤成功后才写入 `versions/<id>/<id>.json`，因此"目录存在"不等于"安装成功"。

### 3.2 启动游戏

```
launch_instance(id)
  ├─ 读取实例与已安装元数据
  ├─ Java 选择：实例指定（必须兼容）→ 精确 major →
  │   用户默认（兼容时）→ 安全范围内最接近版本
  ├─ 兼容性检查（required major）
  ├─ 账号检查（当前仅离线档案）
  ├─ 文件预检：客户端 JAR + 全部非 native 库
  ├─ natives 目录检查（旧版需要）
  ├─ build_plan：
  │    ├─ 规则过滤
  │    ├─ 占位符白名单替换
  │    ├─ classpath（平台分隔符）
  │    └─ -Xmx + 额外参数
  ├─ Command::new(java).args(vec)（不经过 shell）
  ├─ stdout/stderr 异步逐行泵送 → 事件 + 日志文件
  └─ 300ms 轮询 try_wait → 退出码事件
```

### 3.3 停止游戏

- Unix：先 `SIGTERM`，让 JVM 执行 shutdown hook（保存世界）；8 秒后仍在运行则强制终止。
- Windows：`start_kill`。
- 只操作本启动器创建的进程，不按名称杀进程。

## 4. IPC 命令

| 命令 | 输入 | 输出 |
|---|---|---|
| `get_bootstrap` | — | 设置、路径、实例、账号、Java、已安装版本、功能开关 |
| `list_versions(refresh)` | bool | 清单 + 已安装集合 + 来源/警告 |
| `install_version` | id, includeAssets | taskId |
| `list_tasks` / `cancel_task` / `clear_finished_tasks` | — | 任务视图 |
| `get_version_detail` / `verify_version` | id | 详情 / 校验报告 |
| `list/create/update/delete/duplicate_instance` | DTO | 实例 |
| `launch_instance` / `stop_instance` | id | pid + 日志路径 / — |
| `list_java(refresh)` / `add_java_path` / `check_java_for_version` | — | Java 信息 |
| `java_launch_check` / `install_java_for_version` | instanceId / major | 启动前 Java 检查 / 自动补全 |
| `list_accounts` / `add_offline_account` / `remove_account` / `set_active_account` / `microsoft_login` | — | 账号 |
| `get_diagnostics` / `list_logs` / `read_log` / `export_diagnostics` | — | 诊断 |

事件：

- `task://update`：任务快照（状态、字节、文件数、速度、ETA、错误）
- `launch://event`：`log` / `exit`

所有命令返回 `Result<T, AppError>`；错误码见 `error.rs`，UI 直接展示 `message`，`detail` 用于展开。

## 5. 并发模型

- Tokio 多线程运行时（Tauri 提供）。
- 每个安装任务独立 `tokio::spawn`，内部 `buffer_unordered(N)` 控制文件级并发。
- 每个任务用 `buffer_unordered(N)` 控制文件级并发；所有任务共享一个全局 `Semaphore`，跨任务总并发受设置约束。
- 同一版本禁止并发安装（`AppState.installing`）。
- 每个下载目标使用唯一临时文件，最后 `rename`，避免并发写坏。
- 任务进度通过 350ms ticker 聚合，避免每 chunk 触发 IPC。

## 6. 存储

| 文件 | 内容 | 写入方式 |
|---|---|---|
| `settings.json` | 游戏目录、并发、主题 | 临时文件 + fsync + rename |
| `accounts.json` | 档案元数据，**无令牌** | 同上 |
| `cache/version_manifest_v2.json` | 官方清单缓存 | 同上 |
| `instances/<id>/instance.json` | 实例配置 | 同上 |
| `versions/<id>/<id>.json` | 已解析的完整元数据 | 安装成功后写入 |
| `logs/` | 启动器/游戏日志 | 追加 |

不引入 SQLite：当前数据量小、无查询需求。历史任务/大规模模组索引出现后再评估。

## 7. 安全边界

- WebView 为不可信输入源：命令参数在 Rust 侧再次校验。
- 下载 URL：HTTPS + 官方域名精确匹配。
- 文件路径：`safe_join` 拒绝绝对路径、`..`、反斜杠、盘符、NUL。
- ZIP：`enclosed_name` + 路径校验 + 总解压上限 + 拒绝符号链接。
- 启动：参数数组，不经过 shell。
- 日志：令牌字段脱敏；不记录完整参数。
- 前端：无 shell/fs 插件，CSP 禁止内联脚本。

## 8. 已知架构债务

1. 任务历史已持久化；重启后未完成任务标记为中断，但不自动续传。
2. 无跨任务全局下载限流。
3. Windows 命令行长度未处理。
4. 版本元数据只支持单一官方源，无镜像抽象（刻意保守）。
5. 任务快照每 3 秒与关键操作时写入，崩溃可能丢失最近约 3 秒的进度。
6. 虚拟资源（legacy）只做了映射，缺少端到端真实版本验证。

## 9. 下一阶段模块（0.3 开发线）

| 模块 | 位置 | 职责 |
|---|---|---|
| `dependency::model` | `crates/sr-core/src/dependency/model.rs` | 统一依赖关系、节点、边、分析结果 |
| `dependency::resolve` | `dependency/resolve.rs` | Modrinth / CurseForge 适配器、递归解析、深度/请求上限 |
| `dependency::installed` | `dependency/installed.rs` | 只读 JAR 元数据：Fabric/Quilt/Forge/NeoForge、真实 mod id、CurseForge 指纹 |
| `dependency::apply` | `dependency/apply.rs` | 服务端保存分析、校验选择、复用 `mod_install::apply_files` 安装 |
| `multiplayer::server_status` | `multiplayer/server_status.rs` | Minecraft Java Server List Ping、VarInt、MOTD、RTT、错误分类 |
| `multiplayer::guest` | `multiplayer/guest.rs` | 游客网络监测任务、取消、采样状态 |
| `multiplayer::journal` | `multiplayer/journal.rs` | 房间历史/日志 JSONL、脱敏、大小上限 |
| `minecraft::parse_player_events` | `multiplayer/minecraft.rs` | 从真实游戏日志提取玩家加入/离开与观测地址 |

### 9.1 依赖分析流程

```
analyze_mod_dependencies(request)
  ├─ 可选：读取实例，派生真实 Minecraft 版本与加载器
  ├─ 扫描 mods/：读取 JAR 内 fabric.mod.json / quilt.mod.json /
  │   META-INF/mods.toml / META-INF/neoforge.mods.toml
  ├─ CurseForge：Murmur2 指纹批量匹配 projectID/fileID
  ├─ ModrinthAdapter / CurseForgeAdapter
  ├─ 递归解析 required / optional / incompatible / embedded /
  │   tool / include / unknown；深度上限 16，请求上限 120，节点上限 200
  ├─ 生成 nodes + edges + warnings + blockers + default_plan
  └─ 保存到 cache/dependencies/<analysis-id>.json
```

安装时渲染进程只提交 `analysisId + nodeIds`；URL、大小、SHA-1 从服务端分析记录读取，并再次校验实例、加载器、Minecraft 版本、必需依赖与不兼容关系。下载复用 `mod_install::apply_files`：临时目录 → SHA-1 → 备份同名文件 → 原子 rename → 失败回滚。

### 9.2 联机状态采集流程

```
multiplayer_status()
  ├─ 52FRP 账户
  ├─ 房间持久化记录
  ├─ 本地进程存活 + 本地端口 TCP/UDP 探测
  ├─ 52FRP GET /tunnels/{id}（20 秒缓存）
  ├─ FRPC 子进程状态
  └─ 状态变化写入 room-log.jsonl

multiplayer_members()
  ├─ 读取该实例最新 SRL 游戏日志尾部（上限 4 MB）
  ├─ 解析 joined / left / logged in with entity id
  ├─ 只接受房间创建后的事件
  ├─ 标注观测 IP 类型（loopback/lan/public/unknown）
  └─ 新事件写入 room-log.jsonl，不记录 IP

multiplayer_guest_ping/address
  └─ Rust Server List Ping；前端不直接联网

multiplayer_guest_monitor_start/stop/status
  └─ 单个 Tokio 任务 + CancellationToken；12 秒采样
```

### 9.3 新增 IPC 命令

| 命令 | 输入 | 输出 |
|---|---|---|
| `java_launch_check` | instanceId | 需要的 Java、已选环境、是否可安装、是否需要重置实例 Java |
| `install_java_for_version` | major, setAsDefault=false | 安装后的 JavaInfo；不修改全局默认 |
| `analyze_mod_dependencies` | DependencyRequest | DependencyAnalysis |
| `install_mod_dependencies` | instanceId, analysisId + nodeIds | 资源任务 ID |
| `multiplayer_members` | — | MembersView（部分可用标记、IP 类型、IP 提示） |
| `multiplayer_logs` / `multiplayer_clear_logs` | limit, level | 房间日志 / 清理数量 |
| `multiplayer_guest_ping` | address | ServerStatusReport |
| `multiplayer_guest_monitor_start/stop/status` | address, interval | GuestMonitorStatus |

### 9.4 存储补充

| 文件 | 内容 | 写入方式 |
|---|---|---|
| `cache/dependencies/<id>.json` | 依赖分析快照（含下载 URL/哈希） | 原子写；最多保留 200 份 |
| `multiplayer/room-log.jsonl` | 房间历史与统一日志 | 追加 + 2 MB 上限裁剪 |

仍不引入 SQLite、Redis、独立服务端或前端网络请求。
