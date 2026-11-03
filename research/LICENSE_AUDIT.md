# 许可证审计

审计日期：2026-10-23。以下为工程性结论，不构成法律意见。

## 1. 参考项目

| 项目 | 仓库 | 许可证 | SPDX | 是否允许复制代码 | 结论 |
|---|---|---|---|---|---|
| PCL2（用户提供） | `tangge233/PCL2` | 《PCL 分发有限许可》+《合理使用指南》 | NOASSERTION | 否（仅允许原样副本分发） | 仅功能层研究，不复制 |
| PCL（上游） | `Meloong-Git/PCL` | 同上 | NOASSERTION | 否 | 仅功能层研究，不复制 |
| HMCL | `HMCL-dev/HMCL` | GNU GPL v3 | GPL-3.0 | 是，但衍生作品必须 GPL-3.0 且开放源码 | 不复制，避免许可传染 |
| HMCL 文档 | `HMCL-dev/HMCL-docs` | CC BY-NC-SA 4.0 + 附加条款 | CC-BY-NC-SA-4.0 | 否（禁止商业使用） | 不摘录文档文本 |

### 1.1 PCL2 关键限制摘录（依据 `LICENCE` 原文）

- 只授予"原样副本"的分发权，未授予修改权。
- "重度使用"包括参考较大量的内容以重新实现其部分实质功能（原文举例：启动/下载 Minecraft、管理 Mod）。
- 重度使用需以 `Plain Craft Launcher (PCL)` 开头命名、在关于页首位署名、公开源码、实现同等或更强的正版购买劝导、Windows 主色相为蓝色等。

**项目应对**：SR Launcher 不复用其名称、代码、素材、UI；技术上使用 Rust/React，而非 C#/WPF；不与 PCL 建立任何隶属或兼容暗示。

### 1.2 HMCL 关键限制

GPL-3.0 要求衍生作品整体以 GPL-3.0 分发并提供完整对应源码。本项目希望采用宽松许可（MIT），因此：

- 不复制 HMCL 的 Java 代码、资源文件、语言文件。
- 不将 HMCL 作为库链接。
- 仅参考公开目录结构所体现的通用分层思想（账号/下载/实例/启动），该思想本身不受版权保护。

## 2. 本项目许可

- SR Launcher 自有代码：**MIT**（见根 `LICENSE`）。
- 不包含 Minecraft 客户端、资源、库文件；这些文件由用户在安装时从 Mojang 官方源下载，受 Minecraft EULA 约束。
- 不包含 Mojang/Microsoft 商标素材。项目名称与图标为独立创作，不使用官方徽标。

## 3. 依赖许可证

当前直接依赖（阶段 1）：

| 依赖 | 许可证 | 用途 | 风险 |
|---|---|---|---|
| tauri 2 | MIT / Apache-2.0 | 桌面框架 | 低 |
| serde / serde_json | MIT / Apache-2.0 | 序列化 | 低 |
| tokio | MIT | 异步运行时 | 低 |
| reqwest | MIT / Apache-2.0 | HTTP | 低 |
| sha1 | MIT / Apache-2.0 | 文件校验 | 低 |
| thiserror | MIT / Apache-2.0 | 错误类型 | 低 |
| tracing / tracing-subscriber | MIT | 日志 | 低 |
| uuid | MIT / Apache-2.0 | 实例 ID | 低 |
| chrono | MIT / Apache-2.0 | 时间戳 | 低 |
| tokio-util | MIT | CancellationToken | 低 |
| keyring | MIT / Apache-2.0 | 系统凭据 | 中（平台后端差异） |
| zip | MIT | 解压 natives/整合包 | 中（需防 zip bomb） |
| React / Vite / TypeScript | MIT | 前端 | 低 |
| EasyTier 2.6.4（独立可执行文件） | LGPL-3.0 | 虚拟组网/NAT 穿透 | 中：需保留许可证与源码入口，不静态链接 |

> 依赖许可证需在发布前用 `cargo deny` / `npm license checker` 复核；当前为人工核对，标记为"待自动化验证"。

## 4. 资产审计

- 图标：由本项目生成（几何 SR 标记），不使用参考项目素材。
- 字体：使用系统字体栈，不内嵌第三方字体。
- 截图/文案：全部自写。

## 5. 待办

- [ ] 引入 `cargo-deny` 与 `cargo-audit` 到 CI。
- [x] 生成 `THIRD_PARTY_NOTICES.md`（含 EasyTier 与 x25519-dalek）。
- [ ] 发布前复核 Minecraft EULA 对启动器的要求。
- [ ] 若未来接入 Microsoft OAuth，复核 Microsoft 身份平台条款与品牌规范。
