# Changelog

## 0.2.5 — 2026-11-13

稳定性、性能与 Bug 修复版本。不新增功能，收敛日志采集、任务取消、文件事务和 Windows 文件替换的安全性。

### Fixed

- 游戏日志采集改为按字节读取并容错非 UTF-8 输出；中文 Windows 的 GBK stdout 不再触发 `InvalidData` 并中断后续全部日志。
- 玩家检查点按真实字节偏移推进，处理多字节字符边界；日志轮转/新会话后将上一会话的“推断在线”玩家降级为离线，消除幽灵在线玩家。
- 依赖分析中的取消和整体截止时间现在会中断正在进行的 HTTP 请求；此前取消只会让请求变成“未解析节点”，分析仍继续。
- 下载在等待响应头阶段即可取消，`Retry-After` 等待也可取消；新增 60 秒无数据停滞超时。
- Windows 下载替换、实例归档导出、模组/材质包/光影导入、FRPC 释放统一使用原子替换；重命名失败不再先删除旧文件。
- 日志分片轮转失败时保留真实字节计数并延迟重试，不再每行重试或错误归零。
- 通用 ZIP 整合包导入改用与 overrides 相同的事务（校验 → 暂存 → 备份 → 提交/回滚），失败不再直接覆盖用户文件。
- 整合包事务清单改为原子写入；恢复时重新校验相对路径、事务目录名和备份完整性，路径穿越或备份缺失会保留材料并报错。
- 导入事务解压同时校验实际写出字节数与压缩比，防御伪造头部和 ZIP 炸弹。
- Release 工作流的 `SHA256SUMS` 现在同时覆盖平台产物和源码归档（源码文件名使用连字符，旧 glob 会遗漏）。

### Changed

- FRPC 遗留进程清理移出 `FrpcManager` 构造函数，改为启动时一次性后台执行；SIGKILL 前重新核对命令行，降低 PID 复用误杀风险。
- FRPC 缓存校验改为流式 SHA-256，不再每次创建房间读取完整 20–40 MB 二进制。
- 下载目标锁在最后一个使用者释放后移除，长时间运行不再累计路径条目。
- 发布版本号更新为 0.2.5（`Cargo.toml` / `Cargo.lock` / `package.json` / `tauri.conf.json`）。

### Performance

- 前端游戏日志事件按 200 ms 批量刷新，缓冲上限 400 条；避免每条 stdout 触发一次全局 React 更新。
- 模组列表使用 `content-visibility: auto`，浏览器跳过屏外行的布局与绘制（不影响筛选与交互）。
- 基线测量（Intel i9-9880H / macOS 26.7 / release）：50,000 行日志解析 33.4 ms，增量读取 52.9 ms，100 个实例扫描 5.7 ms。上述路径未发现需要结构优化的瓶颈。

### Tests

- Rust：264 项单元测试 + 2 项启动集成测试通过；新增非 UTF-8 日志、字节偏移、跨会话玩家状态、轮转失败计数、取消中断在途请求、下载取消、目标锁释放、事务路径校验等测试。
- 前端：50 项测试通过；`tsc -b`、`vite build`、`cargo fmt --check`、`cargo clippy -D warnings` 通过。

### Known limitations

- 未在真实 Windows 设备复测 `MoveFileExW`、优雅退出和崩溃恢复；仅通过代码路径与交叉编译检查。
- 50,000 条日志的 UI 压力场景不适用：日志面板只读取尾部 400 行，完整日志保存在文件中。
- 2,000 条模组的真实 WebView 滚动未做自动化帧率测量；仅实施浏览器级跳过绘制优化。
- 大型整合包解压仍需要等量的临时磁盘空间，这是事务式导入的代价。


## 0.2.4 — 2026-11-12

可靠性修复版本，继续收敛依赖管理、玩家状态、日志、进程身份与联机恢复。

### 依赖管理

- 依赖节点 ID 统一为 `source:project:version`；根节点、普通节点、边和未解析节点使用同一生成函数。
- 生成依赖图后强制校验节点 ID 唯一、所有边端点存在，否则阻止安装。
- 本地模组状态拆分为 `enabled_satisfied` / `disabled` / `version_mismatch` / `version_unknown` / `conflict` / `not_installed`；只有已启用且版本匹配才算满足依赖。
- 前端默认选择改用后端 `selectedByDefault`，并显示安装状态徽章。
- 依赖解析增加 4 路受控并发预取、15 秒单请求超时、120 秒整体 deadline、真实请求计数与缓存去重；支持通过取消键中止分析。
- 未选中可选依赖的必需子依赖不再自动勾选；Incompatible 边不再被节点去重吞掉。

### 玩家状态与日志

- 新增持久化 `PlayerCheckpoint`：日志路径、读取偏移、事件序号、玩家推断状态；增量读取追加日志，重启后可恢复。
- 恢复的在线玩家标记为“日志推断”，不再因为 4 MiB 日志窗口截断而消失。
- 跨午夜日志按相邻事件时间递增日期。
- Minecraft 日志增加 32 MiB 分片轮转，最多保留 5 个历史分片；写入失败产生诊断，不阻塞 stdout/stderr。
- 房间日志继续按文件串行写入，严格 2 MB 字节裁剪。

### 进程与联机

- 保存本地游戏进程启动时间；PID 存活检查同时核对启动时间，降低 PID 复用误判。
- Windows 停止游戏保持“优雅关闭 → 10 秒等待 → 用户确认后强制结束”的流程。
- FRPC 启动时清理本应用私有配置目录对应的遗留进程，不匹配其他用户的 `frpc`。
- 房间增加 `lifecycleState`：`running` / `paused` / `cleanup_required`，UI 显示“已暂停”“待清理”。
- 52FRP 恢复使用绝对 10 秒 deadline；隧道列表分页请求共享 20 秒预算。
- 本地 Java/Bedrock 服务分别使用 Server List Ping / RakNet 严格校验；未知节点负载不再按 30% 计算。

### 文件事务

- 整合包 overrides 使用持久化事务清单，状态覆盖 created/prepared/committing/committed/rolling_back/recovery_required。
- SRL 启动时扫描实例目录，按备份恢复中断事务；恢复失败保留备份并报告路径。
- Windows 原子替换继续使用 `MoveFileExW`，失败不删除旧文件。

### 已知限制 / 需要真实验证

- Windows 优雅退出、`MoveFileExW`、事务崩溃恢复尚未在真实 Windows 设备复测。
- 真实 52FRP 账户、Bedrock 服务端和跨平台联机仍需实机验证。
- 依赖并发为受控预取，不是完全无序并发；取消在批次之间生效，单请求由 15 秒超时约束。

## 0.2.3 — 2026-11-12

稳定性修复版本，保持 v0.2.2 的功能与界面，不新增大型功能。

### 安全与数据完整性

