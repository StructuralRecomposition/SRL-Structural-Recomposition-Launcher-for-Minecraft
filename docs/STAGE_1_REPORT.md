# 阶段 1 完成报告（含阶段 2/3 部分实现）

日期：2026-10-23
版本：0.1.0
环境：macOS 26.7 (x86_64)、Node 24.8、Rust 1.99.1、Tauri 2.12.5

## 1. 本阶段完成的功能

| 功能 | 状态 |
|---|---|
| 工作区与环境检查 | ✅ 已执行 |
| PCL2 / HMCL 来源与许可证核验 | ✅ 已执行 |
| 研究文档、功能矩阵、ADR、许可证审计、安全评审、开放问题 | ✅ 已建立 |
| Tauri 2 + React 19 + TS + Vite 工程 | ✅ 已实现并构建 |
| Cargo workspace：`sr-core` + Tauri 薄壳 | ✅ 已实现 |
| 统一错误模型与稳定错误码 | ✅ 已实现并测试 |
| 原子 JSON 配置读写 | ✅ 已实现并测试 |
| 官方版本清单 + 6h 缓存 + 断网回退 | ✅ 已实现并测试 |
| 版本元数据解析 / 继承 / 规则 | ✅ 已实现并测试（真实 26.4 夹具） |
| 下载引擎（并发/取消/重试/哈希/原子写入） | ✅ 已实现并测试 |
| 版本安装（真实 1.12.2） | ✅ 已实现并测试 |
| natives 多包合并解压 | ✅ 已实现并测试（真实安装 4 个条目） |
| 文件完整性检查 | ✅ 已实现并测试 |
| Java 发现 / 版本解析 / 兼容性 | ✅ 已实现并测试（本机发现 3 个） |
| 启动参数生成 / 进程 / 日志 / 退出码 | ✅ 已实现并测试（假 Java 全链路） |
| 多实例 CRUD / 复制 / 删除 | ✅ 已实现并测试 |
| 每实例账号与分辨率 | ✅ 已实现并测试 |
| 实例导出 / 导入（安全 ZIP） | ✅ 已实现并测试 |
| 模组文件管理（启停/删除/导入） | ✅ 已实现并测试 |
| 离线账号档案 | ✅ 已实现并测试 |
| 9 个页面 + 独立设计系统 | ✅ 已实现并测试（组件/交互测试） |
| 诊断中心（检查/日志/预览复制） | ✅ 已实现并测试 |
| 崩溃报告分析 | ✅ 已实现并测试 |
| Microsoft 登录 | ✅ 已实现并测试（模拟 OAuth 全链路；真实 client id 未验证） |
| Fabric 加载器安装 | ✅ 已实现并测试（真实 1.20.4） |
| Quilt 加载器 | ⚠️ 已实现未完整验证（网络受限） |
| Forge / NeoForge / Quilt / 整合包 / 更新 | ⛔ 未实现（UI 明确禁用并说明） |
| 任务持久化（中断标记） | ✅ 已实现并测试 |
| 跨任务全局下载限流 | ✅ 已实现并测试 |
| 断点续传 | ✅ 已实现并测试 |

## 2. 新增或修改的关键文件

- `crates/sr-core/src/{error,util,paths,settings,download,install,java,instance,launch,account,diagnostics}.rs`
- `crates/sr-core/src/version/{mod,model,rules,resolve,args}.rs`
- `crates/sr-core/tests/{launch_pipeline,real_install}.rs`
- `crates/sr-core/tests/fixtures/version_26.4-snapshot-5.json`、`version_manifest_v2.json`
- `src-tauri/src/{lib,commands,state}.rs`、`tauri.conf.json`、`capabilities/default.json`
- `src/{App,main,styles.css}`、`src/pages/*.tsx`、`src/components/*`、`src/lib/*`、`src/state/*`
- `research/*.md`、`docs/*.md`、`README.md`、`LICENSE`、`SECURITY.md`、`AGENTS.md`
- `icon-1024.png` 与 `src-tauri/icons/*`（项目自绘）

## 3. 已执行的构建命令

```bash
cargo check -p sr-core
cargo check -p sr-launcher
cargo fmt --all -- --check
cargo clippy -p sr-core --all-targets -- -D warnings
cargo clippy -p sr-launcher --all-targets -- -D warnings
npm run build
npx tauri build --debug --no-bundle
```

