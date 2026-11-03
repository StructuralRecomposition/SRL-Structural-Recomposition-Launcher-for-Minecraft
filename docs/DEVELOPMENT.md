# 开发指南

## 代码地图

```
src/                      React 界面
  components/             设计系统与通用组件
  pages/                  9 个页面
  state/AppContext.tsx    全局状态、任务/启动事件订阅
  lib/api.ts              类型化 IPC 封装
  lib/format.ts           字节/速度/时间格式化
src-tauri/                Tauri 薄壳
  src/lib.rs              启动、状态装配、命令注册
  src/commands.rs         IPC 命令
  src/state.rs            共享状态与事件 sink
crates/sr-core/           业务核心（可独立测试）
  src/version/            元数据模型、规则、继承、参数
  src/download.rs         下载引擎与任务
  src/install.rs          安装编排
  src/launch.rs           启动与进程管理
  tests/                  集成测试与真实网络测试
research/                 研究、功能矩阵、ADR、许可证、安全
docs/                     架构与运维文档
```

## 开发循环

```bash
npm run dev:desktop        # 前端热更新 + Rust 重编译
cargo test -p sr-core      # 核心逻辑
npx vitest run             # UI
cargo check -p sr-launcher # 命令层
npm run build              # 类型检查 + 生产构建
```

## 添加一个 IPC 命令

1. 在 `crates/sr-core` 实现业务函数，返回 `Result<T, AppError>`，并写单元测试。
2. 在 `src-tauri/src/commands.rs` 添加 `#[tauri::command]` 薄包装：
   - 校验输入范围/路径；
   - 读取 `State<AppState>`；
   - 不把业务逻辑写进命令。
3. 在 `src-tauri/src/lib.rs` 的 `invoke_handler` 注册。
4. 在 `src/lib/api.ts` 添加类型与 wrapper。
5. 页面调用 wrapper；错误通过 `ApiError` 统一展示。
6. 增加前端测试（成功/空/错误状态至少各一）。

## 约定

- **错误**：使用稳定 `ErrorCode` + 面向用户的中文 `message`；`detail` 可为原始错误，但不得包含令牌。
- **路径**：任何来自元数据/用户输入的相对路径必须经过 `safe_join`。
- **文件写入**：配置与元数据使用 `atomic_write`；下载先写唯一临时文件再 rename。
- **命令**：使用 `Command::new(...).args([...])`，禁止 `sh -c` 拼接。
- **日志**：记录事件与原因，不记录完整启动参数；用户可见文本必须脱敏。
- **UI**：按钮必须有真实行为；不可用功能必须禁用并说明原因；异步任务显示真实状态。
- **测试**：不得把模拟测试描述为真实环境测试；网络测试默认 `#[ignore]`。

## 设计系统

`src/styles.css` 顶部为设计令牌：

- 深色为默认；浅色通过 `data-theme="light"`。
- 间距 4/8/12/16/18/24；圆角 8/12/16。
- 颜色语义：`--accent`（青绿）、`--violet`、`--success`、`--warning`、`--danger`。
- 动效 ≤ 220ms，并遵循 `prefers-reduced-motion`。
- 组件：`Card`、`Button`、`Badge`、`Field`、`Toggle`、`ProgressBar`、`Modal`、`EmptyState`。

禁止照搬参考启动器的布局、图标或文案。

## 测试策略

| 层 | 工具 | 覆盖 |
|---|---|---|
| 核心单元 | `cargo test -p sr-core` | 规则、继承、参数、路径、哈希、任务、Java、实例、账号 |
| 下载/进程集成 | 本地 HTTP 服务、假 Java | 校验失败清理、取消、启动参数、退出码 |
| 真实网络 | `--ignored` 测试 | 官方清单、真实安装 1.12.2 |
| 前端 | Vitest + Testing Library | 页面加载、启动交互、空状态、离线账号标识 |

## 当前 TODO（按阶段）

- 阶段 2：任务持久化/崩溃恢复、断点续传、跨任务全局限流。
- 阶段 3：Windows/Linux 实机验证、Java 版本选择 UI 细化、崩溃报告解析。
- 阶段 4：Microsoft OAuth（需 Azure 应用）、令牌写入 keyring、账号刷新。
- 阶段 5：Fabric → Quilt → NeoForge → Forge 适配器。
- 阶段 6：安全 zip 整合包导入、实例导出、诊断报告文件导出。
- 阶段 7：更新签名与回滚、`cargo audit`/`cargo deny`。
- 阶段 8：三平台 CI 构建与安装包验证。

## 提交前检查

```bash
cargo fmt --all -- --check
cargo clippy -p sr-core -- -D warnings
cargo test -p sr-core
npm run build
npx vitest run
```