- 实例增加统一生命周期锁：启动、停止中的实例禁止删除、复制、修改配置或导入整合包；并发启动只允许一个进程。
- 整合包 overrides 改为“校验 → 暂存 → 备份 → 提交”事务流程；失败时回滚已提交文件，回滚不完整时保留备份目录并明确报告路径。
- 解压前检查目标目录已有符号链接，拒绝经链接写出实例目录；路径校验统一拒绝盘符、UNC、ADS、冒号、尾随点/空格和 Windows 保留名。
- Windows `atomic_write` 改用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`，替换失败不再先删除旧文件。
- 房间日志写入按文件串行化，裁剪严格按 2 MB 字节上限保留最新行，避免并发追加/裁剪互相覆盖。

### 实例与进程管理

- Windows 停止游戏改为 `taskkill`（不带 `/F`）请求关闭，等待 10 秒；超时后由前端询问是否强制结束，强制结束会提示未保存进度可能丢失。
- 同一实例的启动、删除、复制、配置修改和资源安装共用后端锁，前端禁用按钮仅作为辅助。

### 联机与状态可靠性

- 本地服务检测改为协议级：Java 使用 Server List Ping，Bedrock 使用 RakNet Unconnected Ping；普通 TCP/UDP 端口占用不再被误报为 Minecraft 服务健康。
- 房间状态拆分：FRPC 进程存活、云端隧道在线、本地协议健康分别建模；FRPC 存活但隧道离线/未知时不再显示“联机在线”。
- 52FRP 隧道列表读取全部分页，缺少 `list` 字段时返回错误而不是空列表；查询失败不再被当成“云端没有隧道”。
- 重启 FRPC 后轮询云端隧道状态，10 秒内未确认则返回明确错误，不把进程启动当成隧道恢复。
- 端口检查区分网络/API 错误与端口占用，并增加 15 秒整体预算。
- SRL 捕获日志中的 `[stdout]` / `[stderr]` 包装前缀会在解析前剥离，玩家事件时间戳恢复正常；去重使用有界集合（10,000 条）并包含日志行号，不再在 5,000 条后停止处理。
- 房间状态轮询增加单飞保护与最高 60 秒的指数退避、抖动。

### 模组依赖管理

- 后端独立校验必需依赖闭包，不再依赖前端勾选；漏选必需依赖会被拒绝。
- 已有依赖节点再次以 Incompatible 出现时仍会进入 blocker，不再因节点去重吞掉冲突。
- 依赖节点 ID 包含具体版本；同一项目不同固定版本不再直接合并，无版本约束时才按项目合并。
- 未选中的可选依赖，其必需子依赖不再自动加入默认安装计划。

### 跨平台与发布工程

- Java 自动选择增加 CPU 架构校验；已知 ARM64/x64 不匹配时不会自动选用，实例固定 Java 架构不匹配会明确报错。
- Linux 端口检测在 lsof 不可用或无结果时回退 `ss`，两者都失败时返回空结果而不是误报。
- 52FRP 错误响应保留 HTTP 状态、业务码和限长脱敏摘要。
- SECURITY.md 更新支持版本与漏洞报告流程；发布工作流增加 fmt、test、clippy、前端测试/构建、cargo audit、npm audit 门禁，并校验 tag 与应用版本一致、拒绝覆盖已有 Release。

### 仍需真实验证 / 已知限制

- Windows 优雅退出、`MoveFileExW` 原子替换尚未在真实 Windows 设备复测。
- 整合包回滚的中途进程崩溃恢复、真实磁盘满/权限拒绝场景尚未实测。
- 依赖解析仍以串行请求为主，受控并发/整体取消尚未完成。
- 玩家在线状态仍从最新日志尾部重建，尚未实现持久化增量检查点。
- Minecraft 会话日志尚未实现运行时轮转；PID 复用识别、节点负载不可用标记仍待完善。
- 真实 52FRP 账户、Bedrock 服务器与跨平台联机仍需实机验证。

## 0.2.2 — 2026-11-12

### Minecraft 联机管理中心

- 联机页新增房主 / 游客分段模式，保留原 52FRP 账户、节点、创建、停止、重启与临时隧道清理流程。
- 房主房间卡分开显示：Minecraft 本地服务端口、52FRP 云端隧道在线状态、FRPC 客户端状态、玩家日志确认状态；四者不再混为一个“在线”标签。
- 通过 52FRP `GET /tunnels/{id}` 读取 `is_online`、`enabled`、`cur_conns`、`status`，并对 `cur_conns` 明确标注为 TCP 连接数，不推断玩家人数。
- 新增游客成员列表：只解析 SRL 捕获的 Minecraft 日志中真实的 `joined the game` / `left the game` / `logged in with entity id` 事件，只接受房间创建后的事件。
- 观测 IP 分为 loopback / LAN / public / unknown；`127.0.0.1` 显示为“本地代理连接”，不会冒充游客公网 IP；公网地址默认脱敏，可手动显示完整值。
- 明确部分可用边界：玩家 UUID、真实公网 IP、游戏内 OP / 踢出 / 封禁 / 白名单没有可靠来源时显示未知，不提供空按钮。
- 新增 `multiplayer/room-log.jsonl`：记录房间/FRPC/隧道/网络/玩家事件，含 timestamp、source、level、event_type、message；写入前脱敏，2 MB 上限裁剪，UI 支持筛选、时间排序与清理。
- 游客模式新增 Rust Server List Ping：地址解析（域名/IPv4/IPv6/端口）、DNS、TCP、VarInt、状态 JSON、MOTD、版本、协议、在线人数、玩家样本、RTT 与分阶段错误。
- 新增游客网络监测：12 秒采样（10–60 秒范围），可停止并取消后台任务；区分“网络监测状态”和“游戏内加入状态（无法确认）”。
- 正版验证改为登录握手探测：状态查询后额外建立一次登录连接，加密请求判定 `online-mode=true`，登录成功判定 `online-mode=false`；不发送账号凭据、不进入游戏。正版隔离仍显示未知（52FRP OpenAPI 无该字段）。
- 游客网络监测改为连接诊断完成后才显示；移除“不会点击加入按钮”的说明文案。

### 模组依赖关系可视化

- 新增 `crates/sr-core/src/dependency/`：统一依赖模型、Modrinth/CurseForge 适配器、递归解析、已安装检测与安装执行。
- Modrinth 支持 `required` / `optional` / `incompatible` / `embedded`；CurseForge 支持 `relationType` 1–6，并保留原始平台字段。
- 递归解析优先精确 `version_id`，否则按目标 Minecraft 版本和加载器筛选；稳定版优先；深度上限 16、请求上限 120、节点上限 200；循环引用截断。
- 从 JAR 内 `fabric.mod.json`、`quilt.mod.json`、`META-INF/mods.toml`、`META-INF/neoforge.mods.toml` 读取真实 mod id；CurseForge 使用 Murmur2 指纹；不使用文件名或标题相似度。
- 资源详情新增“依赖”入口：SVG 图形视图 + 列表视图、节点详情、已安装/待安装/不兼容状态、可选依赖勾选、阻止安装的问题、预计下载大小。
- 分析保存到 `cache/dependencies/<id>.json`；安装时前端只提交 `analysisId + nodeIds`，后端重新校验实例、加载器、Minecraft 版本、必需依赖与不兼容关系。
- 安装复用 `mod_install::apply_files`：SHA-1、临时文件、同名备份、原子替换、失败回滚、任务进度与取消。
- 全程不调用 AI 模型推断依赖。

### Java 自动运行环境

- 新增 `java::choose_for_launch` 统一启动选择：实例固定 Java → 精确 major → 用户默认（兼容时）→ 安全范围内最接近版本。
- 实例显式指定的 Java 不再被自动替换；不兼容时返回明确错误并提供改回自动选择的路径。
- 新增 `java_launch_check`：动态返回 required major、兼容范围、当前选择、是否可安装、是否需要重置实例 Java。
- 新增 `install_java_for_version`：缺失 Java 时下载安装 Temurin，安装后重新扫描；默认不修改全局默认 Java。
- 首页/实例页在启动返回 `JAVA_INVALID` / `JAVA_INCOMPATIBLE` 时打开安装提示，安装成功后只继续启动一次，不重复启动。
- Java 环境页手动安装仅在用户没有默认 Java 时设置默认；已有默认不会被新安装顶掉。

### 文档与测试

- 新增 `docs/DEPENDENCIES.md`、`docs/JAVA_RUNTIME.md`，更新 `docs/MULTIPLAYER.md`、`docs/ARCHITECTURE.md`、`research/FEATURE_MATRIX.md`。
- 新增 Rust 测试：Server List Ping/VarInt/地址/MOTD/mock TCP/超时/拒绝、玩家日志事件、日志脱敏、依赖递归/循环/深度/缺失/不兼容、CurseForge 1–6 映射、JAR 元数据读取、Java 8/17/21/25 选择。
- 新增前端测试：依赖图分层布局与空图。
- 真实网络验证：Modrinth Iris → Sodium required（2 节点 / 2 文件 / 4,264,786 字节）；`mc.hypixel.net:25565` Server List Ping 返回协议 47、26854/200000、355 ms。


## 0.2.1 — 2026-11-09

### 界面与语言

- 新增中英双语界面：设置 → 界面语言可在简体中文与 English 之间切换，默认简体中文；切换后立即保存并重新加载窗口。
- 导航、页面、表单、按钮、提示、帮助文案与任务状态全部接入翻译层；缺失条目回退中文，不会显示原始 key。

### 联机

- FRPC 改为安装包内置：按平台/架构携带官方压缩包，锁定 SHA-256，首次使用时校验并解压到应用数据目录；有内置客户端时完全离线，不再访问 GitHub。
- 内置缺失时才回退官方 Release 下载，并保留镜像与 SHA-256 校验。

### 实例与内容

- 新增 Modrinth `.mrpack` 整合包导出：本地模组按 SHA-1 在 Modrinth 识别，命中项写入 `modrinth.index.json`，其余模组与 `config`、`defaultconfigs`、`kubejs`、`scripts`、资源包、光影进入 `overrides/`。
- 导出会读取实例版本元数据确定 Minecraft 版本与加载器依赖；不包含存档、日志、截图或账号令牌。
- 新增 CurseForge 整合包导出：按官方 Murmur2 指纹解析 `projectID` / `fileID` 并生成 `manifest.json`；识别不到或已禁用的模组进入 `overrides/`。
- 实例卡片新增「导出整合包」入口，可选择 Modrinth 或 CurseForge 格式，并设置名称、版本与导出路径。

## 0.2.0 — 2026-11-07

### Minecraft 联机中心

- 新增「Minecraft 联机」页面：账户连接、Java/Bedrock 选择、本地服务检测、节点选择、创建房间、复制地址、停止联机与删除临时隧道。
- 使用 52FRP 开放 API v1：账户、节点、节点负载、名称检查、端口检查、已占用端口、创建/查询/删除隧道与完整 FRPC TOML 配置。
- 52FRP API 访问令牌（`hz_`）只保存在系统凭据库；支持连接、验证、替换与退出，不写入普通配置或日志。
- 节点选择综合 `available`、在线状态、协议支持、隧道配额、TCP 延迟和 `/nodes/load`，不只看延迟；支持手动指定节点。
- 远程端口遵守节点 `proxies_allowed` / `proxies_denied`，避开已占用端口并调用 `check-port`；创建接口无幂等键，SRL 用单飞锁、名称检查和房间记录防止重复提交。
- FRPC 使用 fatedier/frp 官方 Release，按 GitHub 资产 SHA-256 校验；下载优先国内镜像，单源 15 秒超时、总上限 50 秒；已下载客户端缓存优先，连接账户后后台预下载，不再每次创建房间都请求 GitHub；下载进度显示大小与速度。
- 配置来自 52FRP 完整 TOML，写入 0600 临时文件并在停止时删除。
- Java 版使用 TCP，自动识别 Minecraft 局域网动态端口（`Started serving on` 等日志行）与进程监听端口；Bedrock 版使用 UDP，支持默认 19132 与手动端口。
- 创建过程逐步展示真实结果；任一步失败会回滚已创建的隧道并停止 FRPC，避免半成品房间。
- 停止联机与删除云端隧道分离；只允许删除名称以 `SRL_` 开头的 SRL 临时隧道，不批量删除用户其他隧道。
- 异常处理区分本地游戏停止、FRPC 退出、隧道离线、令牌失效、实名/套餐限制、API 限流/401/403、端口冲突与下载损坏。
- 退出应用时停止 FRPC 并清理临时配置；异常退出遗留的 SRL 隧道可在联机页面单独清理。

### 界面

- 联机页压缩为「账户 / 创建房间 / 当前状态」三块，移除大段说明文字；字段解释统一收进旁边的「?」，悬停或键盘聚焦时显示。
- 当前状态旁新增「帮助」按钮，内置完整教程：52FRP 注册、实名、套餐、API 令牌权限、创建房间、朋友加入、离线联机与常见错误。
- 房间卡只突出公网地址、节点、本地端口和延迟；FRPC PID、本地 PID 与日志收进「技术信息」。
- 账号页标注 Microsoft 正版登录暂时需要自行注册 Azure 应用并填写 client ID，后续修复为开箱即用。
- 设置页隐藏重复说明与 Toggle 长描述；账号卡只保留名称、类型、状态与操作。
- 鸦鸭导航和页面标题增加「测试」标签。

### 配置

- 设置页新增 52FRP API 访问令牌管理；令牌存系统凭据库。
- 联机页支持首次粘贴令牌连接，并提供 52FRP 个人中心入口。

### 已知边界

- 需要真实 52FRP 账户与 API 令牌完成在线联调；没有令牌时不能用模拟数据代替。
- 52FRP 账户必须完成实名认证，且套餐覆盖目标节点；SRL 不自动购买套餐。
- Bedrock 联机要求本机已有 UDP 服务在监听；SRL 会检查端口占用，但不替代 Bedrock 服务端本身。

## 0.1.1 — 2026-11-07

### 界面重构

- 全新中性色设计系统：移除紫绿渐变、背景光晕、彩色投影与过度圆角，改为平面化面板、单一点缀色和更克制的层级。
- 侧栏改为「游戏 / 工具 / 系统」三段式导航，当前页使用实心点缀色标记；品牌区、版本信息与主题切换同步收紧。
- 顶栏、卡片、按钮、输入框、标签、进度条、弹窗与提示统一到同一套尺寸、圆角和字重规范。
- 首页启动面板改为顶部细强调线的平面结构，实例信息、启动操作与运行状态更清晰。
- 资源、实例、Java、账号、任务与鸦鸭页面沿用既有功能，仅做视觉与密度整理。
- 增加页面入场、列表错落、导航滑入、卡片悬浮、弹窗/提示出现、进度流光与空状态浮动等动画，并支持 `prefers-reduced-motion`。
- 明暗主题均可使用；深色保持中性黑灰，浅色改为暖白纸面。

### 兼容性

- 不改变任何后端命令、数据目录、实例格式或账号数据。
- 所有既有功能入口保留：版本安装、加载器、资源搜索、实例管理、Java、任务、账号、皮肤、诊断与鸦鸭助手。

### 安装包

- macOS、Windows、Linux 安装包在本地构建并校验，等待实机测试通过后发布。

## 未发布 — 2026-10-28

### 修复 — Java 选择导致 1.21.1 Forge 启动崩溃

- 现象：1.21.1 Forge 52.0.16 启动时报 `Unsupported class file major version 69`。原因是全局默认 Java 被设为 Temurin 25，运行时未再校验目标版本。
- `java::select` 现在只在全局默认 Java 不高于目标版本 2 个主版本时采用它；否则优先精确匹配（如 Java 21）。每实例手动选择的 Java 仍然优先。
- `check_compatibility` 将“比目标新 2 个主版本以上”改为 warning，并给出明确提示；不再把 Java 25 对 Java 21 判为完全兼容。
- Java 页新增「自动选择」按钮，可清除全局默认；安装新 Java 不再自动顶掉已有默认。
- Windows 便携包补齐 EasyTier 运行时（`easytier\windows-x86_64`），`Install.cmd` 会一起复制。
- 修复 Windows 便携包启动后显示 `localhost:1420` 连接失败：交叉编译命令缺少 `custom-protocol` feature，二进制按开发模式加载 dev server。新增 `custom-protocol` feature，并用 `--features custom-protocol` 重建；窗口增加 `--proxy-bypass-list=tauri.localhost;localhost;127.0.0.1`，避免云电脑系统代理拦截。


### 新增 — Structural Connect 跨网络联机

- 侧栏新增「Structural Connect」，位于诊断中心上方；沿用现有主题与组件，不修改其他页面。
- 主机：从该实例最新启动日志解析 Minecraft 自己打印的 LAN 端口，并用真实 Server List Ping 验证；创建房间时 Rust 侧再次验证，前端状态不能替代。
- 网络：以受控子进程运行官方 EasyTier 2.6.4（无 TUN 模式），不创建虚拟网卡；macOS x86_64 真机验证无需管理员权限，其他平台待验证；主机通过 EasyTier 本节点虚拟 IP → 127.0.0.1 重写暴露 Minecraft 端口，客户端通过 `port_forward` 获得本地连接地址。
- 安全：每房间独立网络名/密钥；邀请携带短期临时凭据（非网络密钥，请求 `--reusable false`），有 TTL 且可撤销；主机入站 ACL 默认 Drop，仅放行实际 Minecraft TCP 端口；客户端入站默认 Drop；禁用 UPnP、子网代理、出口节点、socks5、KCP/QUIC 代理；RPC 仅 loopback。
- 策略：默认优先进程直连；严格直连使用 `--p2p-only` 并在只有 relay 路径时明确失败；允许中继必须由用户在加入前显式选择，UI 不静默降级。
- 诊断：运行时/进程/路由/P2P 或中继/Minecraft 状态/共享节点可达性检查；日志脱敏；所有延迟与在线数据来自 EasyTier 或 Minecraft 响应，不填假值。
- 测试：状态机、邀请篡改/过期、配置注入、ACL 生成、进程清理与脱敏、Minecraft 状态协议、管理器持久化；`tests/connect_easytier.rs` 已用官方 EasyTier v2.6.4 在 macOS x86_64 通过（no-TUN + 凭据 + P2P 直连 + port_forward + Minecraft 状态握手 + 非 MC 端口 ACL 隔离 + 未授权网络名/未知凭据不可见；仍是本机 loopback，非跨 NAT）。
- 许可证：EasyTier 为 LGPL-3.0，按独立未修改可执行文件分发；新增 `THIRD_PARTY_NOTICES.md`、`licenses/EasyTier-LGPL-3.0.txt` 与带 SHA-256 校验的 `scripts/fetch-easytier.sh`。
- 明确未实现：自动写入服务器列表、深链接注册、内置公共共享节点、跨启动器重启恢复；跨 NAT 打洞需两台真实设备验证。


## 未发布 — 2026-10-26

### 修复 — 光影版本选择、Java 国内镜像与安装包

- 版本安装弹窗的光影插件新增版本下拉：
  - Iris 按实例加载器过滤 `fabric` 变体，安装所选版本而不是固定最新版；
  - OptiFine 列出该 Minecraft 版本所有非 preview 构建，按所选构建下载并安装（不再忽略选择）；
  - 安装仍复用 `install_loader_resource` 的实例版本/加载器校验和 SHA-1 下载校验。
- Java 一键安装支持国内镜像：
  - 设置中的“国内镜像”开启时，优先使用清华 TUNA Adoptium 镜像下载安装包；
  - 镜像文件先做 HEAD 探测，缺失或不可用时自动回退 Adoptium 官方 GitHub 资源；
  - 版本元数据仍来自 Adoptium API，SHA-256 校验值不变；可信域名新增 `mirrors.tuna.tsinghua.edu.cn`。
- 重新生成 macOS universal DMG 与 Windows x64 便携包：
  - macOS `release/SRL_0.1.0_universal.dmg`，SHA-256 `2a59983e…e57c`；
  - Windows `release/SRL_0.1.0_windows-x64-portable.zip`，SHA-256 `e56bf31a…e375`。
- 测试：Rust 178 单元通过（4 ignored），前端 39 通过；两安装包 clean 检查通过，Windows exe 不再导入 `TaskDialogIndirect`。

## 未发布 — 2026-10-26

### 新增 — 鸦鸭 AI 助手（阶段 1）

- 侧栏新增“鸦鸭”，位于账号管理上方；Java 环境与下载任务顺序调换。
- 本地证据解析（无需 AI）：
  - 读取实例 crash-reports 最新/指定报告与 logs/latest.log；
  - 提取异常类型、消息、Caused by、关键栈帧、原始行号、模组列表；
  - 仅在模组名出现在异常/栈帧行时才标记“可能相关”，不把第一条栈帧当根因；
  - 明确列出解析限制。
- DeepSeek 接入：
  - 设置页配置 API key（系统凭据库）、模型、HTTPS API 地址；支持测试连接、清除。
  - 工具调用白名单：实例诊断、读取 latest.log/debug.log、实例信息、模组列表、Modrinth 搜索；全部只读。
  - 最多 4 轮工具调用，工具参数和结果均有类型/长度限制；无 key 时本地诊断照常工作。
  - 审计日志 `assistant-audit.jsonl` 记录工具、参数、结果摘要和时间。
- 崩溃报告手动删除：诊断中心每条崩溃报告新增删除按钮；后端限制在实例 crash-reports 目录内。
- 修复：诊断中心日志删除不再使用不可靠的 `window.confirm`。
- 安全边界：当前未实现自动修复、模组安装、整合包规划与回滚；界面明确标注“仅只读分析与搜索”。

### 新增 — 鸦鸭安全修复计划（阶段 2）

- 本地生成低风险修复计划（不执行）：
  - 崩溃/日志出现 `OutOfMemoryError` 时，建议提高实例内存（最高 16384 MB）。
  - 疑似模组出现在异常/栈帧行时，建议禁用对应 `.jar`（重命名为 `.jar.disabled`）。
  - 证据不足时明确不生成操作。
- 执行前：用户必须勾选确认；备份 `instance.json` 到 `backups/assistant/<token>/`，写入 manifest；实例配置使用原子写入。
- 失败自动回滚；执行结果提供回滚令牌，可手动回滚内存与模组文件名。
- 只允许 `set_memory` / `disable_mod` 两种白名单操作，不触碰 `saves/`。
- AI 工具新增 `get_repair_plan`：只生成计划并返回证据，不能执行；执行只能由用户在界面确认。
- 测试：内存修改 + 禁用模组 + 回滚的完整流程；证据不足时不生成操作。

### 新增 — 鸦鸭聊天式界面 + 模组依赖安装（阶段 3）

- 界面改为类似 ChatGPT/DeepSeek 的简洁聊天：实例选择、消息流、输入框、快捷问题；移除重复的本地证据面板（诊断中心已提供本地分析）。
- AI 工具新增 `prepare_mod_install`：
  - 通过 Modrinth 官方 API 解析主模组与所有 required 依赖（最多 30 项/8 层）；
  - 校验 Minecraft 版本、加载器、文件 SHA-1 与可信域名；
  - 只生成安装计划，不下载；聊天中展示为可确认卡片。
- 用户确认后执行：
  - 所有文件先下载到隔离临时目录并校验 SHA-1；
  - 同名文件先备份，再原子替换到 `mods/`；
  - 任何失败自动清理并恢复备份；成功后提供回滚令牌。
- 修复计划与模组安装都以聊天内卡片呈现，必须勾选确认后才能执行。
- AI 工具新增 `prepare_modpack_install`：对多个 Modrinth 项目生成统一安装计划，自动合并必需依赖；可审查、可回滚。尚未实现复制实例、启动测试与清单比较。
- 真实验证：Modrinth Sodium + 依赖计划、下载、安装、回滚全流程通过（网络测试）。
- 测试：Rust 180 单元（176 通过，4 ignored）+ 4 集成（其中 1 个真实 Modrinth 安装测试通过）；前端 36 项；fmt/clippy/build 通过。
- 新增崩溃解析（行号/证据/不臆测）、审计日志、设置校验、助手页面与本地证据测试。

## 未发布 — 2026-10-26

### 修复 — 版本安装器可选 Iris/OptiFine 版本

- 版本管理 → 安装 Minecraft 弹窗中，选择 Iris 或 OptiFine 后新增版本下拉框；Iris 仅列出当前 Fabric/Quilt 可用的 Fabric 变体。
- 安装改为调用 `install_loader_resource`，按所选版本装入实例，不再固定装 latest。
- 新增前端回归测试，覆盖 Fabric + Iris 版本选择。

### 新增 — Windows x64 便携测试包

- 使用 `cargo-xwin` 交叉编译出 `sr-launcher.exe`（x86_64-pc-windows-msvc）。
- 打包 `release/SRL_0.1.0_windows-x64-portable.zip`：含 `Install.cmd`、`Uninstall.cmd`、图标与说明；安装到 `%LOCALAPPDATA%\SRL` 并创建快捷方式。
- 未签名、exe 资源不完整（跨平台构建限制）；需要 Windows 10/11 x64 + WebView2。
- 包内无 settings/账号/聊天/Java/实例/日志/`sk-` key；`verify-clean-bundle.sh` 通过。
- 修复 Windows 启动报错 `无法定位程序输入点 TaskDialogIndirect`：Tauri 默认启用 `common-controls-v6`，交叉构建缺少 Common Controls v6 manifest，Windows 加载 v5 后找不到入口。已在 `src-tauri/Cargo.toml` 禁用该 feature 并重建；新 exe 不再导入 `TaskDialogIndirect`。
- 新包 SHA-256：`35c4c01ebba6f8dcdc74aa7761209921826a5c44338e3d63c5441f31dae82188`。
- 正式 NSIS/MSI 仍使用 `.github/workflows/build-installers.yml` 在原生 Windows 构建。

## 未发布 — 2026-10-26

### 打包与默认主题

- 新建配置默认主题改为浅色（`Settings::defaults` 与前端首屏均改为 light）；已有用户设置不被覆盖。
- 生成 macOS universal 安装包：`release/SRL_0.1.0_universal.dmg`（x86_64 + arm64，已用 `lipo` 验证）。
- 新增 `.github/workflows/build-installers.yml`：macOS universal、Linux（deb/AppImage）、Windows（NSIS/MSI）原生 CI 构建。
- 新增 `scripts/verify-clean-bundle.sh` 与 `scripts/verify-no-secrets.mjs`：
  - 安装包不得包含 settings/accounts/instance/日志/皮肤/Java/测试样例；
  - 不得包含 `sk-...` 形式的 DeepSeek key；
  - 已对 macOS universal app 与 DMG 实测通过。
- DeepSeek key 仍只存系统凭据库，不进入源码、settings.json、日志或安装包。

## 未发布 — 2026-10-26

### 调整 — 设置关于与两处排版

- 设置页“关于”仅保留一行：`SRL (Structural Recomposition Launcher) V0.1.0 结构重组`；移除功能可用性列表与免责声明区块。
- 诊断中心“网络检查”改为与按钮同高的胶囊式开关，修正与“重新检查/导出报告”的对齐。
- 版本管理工具栏“下载资源文件（约数百 MB）”同样改为胶囊式开关；安装版本弹窗中的资源选项改为带标题与说明的整行选项，避免文字拥挤。

## 未发布 — 2026-10-26

### 新增 — Microsoft 正版登录配置与账号刷新

- 账号页新增 Azure client id 配置向导：直接粘贴 Application (client) ID，保存后立即启用正版登录；内置 Azure 应用注册步骤和门户入口。
- 设备码登录弹窗新增“打开浏览器”，可一键打开 Microsoft 验证页；复制代码/地址保持不变。
- 已登录的 Microsoft 账号新增“刷新授权”，用 refresh token 重新走 Xbox/XSTS/Minecraft 交换并更新账号状态。
- 登录链路（device code → token → Xbox Live → XSTS → Minecraft profile）已完整实现并通过模拟全链路测试；真实登录仍需要项目所有者自己的 Azure 应用 client id（Microsoft 不允许第三方共用/伪造）。
- Rust 181 单元（177 通过，4 ignored）+ 4 集成；前端 37 项。

## 未发布 — 2026-10-26

### 改进 — 鸦鸭诊断能力、无限工具调用、聊天记录

- 崩溃分析增强：
  - 崩溃时间、Description、Minecraft/Java 版本、加载器；
  - 完整异常列表、Caused by 链、关键栈帧与原始行号；
  - 更完整的 Forge/Fabric 模组列表解析（支持 `.jar)` 与制表符格式）；
  - 故障信号识别：OOM、Java 版本、Mixin、重复模组、缺失依赖、NoSuchMethodError、NoClassDefFoundError、GLFW/OpenGL、服务端 tick、世界/区块、注册表/数据包等；
  - 日志分析增加错误行上下文（±2 行）、日志内模组列表、故障信号，读取 latest.log 失败时回退 debug.log。
- AI 工具调用改为无轮次上限（`loop`），不再限制 20 轮；工具结果上限 60000 字符。
- 聊天记录持久化：
  - 每个对话保存为 `<app data>/assistant/chats/<id>.json`，包含消息、工具轨迹、修复/安装提案；
  - 左侧历史列表，可新建、切换、删除单个对话；
  - 对话上下文由后端保存并自动带入（最多 100 条 / 18 万字符，避免无限膨胀）。
- 前端补聊天记录恢复/删除测试；Rust 181 单元（177 通过，4 ignored）+ 4 集成；前端 37 项。

## 未发布 — 2026-10-26

### 改进 — 鸦鸭回复格式与模组搜索

- 聊天消息支持 Markdown 渲染：加粗、斜体、行内代码、代码块、标题、有序/无序列表、引用和简单表格；不再直接显示 `**`、`##` 等符号。
- 系统提示要求回答简洁：先结论，默认不超过 6 句或 5 个要点，避免复述长日志和大表格。
- `search_mods` 升级为资源页式搜索：支持实例默认版本/加载器、分类、排序、分页，单次最多 30 条；提示模型使用同义词、分类和多次搜索对比。
- 移除原先最多 4 轮工具调用的低限制，提高到 20 轮（仅作为防失控保护）；工具结果上限提高到 60000 字符，历史上下文提高到 30 条。
- 测试补充：Markdown 加粗/列表渲染。