## 4. 已执行的测试命令

```bash
cargo test -p sr-core
cargo test -p sr-core --test real_install -- --ignored --nocapture
npx vitest run
```

## 5. 实际测试结果

| 项目 | 结果 |
|---|---|
| `cargo test -p sr-core` | ✅ 100 项库测试通过；网络测试默认忽略 |
| 启动集成测试 | ✅ 2 项通过（假 Java、缺文件拒绝启动） |
| 真实 1.12.2 安装 | ✅ 通过：37 个核心文件校验通过，4 个 natives 条目 |
| 真实 1.20.4 + Fabric | ✅ 通过：66 个库、共享原版 JAR、完整性校验通过 |
| 前端测试 | ✅ 11 项通过（3 个文件） |
| `npm run build` | ✅ 通过（TS 严格检查 + Vite） |
| `cargo fmt --check` | ✅ 通过 |
| `cargo clippy`（core + tauri） | ✅ 通过，`-D warnings` |
| Tauri debug 构建 | ✅ 生成 macOS 可执行文件 |
| 应用实际启动 | ✅ 进程存活，数据目录创建，Java 扫描发现 3 个 |
| GUI 截图 | ❌ 受 macOS 辅助访问/录屏权限限制，未取得 |
| 真实 Minecraft 进程启动 | ❌ 未验证：本机无 Java 8（1.12.2 要求）且无正版账号 |

## 6. 尚未完成的功能

- Microsoft OAuth 登录与令牌刷新（需 Azure 应用注册）。
- Forge/NeoForge 加载器；Quilt 真实库下载验证。
- CurseForge/Modrinth 等整合包格式（自有实例归档已完成）。
- 整合包安全导入/导出。
- 应用更新与签名验证。
- 中断任务的自动重启（当前仅标记；重新安装可复用断点文件）。
- 资源文件完整下载的端到端测试（安装选项已实现）。
- Windows/Linux 构建与运行验证。

## 7. 当前阻塞问题

1. 无 Microsoft OAuth 客户端配置，正版登录无法实现。
2. 本机无 Java 8，无法验证旧版本真实启动；仅验证了参数与进程链路。
3. macOS 辅助访问权限受限，无法自动截图或读取窗口信息。
4. 未在 Windows/Linux 实机验证。

## 8. 已知安全问题

- 已实现：路径穿越防护、URL 白名单、SHA-1 校验、原子写入、日志脱敏、无 shell 插件、ZIP 解压上限、最小 Tauri 权限。
- 剩余风险：官方源被入侵、恶意用户自选 Java、WebView 0day、JVM 命令行可见性、依赖漏洞尚未自动化扫描（`cargo audit`/`npm audit` 未接入）。

## 9. 平台兼容性限制

- 仅在 macOS 26.7 x86_64 实机验证。
- Windows/Linux 代码路径已实现但未构建验证。
- Windows 超长命令行未处理。
- Linux 无 Secret Service 时不能保存令牌（当前未写入令牌）。

## 10. 下一阶段建议

1. 实现全局下载健康度（失败率/退避）监控与镜像策略。
2. 在 Windows/Linux 实机（或 CI runner）执行构建与启动验证。
3. 配置 Azure 应用后实现 Microsoft OAuth + keyring 令牌存储。
4. 实现 Quilt/NeoForge 适配器（Fabric 已完成）。
5. 接入 `cargo audit`、`npm audit` 与三平台 CI。

---

## 阶段 1.5 增补（2026-10-24）

- 版本页：下载版本按正式版/快照/旧版 Beta/旧版 Alpha 分栏；新增“已安装”视图，读取 `versions/` 并支持打开目录、快速创建实例、检查、删除。
- 版本删除：被实例引用时拒绝；被加载器继承时需显式确认并同时删除依赖版本；路径与符号链接校验。
- 模组：拖拽、原生文件选择、批量导入、原始字节 IPC、打开 mods 目录。
- 光影：`shaderpacks/` 管理 + Iris/OptiFine 检测；Modrinth Iris 安装代码已实现，但因当前网络不可达，未完成真实下载验证。
- 测试：Rust 113 单元 + 2 启动集成通过；前端 20 项通过；`cargo clippy -D warnings`、`npm run build`、Tauri debug 构建通过。
- 仍未验证：Windows/Linux、真实 Microsoft 登录、真实 Minecraft 启动、Iris 线上下载。
