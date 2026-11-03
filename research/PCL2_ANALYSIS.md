# PCL2 研究报告

- 研究日期：2026-10-23
- 研究对象：`tangge233/PCL2`（用户提供链接）
- 上游来源：`Meloong-Git/PCL`（该 fork 的 parent/source，README 中链接的官方仓库）
- 核验提交（上游 `main`）：`6b9a50df911c`，2026-10-20T06:51:42Z
- 许可证：`NOASSERTION`（GitHub 无法识别为标准许可证），仓库根文件名为 `LICENCE`

## 1. 来源与许可证核验

GitHub API 显示 `tangge233/PCL2`：

- `fork: true`，`parent: Meloong-Git/PCL`，`source: Meloong-Git/PCL`
- `stargazers_count: 4`，创建于 2022-11-20，默认分支 `main`
- 描述为 "Plain Craft Launcher 2（我的世界启动器 PCL2）的源代码库。"
- 许可证字段 `key: other`、`spdx_id: NOASSERTION`

`Meloong-Git/PCL` 为 `fork: false`，8,896 stars，README 指向作者（龙腾猫跃）的发布页与爱发电页面，可认定为事实上的官方源码仓。用户给出的 `tangge233/PCL2` 是二手 fork，研究时应以上游为准。

仓库根 `LICENCE` 不是 OSI 许可证，而是两份自定义文本：

1. 《PCL 分发有限许可》
2. 《PCL 存储库合理使用指南》（文本自述"不是法律文件或协议"）

关键限制（原文摘要）：

- 仅授予"原样副本"的存储、镜像、备份、上传、下载、分发权利；**不授予修改权**。
- 不得"以软件为收费主体"进行收费。
- "重度使用"包括：基于该库修改；或参考较大量的内容以重新实现 PCL 的实质功能（原文举例：启动/下载 Java 版 Minecraft、下载或管理 Mod）。
- 重度使用要求：名称必须以 `Plain Craft Launcher (PCL)` 开头并加第三方后缀、关于页首位署名并公开源码、启动器需实现同等或更强的正版购买劝导、Windows 上主色相必须为蓝色等。

**结论：PCL2 不是可自由复用的开源项目。** SR Launcher 不复制其代码、图标、文案、布局或素材，也不以"改写变量名"的方式移植实现。本项目的 PCL 研究仅基于公开 README、可观察的产品行为、公开帮助文档与社区讨论，属于独立实现前的功能层面研究。为避免落入其"重度使用"定义，SR Launcher 不声称是 PCL 衍生作品，不复用其名称、品牌或 UI 资产，并采用完全不同的技术栈（Rust + Tauri + React）与架构。

> 注：以上是对公开许可证文本的工程性解读，不构成法律意见。

## 2. 可确认的产品能力（来自 README 与公开资料）

README 明确说明仓库"包含 PCL 的大部分源代码，包括 UI 库、动画模块、下载模块、Minecraft 启动模块等"，且代码在每次 PCL 发布时同步，不实时更新。

公开可见的能力类别（功能层，不涉及内部实现）：

- Minecraft 原版与模组加载器安装
- 多版本/多实例管理
- 下载与文件校验、失败重试
- Java 自动搜索与选择
- 正版/离线账号档案
- 整合包安装与导出
- 资源、模组、存档管理
- 内置帮助与错误诊断
- 启动前检查与正版购买提示（许可证中亦明确提及 `McLaunchPrecheck`）

## 3. 架构观察（仅限可公开确认的信息）

- 技术栈为 .NET / WPF（仓库含 `.sln`、`PCLCS`、`MeloongCore`、`patches`、`最新正式版.zip`），面向 Windows。
- 未在公开资料中确认其模块边界、下载调度算法、元数据缓存结构等内部细节，**标记为"待核实"**。
- 仓库含 `MeloongCore` 与 `PCLCS` 子模块，说明存在核心库与启动器壳的分离；具体职责未逐一核验，**待核实**。

本报告不推测其私有实现。SR Launcher 的相关设计均来自 Minecraft 官方元数据规范与通用工程实践，而非 PCL 内部代码。

## 4. 可借鉴的产品经验（抽象层面）

1. **启动前检查前置化**：把 Java、文件完整性、账号状态问题在启动前集中暴露，而不是等进程崩溃。
2. **错误信息面向解决**：区分"发生了什么 / 影响是什么 / 用户可以怎么做"。
3. **下载可视化**：文件数量、总字节、速度、失败原因都要可见。
4. **本地优先**：已安装版本在断网时仍可启动。
5. **诊断入口固定**：日志、环境信息、崩溃报告集中在一处。

以上为通用产品原则，不涉及复制其实现或视觉设计。

## 5. 不采用的做法

- 不复制其 XAML/C# 代码或资源字典。
- 不模仿其品牌色、图标、启动动画与页面布局。
- 不使用 "PCL"、"Plain Craft Launcher" 作为 SR Launcher 的名称、副标题或兼容性暗示。
- 不基于反编译产物进行实现。

## 6. 对 SR Launcher 的输入

- 功能范围参考：版本管理、下载、Java、实例、账号、整合包、诊断（见 `FEATURE_MATRIX.md`）。
- 非功能要求：离线可用、错误可诊断、下载可取消/重试、文件校验。
- 技术路线：**不复用**，采用 Tauri 2 + Rust + React 的独立实现。

## 7. 待核实事项

- PCL2 内部下载并发模型与重试策略：无公开权威说明。
- 其版本继承合并的具体规则：未核验。
- 其整合包格式兼容矩阵：未核验。
- 许可证中"重度使用"条款在司法辖区的具体效力：需法律专业人士判断。

## 8. 引用

- https://github.com/tangge233/PCL2 （fork，4 stars，NOASSERTION）
- https://github.com/Meloong-Git/PCL （上游，8,896 stars，commit `6b9a50df911c`）
- https://raw.githubusercontent.com/Meloong-Git/PCL/main/LICENCE
- https://raw.githubusercontent.com/Meloong-Git/PCL/main/README.md