## 未发布 — 2026-10-26

### 改进与修复 — 日志清理、26.x 皮肤、Fabric/Quilt 安装

- 诊断中心日志不再无限增长：
  - 启动时自动删除 14 天前的日志；总量超过 200MB 时从最旧的开始清理（最近 1 小时内的文件受保护）。
  - 日志列表新增“清理旧日志”和单条删除；删除改为应用内直接执行，不再依赖 Tauri 中不可靠的 `window.confirm`。
  - 读取日志只读取最后 2MB，超大日志不会整体载入内存。
- 修复 Minecraft 26.x（如 26.3）离线皮肤不生效：
  - 26.x 客户端的资源包格式在 `version.json` 的 `pack_version.resource_major` 中（26.3 为 97），旧逻辑把 26.x 当成 `pack_format 15`，皮肤包被客户端判定不兼容并移出启用列表。
  - 现在从客户端 JAR 读取实际格式；26.x（format ≥ 65）只写 `pack_format` / `min_format` / `max_format`，不再写 `supported_formats`（26.x 会把它当作解析错误并移除资源包），旧版本才写 `supported_formats`。
- Fabric / Quilt 安装与 Forge 对齐：
  - 安装向导中即使基础版本还没安装，也能正常获取 Fabric / Quilt 加载器版本。
  - 开始安装时会像 Forge 一样先安装 Minecraft 本体，再安装加载器。
