# HMCL 研究报告

- 研究日期：2026-10-23
- 仓库：`HMCL-dev/HMCL`
- 核验提交（`main`）：`0d20910ea302`，2026-10-22T13:27:52Z
- Stars：10,611
- 许可证：**GPL-3.0**（仓库根 `LICENSE` 为 GNU GPL v3 全文）
- 文档仓库：`HMCL-dev/HMCL-docs`，commit `bc027fc4555c`，许可证为 **CC BY-NC-SA 4.0**（含附加条款）

## 1. 许可证结论

- HMCL 代码采用 GPL-3.0。任何复制、修改、链接衍生都应按 GPL-3.0 分发，并要求提供对应源码。
- HMCL 文档采用 CC BY-NC-SA 4.0：**禁止商业使用**、要求署名与相同方式共享。因此本项目不摘录其文档文本。
- SR Launcher 采用**净室实现**：不复制 HMCL 源码、资源、语言文件或文档段落。仅参考公开的模块划分思想与 Minecraft 官方规范。若未来需要复用任何 HMCL 代码，必须先将项目整体切换为 GPL-3.0 兼容许可并履行源码开放义务；当前不这样做。

## 2. 模块组织（来自仓库目录结构，事实观察）

`settings.gradle.kts`：

```
rootProject.name = "HMCL3"
include("HMCL", "HMCLCore", "HMCLBoot")
include("HMCLTransformerDiscoveryService", "HMCLMultiMCBootstrap")
```

`HMCLCore/src/main/java/org/jackhuang/hmcl/` 顶层包：

| 包 | 目录内可见类型（示例） | 职责推断 |
|---|---|---|
| `auth` | `Account`, `AuthInfo`, `OAuth`, `ClassicAccount`, `microsoft/`, `offline/`, `yggdrasil/` | 账号抽象、OAuth、离线档案、外置登录 |
| `download` | `DownloadProvider`, `DownloadCandidate(s)`, `DefaultCacheRepository`, `GameBuilder`, `fabric/`, `forge/`, `neoforge/`, `quilt/`, `cleanroom/`, `optifine/` | 下载源抽象、缓存仓库、加载器安装器 |
| `game` | `GameInstance`, `GameRepository`, `Argument(s)`, `CompatibilityRule`, `AssetIndex`, `ExtractRules`, `GameJavaVersion` | 版本/实例模型、规则、资源索引、参数 |
| `java` | `JavaInfo`, `JavaRepository`, `JavaRuntime` | Java 发现与运行时管理 |
| `launch` | `DefaultLauncher`, `ProcessListener`, `StreamPump`, `ExitWaiter`, `CommandTooLongException` | 启动、进程监听、输出泵送 |
| `modpack` | `ModpackProvider`, `ModpackInstallTask`, `curse/`, `modrinth/`, `multimc/`, `server/` | 整合包格式适配 |
| `task` | `Task`, `TaskExecutor`, `AsyncTaskExecutor`, `FileDownloadTask`, `FetchTask`, `Schedulers` | 异步任务与下载任务模型 |
| `addon` | 资源/模组/存档等附加内容管理 | 扩展内容 |
| `event` | 事件总线 | 模块解耦 |
| `util` | 工具 | 基础设施 |

以上为**目录级事实**。各类内部算法、线程模型、缓存失效策略未逐一阅读源码，标记为"待核实"。

## 3. 关键设计观察（抽象层）

1. **接口化账号体系**：`Account` 抽象 + 多实现（Microsoft / 离线 / Yggdrasil），令牌与档案分离（`AuthInfo`）。
2. **下载源候选机制**：`DownloadProvider` + `DownloadCandidate(s)` 表明同一文件可有多个镜像候选，按优先级/可用性选择。SR Launcher 将采用"官方源优先 + 显式可信镜像配置"的简化模型，不内置未经验证的第三方镜像。
3. **任务模型独立**：下载不是裸 `reqwest` 调用，而是可组合的 Task，具备取消、进度、事件。SR Launcher 采用 `TaskManager` + `CancellationToken` + Tauri 事件实现同等能力。
4. **加载器适配器分离**：每个加载器独立子包。SR Launcher 定义 `ModLoaderAdapter` trait，按加载器分别实现。
5. **规则与参数模型**：`CompatibilityRule`、`Argument(s)`、`ExtractRules` 对应官方版本 JSON 的规则系统。SR Launcher 直接实现官方规范，并以真实版本 JSON 做测试夹具。
6. **启动输出泵送**：`StreamPump` 说明 stdout/stderr 需要持续消费，避免管道缓冲区阻塞子进程。SR Launcher 使用 tokio 异步逐行读取并写入环形缓冲与日志文件。
7. **跨平台**：HMCL 基于 JVM，天然跨平台；SR Launcher 需自行处理 Windows/macOS/Linux 的路径分隔符、可执行文件后缀、natives 分类器与参数引号。

## 4. 未采用的做法

- 不复制 GPL 代码（会改变本项目许可策略）。
- 不复制 CC BY-NC-SA 文档文本。
- 不照搬其 UI 结构、图标、文案。
- 不实现 Yggdrasil/外置登录（非首期范围，且涉及第三方服务政策）。

## 5. 对 SR Launcher 的输入

- 模块边界参考：auth / download / game / java / launch / modpack / task。
- 必须自行实现的核心：官方版本 JSON 解析、规则求值、继承合并、资源索引、classpath、参数替换、进程管理、下载调度、校验与原子写入。
- 测试策略：以真实 Mojang 元数据为夹具，离线单测为主，网络集成测试显式标记。

## 6. 待核实事项

- HMCL 下载失败后的具体退避参数：未核验。
- 其缓存仓库的哈希分片布局：未核验。
- 其 Microsoft OAuth 的 client id 配置方式与设备码流程细节：未核验（SR Launcher 将自行查阅 Microsoft 官方文档）。
- HMCL 对旧版 `minecraftArguments` 的兼容边界：未核验。

## 7. 引用

- https://github.com/HMCL-dev/HMCL （GPL-3.0，commit `0d20910ea302`）
- https://github.com/HMCL-dev/HMCL-docs （CC BY-NC-SA 4.0，commit `bc027fc4555c`）
- https://raw.githubusercontent.com/HMCL-dev/HMCL/main/settings.gradle.kts