- 加载器入口迁移到“资源”：
  - 移除侧栏“模组加载器”页面。
  - 资源页新增“加载器”标签：Fabric / Quilt / Forge / NeoForge / Iris / OptiFine。
  - 自由选择任意 Minecraft 版本与加载器版本，然后“安装到实例”或“下载到本地”。
  - 安装到实例会安装加载器并把实例切换过去；Iris/OptiFine 直接写入实例 `mods/`。下载到本地保存 profile JSON / 官方安装器 JAR / Iris-OptiFine 文件。

### 新增 — Java 运行时一键下载安装

- Java 环境页新增“下载并安装 Java”卡片：可选择 Java 8–27（LTS 标注、推荐版本），一键下载安装。
- 来源 Eclipse Adoptium（Temurin）官方 API；安装包按 SHA-256 校验，解压到启动器数据目录 `java/`，不修改系统 Java 或 `JAVA_HOME`。
- 安装完成后自动加入 Java 列表并设为默认；下载/校验/解压进度接入任务系统。
- 平台与架构：macOS / Linux / Windows，x64 / arm64（按 Adoptium 可用构建）。
- 安全：仅允许 HTTPS 与可信域名（github.com / objects.githubusercontent.com）；拒绝路径穿越、绝对路径、符号链接与超限压缩包；解压上限 4GB。
- 真实验证：Java 21 完整下载（约 194MB）+ SHA-256 + 解压 + 探测通过；Rust 173 单元（169 通过，4 ignored）+ 3 集成（1 个真实下载 ignored 通过）；前端 33 项通过。

### 新增 — CurseForge 全资源搜索 + 内置公共 API key

- “资源”页新增来源切换：Modrinth / CurseForge。模组、整合包、光影、材质均可通过 CurseForge 搜索，地图固定走 CurseForge。
- CurseForge 搜索支持分类、Minecraft 版本、加载器（Forge / Fabric / Quilt / NeoForge）、排序（相关度/下载量/最新/更新）与分页。
- 详情与版本列表使用 CurseForge 真实数据：文件大小、SHA-1、MC 版本与加载器从 `gameVersions` 中拆分，避免把 `NeoForge`/`Client` 当成游戏版本。
- 安装：模组/光影/材质写入实例对应目录；整合包下载 ZIP 后按 `manifest.json` 流程安装；地图解压到 `saves/`。均支持“下载到本地目录”。
- CurseForge API key 改为内置公共配置（随启动器发布），所有用户开箱即用，无需申请。设置页仍可填入个人 key，优先于内置 key 使用，便于获得独立配额。
- 修复：默认使用内置 key 时完全不访问系统钥匙串，搜索不再弹出“SRL 想要使用钥匙串中的机密信息”；只有显式保存个人 key 后才会读写钥匙串（以本地标记文件启用）。
- 真实验证：classId 6/4471/12/6552/17 搜索均返回真实结果（模组 1.21.1 + NeoForge + 科技分类 2,929 条；整合包 10,000+；材质 9,214；光影 554；地图 723）。
- 测试：Rust 164 单元（161 通过，3 个网络测试忽略）+ 2 集成；前端 31 项通过；fmt/clippy/npm build 通过。

## 未发布 — 2026-10-25

### 新增 — CurseForge 地图搜索

- “资源 → 地图”已接入 CurseForge 官方 API（classId 17），不再是静态提示；需要先在设置页配置 CurseForge API key。
- 支持地图分类（冒险、生存、解谜、建筑/创作、跑酷、游戏地图、模组世界）、Minecraft 版本筛选、排序（相关度/下载量/最新/更新）和分页加载。
- 详情弹窗显示作者、分类、下载量、支持版本；版本列表按所选 Minecraft 版本过滤。
- 安装到实例：下载 ZIP 并校验 SHA-1，解压到实例 `saves/<世界名>/`；自动识别单一世界根目录，同名存档自动追加 `-2`、`-3`，不覆盖已有存档。
- 下载到本地：只下载地图 ZIP，不解压。
- 安全：仅接受 `.zip`，要求 SHA-1；拒绝路径穿越、绝对路径、符号链接、Windows 保留名、压缩比异常、解压超过 8GB 或 5 万文件；多个世界存档的 ZIP 会明确拒绝。
- 来源限制：Modrinth 没有世界/地图项目类型，地图搜索明确使用 CurseForge；无 key 时提示去设置页配置，不伪造结果。
- 真实验证：CurseForge 1.21.1 地图搜索返回 723 条结果；真实下载 `Lucky OneBlock 1.21-1.21.1 (v1.1).zip`（11,276 B，SHA-1 通过），解压出 `level.dat` 与 `datapacks/`，并在临时实例中验证存档目录结构。
- 测试：Rust 161 单元（158 通过，3 个网络测试忽略）+ 2 集成通过；前端 30 项通过；fmt/clippy/npm build 通过。


### 新增 — 本地整合包 ZIP 支持

- “导入本地整合包”现在接受 `.zip` 和 `.mrpack`，不再只限 Modrinth 格式。
- 自动识别三种内容：
  - `modrinth.index.json`：Modrinth `.mrpack`（即使扩展名是 `.zip` 也可）。
  - `manifest.json`：CurseForge 整合包（`minecraftModpack`），读取 MC 版本/加载器并下载清单文件。
  - 其他 ZIP：按普通整合 ZIP 解压到实例目录；自动去掉单一顶层包装目录，跳过不安全的路径/符号链接/`instance.json`，`options.txt` 覆盖前备份。
- CurseForge：
  - 设置页新增 CurseForge API key（保存在系统凭据库，不写入 settings.json/日志），支持保存、测试、清除。
  - 有 key：逐个解析 `projectID/fileID`，从 CurseForge API 获取文件名、SHA-1、下载地址，校验后放入实例 `mods/`。
  - 无 key：可勾选“仅导入 ZIP 内已有文件”，只解压 overrides；未勾选时明确错误提示需要 key。普通 ZIP 和 `.mrpack` 不受影响。
- 修复 keyring 未启用原生存储：`keyring = "3"` 之前没有平台 feature，实际使用 mock（写入返回成功但读不到）。现按平台启用 `apple-native` / `windows-native` / `sync-secret-service`，CurseForge key 与 Microsoft 令牌才能持久化到系统凭据库。
- 真实验证：CurseForge API `games/432` 返回 200；真实文件解析（ATM10 8.3，SHA-1、200MB、`edge.forgecdn.net`）通过。
- 测试：Rust 155 单元 + 2 集成通过；前端 29 项通过；fmt/clippy/npm build 通过。

### 新增 — 社区资源页（Modrinth）

- 侧栏新增“资源”，位于“版本管理”下方。
- 搜索源：Modrinth 官方 API（`api.modrinth.com`），文件走官方 CDN，下载前校验 SHA-1；不执行任何下载文件。
- 支持类型：模组、整合包、光影、材质。
- 筛选：Minecraft 版本（正式版/快照分组）、加载器（Fabric/Forge/Quilt/NeoForge）、中文标签（冒险/魔幻/科技/世界生成/优化/生存/恐怖等，按类型分别定义）、排序（相关度/下载量/关注/最新/更新）。
- 详情弹窗：图标、简介、下载/关注、许可证、支持加载器、兼容版本列表；版本按当前 Minecraft/加载器过滤。
- 安装：
  - 安装到实例：模组 → `mods/`，光影 → `shaderpacks/`，材质 → `resourcepacks/`（并自动写入 options.txt 启用）；校验实例的 MC 版本/加载器。
  - 下载到本地：选择目录，仅下载文件。
  - 整合包：下载 `.mrpack` 到本地，或安装到匹配的实例（解析 `modrinth.index.json`，下载 50+ 文件并解压 `overrides/`/`client-overrides/`）；失败/取消会写入任务状态。
- 本地整合包导入：支持 Modrinth `.mrpack`（文件选择/拖拽/手动路径），需选择匹配实例；CurseForge ZIP 暂未接入（需 API key）。
- 进度：复用下载任务系统，任务页可见；关闭安装弹窗不会取消任务。
- 安全：仅使用可信 host（Modrinth API/CDN、GitHub/ForgeCDN 作为 modpack 外部文件源）、HTTPS、SHA-1 校验；modpack 路径拒绝 `..`/绝对路径/反斜杠/符号链接/Windows 保留名/`instance.json`，解压上限 8GB & 5 万文件，覆盖 `options.txt` 前自动备份。
- 地图：Modrinth 没有世界存档/地图项目类型；当前“地图”页如实提示，不伪造在线地图结果。在线地图需要 CurseForge API key，后续可接入。
- 验证：真实 Modrinth 搜索（Sodium，1.21.1/Fabric）、项目/版本、真实 Sodium 文件下载并 SHA-1 匹配、真实 Fabulously Optimized `.mrpack` 索引解析通过。
- 测试：Rust 155 单元 + 2 集成通过；前端 29 项通过；fmt/clippy/npm build 通过。

### 新增 — 实例地图与材质管理

- 实例卡片在“复制实例”后新增“地图”“材质”两个按钮。
- 地图：
  - 读取实例 `saves/` 下的真实世界目录（要求存在 `level.dat`）。
  - 显示世界文件夹、区域文件数量、占用空间、最后保存时间；按最后保存时间倒序。
  - 支持打开存档根目录与单个世界文件夹；不提供删除/重命名，避免误操作。
  - 只读，不修改世界文件。
- 材质（资源包）：
  - 列表包含 `resourcepacks/` 下的 `.zip` 与文件夹，启用状态来自 `options.txt` 的 `resourcePacks` JSON 数组。
  - 支持拖拽、文件选择、手动路径导入 `.zip`；导入前校验：根目录 `pack.mcmeta`、`pack_format`、路径穿越、符号链接、解压后 4GB 上限、压缩比异常。
  - 启用/禁用、删除、打开文件夹；只修改 `resourcePacks:` 一行，保留其他设置；JSON 非法时保持原样并报错。
  - `SRLocalSkin.zip`（离线账号皮肤）标记“皮肤自动管理”，在材质管理中只读，避免误删皮肤。
- 验证：真实实例读取到 `test` 世界（11.2MB、12 个区域文件）与 `SRLocalSkin.zip`；Rust 142 单元 + 2 集成通过，前端 27 项通过；fmt/clippy/npm build 通过。release `SRL.app`/DMG 构建并安装到 `/Applications/SRL.app`。

### 新增 — 账号皮肤

- 账号管理新增皮肤设置：创建离线档案时可直接选皮肤，已有账号可随时“更换皮肤”。
- 皮肤来源：
  - 内置默认：从本机已安装客户端 JAR 的 `assets/minecraft/textures/entity/player/{wide,slim}/*.png` 提取 9 个官方默认角色 × 标准/纤细，共 18 个；未安装 1.20.2+ 时给出明确提示，不内置 Mojang 素材。
  - 本地导入：支持 64×64、128×128 与旧版 64×32 PNG；128×128 保留原图并生成 64×64 兼容副本，64×32 按官方面镜像规则转换为 64×64 布局。
  - 正版玩家：通过 Mojang 公开档案 API 按玩家名获取当前皮肤与模型，不登录该账号；已用 Notch 真实查询验证（旧版 64×32 皮肤可正确转换）。
- 每个皮肤可独立选择标准（4px 手臂）/纤细（3px 手臂）；账号可覆盖皮肤自带模型。
- 应用方式：
  - Microsoft 账号：调用 `POST /minecraft/profile/skins` 官方接口上传，写入账号皮肤服务（需要有效 Minecraft 令牌；未用真实令牌做线上验证）。
  - 离线档案：启动时生成并启用 `resourcepacks/SRLocalSkin.zip`，覆盖默认皮肤贴图；仅本机可见、会覆盖所有默认皮肤玩家，且无法改变手臂模型。可在设置中关闭。
- 新增皮肤库：本地导入与正版玩家皮肤留存、预览、删除；删除时自动清除账号引用。
- 新增安全校验：PNG 签名/尺寸白名单、2 MB 上限、解码前尺寸检查、皮肤 ID 白名单、皮肤下载域名白名单（`textures.minecraft.net`）、上传前 64×64 规范化。

### 依赖

- `png 0.18`：PNG 解码/编码与尺寸校验（MIT OR Apache-2.0）。
- `base64 0.22`：解码 Mojang 公开档案中的 textures 属性（MIT OR Apache-2.0）。
- 两者仅在本地处理字节，不新增系统/网络权限。

### 修复

- 修复输入框“打一个字光标消失”：Modal 的聚焦 effect 依赖内联 `onClose`，每次父组件渲染都会重新执行，把焦点从输入框移到右上角关闭按钮。现改为 ref 保存回调、effect 只依赖 `open`，并优先聚焦正文输入框；补充回归测试。

### 修复 — OptiFine + 新版 Forge 进入世界崩溃

- 现象：`1.21.1-forge-52.1.20` + `OptiFine_1.21.1_HD_U_J1`，主菜单正常，进入存档后服务端线程崩溃：
  `NoSuchMethodError: 'java.util.List net.minecraft.core.Direction.getUpdateOrder()'`，调用链为
  `VaultBlockEntity.tick -> BlockEntity.setChanged -> Level.updateNeighbourForOutputSignal`。
- 根因：Forge 1.21.1 分支在 2025-03-04 加入 Forge 专用 `Direction.getUpdateOrder()` 并改写 `Level`；OptiFine J1（2024-09-24）的 `Direction` xdelta 补丁基于旧类，运行时覆盖后方法消失。OptiFine 元数据明确标注 J1 对应 **Forge 52.0.16**。
- 修复：安装 `1.21.1-forge-52.0.16`，将实例 `fbcc2eb57bd8` 切换到该版本；世界、模组、皮肤资源包均保留。实机进入 `test` 世界运行约 106 秒无崩溃，正常退出（code 0）。
- 启动器防护：`install_optifine` 现在读取 OptiFine 元数据的 `forge` 字段；当前实例 Forge 高于 OptiFine 编译版本时拒绝安装，并提示应使用的 Forge 版本。新增 `optifine_forge_requirement`、`version_gt` 单测。

### 测试

- Rust：134 单元通过 + 2 集成通过，3 项真实网络测试默认 ignored；其中真实 Mojang 皮肤查询（Notch，64×32）手动通过。
- 前端：26 项通过（新增输入焦点回归、内置皮肤选择与创建参数两项）。
- `cargo fmt --check`、`cargo clippy --all-targets -D warnings`、`npm run build` 通过。
- `npx tauri build --debug --no-bundle` 与 release `SRL.app` 构建通过；release 安装到 `/Applications/SRL.app` 并实际启动成功（17:44 的 DMG 打包因挂载盘忙失败，不影响 `.app`）。
- 真实 1.21.1 客户端 JAR 提取 18 个默认皮肤、生成皮肤资源包与 `options.txt` 通过。
- 未验证：皮肤在游戏内的实际渲染（无法在无 GUI 环境截图）、Microsoft 真实令牌上传。

## 未发布 — 2026-10-24

### 新增

- 版本页改为“下载版本 / 已安装”双视图：下载列表按正式版、快照、旧版 Beta、旧版 Alpha 分栏并显示数量，顶部提供最新正式版/快照入口。
- 已安装版本直接读取 `versions/`：显示类型、加载器、占用空间、修改时间、继承关系与实例占用；支持打开文件夹、创建实例、完整性检查、详情与删除。
- 版本删除带使用检查：被实例使用的版本拒绝删除；被加载器继承的版本需显式确认并同时删除依赖版本。
- 模组管理支持原生文件选择、拖拽批量导入、原始字节 IPC（无 base64）、覆盖开关、打开 `mods/` 文件夹。
- 新增光影管理：`shaderpacks/` 列表、拖拽/选择导入、启停、删除、打开文件夹，并检测 Iris/OptiFine。
- 新增从 Modrinth 安装 Iris：按 Minecraft 版本与 Fabric/Quilt 选择兼容文件，SHA-1 校验后原子写入 `mods/`；未配置加载器时明确拒绝。
- 新增受限的“打开目录”命令（游戏目录、versions、单个版本、实例、mods、shaderpacks、saves），前端不能传任意路径。

### 新增（23:30）

- 版本页修复下载列表：Mojang 清单字段是 `type`，前端之前误读 `versionType`，导致正式版/快照筛选为空；现已按 `type` 渲染。
- 加载器页新增「按版本推荐」：根据已安装版本并行检测 Fabric、Quilt、Forge、NeoForge、Iris、OptiFine，显示推荐版本、状态与说明。
- Fabric/Quilt 推荐项可一键开始安装；Forge/NeoForge/OptiFine 提供官方安装器下载入口（不静默执行第三方安装器）。
- 新增受限 `open_trusted_url` 命令，只允许 BMCLAPI、NeoForge Maven、Forge、OptiFine 官方域名。
- Quilt 构件增加 BMCLAPI/USTC 镜像改写（`hashed` 映射保留官方源），真实 1.21.1 + Quilt 0.20.0-beta.9 安装通过（77 库、shared jar、完整性检查通过）。

### 修复

- 修复点击“版本管理”时，若存在历史已完成安装任务会导致无限 reload/重挂载、界面卡死的问题：版本页改为独立调用 `list_installed_versions`，不再在任务完成回调里触发全局 reload。
- 加载器页改为选中基础版本后自动获取加载器列表；增加 15s 请求超时与重试，避免网络异常时长时间无反馈。
- 新增 BMCLAPI/USTC 镜像回退（设置中默认开启）：Fabric 元数据走 BMCLAPI，构件走 USTC BMCLAPI 镜像；真实 1.21.1 + Fabric 0.19.5 安装通过。
- 删除/继承检测兼容已解析的加载器版本（保存后的元数据不再携带 `inheritsFrom`，通过 id 后缀匹配基础版本）。
- 修正启动器功能状态中“模组加载器不可用”的过期文案。

### 测试

- Rust：120 项单元 + 2 项启动集成通过（网络测试默认 ignored）。
- 真实镜像安装：1.21.1 + Fabric 0.19.5（71 库）与 Quilt 0.20.0-beta.9（77 库），shared jar、`verify_installed` 均通过。
- 前端：24 项通过；`npm run build`、`cargo clippy -D warnings`、Tauri debug 构建通过。

### 已知限制

- 当前环境无法访问 Modrinth，Iris 真实下载未线上验证；错误会原样展示，不会伪造成功。
- 光影支持检测基于已启用模组文件名，不解析 jar 内容。
- Windows/Linux 仍未实机构建验证。

## 0.1.0 — 2026-10-23

首个可运行版本（阶段 1，含阶段 2/3 的核心能力）。

### 新增

- Tauri 2 + React 19 + TypeScript + Vite 工程骨架。
- 独立设计系统（深/浅主题、统一 token、响应式侧栏）。
- 9 个页面：首页、实例、版本、任务、Java、账号、加载器、诊断、设置。
- 官方版本清单获取、6 小时缓存、断网回退。
- 完整版本 JSON 模型：继承合并、规则求值、natives、资源索引、旧版参数。
- 下载引擎：并发、取消、重试退避、SHA-1 校验、原子写入、任务进度事件。
- 安全断点续传：`If-Range` + `Content-Range` 校验；无验证器时回退整包；中断保留临时文件。
- 实例导出/导入：安全 ZIP 校验（穿越、重复、炸弹、符号链接、Windows 保留名），排除日志与账号数据。
- 模组文件管理：列表、启用/禁用（`.disabled`）、删除、导入；原子复制并拒绝符号链接。
- 版本安装：客户端、库、natives 解压、资源索引、可选资源文件、最终校验。
- Java 自动发现与版本解析、兼容性检查。
- 启动引擎：classpath、占位符替换、进程参数数组、stdout/stderr、退出码、SIGTERM 优雅停止。
- 多实例 CRUD、复制、删除（可选删除文件）。
- 离线账号档案、系统凭据库封装。
- Microsoft 设备码 OAuth、Xbox Live/XSTS、Minecraft 服务交换、令牌 keyring 存储与刷新；未配置 client id 时明确禁用。
- 每实例独立账号选择与窗口分辨率；启动参数自动追加 `--width/--height`。
- Fabric 加载器：官方 meta、继承解析、Maven `.sha1` sidecar 校验、共享原版 JAR（真实 1.20.4 安装验证）。
- Quilt 加载器：官方 meta、继承解析、安装计划（单元测试通过；真实库下载因网络限制未完成验证）。
- 诊断中心：环境检查、日志读取/脱敏、报告预览导出。
- 崩溃报告分析：OOM、Java 版本、Mixin、重复模组、API 不兼容、图形驱动、依赖冲突分类。
- 任务历史持久化：重启后将未完成任务标记为中断；跨任务全局下载限流。
- 100 项 Rust 单元测试、2 项启动集成测试、14 项前端测试、真实 1.12.2 与 1.20.4+Fabric 安装冒烟测试。

### 修复

- 修复安装任务进度字节被重复累加的问题（绝对进度与增量进度混用，曾出现 4.4GB/612MB）；已跳过文件也计入进度。
- 版本页明确标注资源文件体积与可选性。

### 已知限制

- 真实 Microsoft 授权尚未用线上 client id 验证；Forge/NeoForge、通用整合包、更新系统未实现。
- 任务历史持久化；中断任务不会自动重启，但重新安装时可复用可验证的断点文件。
- 仅 macOS 实机验证。

## 未发布 — 2026-10-25

### 新增

- Forge / NeoForge 直接安装：按已安装版本匹配官方安装器版本，用户确认后由启动器运行官方安装器；自动创建 `launcher_profiles.json`、预下载 BMCLAPI/USTC 镜像库、运行处理器、压平继承关系并写入可启动版本。
- 真实验证：1.21.1 + Forge 52.1.20（100 库）与 NeoForge 21.1.264（99 库），`verify_installed` 通过。
- 图标与 UI logo 更换为用户提供的 PNG；`tauri icon --fit cover` 重新生成 macOS/Windows/iOS/Android 图标，前端侧栏/启动页使用同一 logo。

### 修复

- 下载版本时新增合并安装弹窗：可选择原版 / Fabric / Quilt / Forge / NeoForge，并可同时选择 Iris / OptiFine 与创建实例。
- 修复加载器版本创建的实例未记录 `modLoader`，导致模组页误报“原版实例”的问题；现在自动从版本 ID 识别。
- 启动器简称改为 SRL（窗口、侧栏、应用包、README）。
- Forge/NeoForge 推荐卡增加加载器版本下拉，可选择具体版本安装。
- 进度条：字节已满但文件数未完成时切换为不确定进度，修复“满了还在下载”。
- OptiFine 在 Forge 实例中作为模组直接安装到 `mods/`，不运行图形安装器。
- 修复 Forge/NeoForge 点击安装无响应：移除 Tauri WebView 中不稳定的 `window.confirm`，改为直接调用后台安装任务。
- Forge/NeoForge 推荐列表支持选择具体加载器版本，不再只取最新版。
- 修复进度条字节已满但仍在下载：当文件数未完成时进度条改为不确定状态，避免“100% 还在下载”。
- 启动器名称/窗口/应用包改为 SRL。
- Forge/NeoForge 安装后压平 `inheritsFrom`，避免启动器启动时缺少父版本库/客户端 JAR。
- 加载器推荐 Forge/NeoForge 改为可安装，不再只提供下载按钮。
