# SRL v0.1.0 — 首个公开发布

**SRL（Structural Recomposition Launcher）** 是一个独立实现的、本地优先的 Minecraft Java Edition 启动器。它使用 Rust + Tauri 2 构建后端，React + TypeScript 构建界面，不依赖自建服务端，不上报遥测。

> 本项目与 Mojang、Microsoft、PCL2、HMCL 均无隶属关系。“Minecraft”是 Mojang Studios 的商标。

## 下载

| 平台 | 文件 | SHA-256 |
|---|---|---|
| macOS 10.15+（Intel + Apple Silicon） | `SRL_0.1.0_universal.dmg` | `2a59983e892a6690bdfc69c249a46917e97d51f2110af2a9b48aac2c4d92e57c` |
| Windows 10/11 x64 | `SRL_0.1.0_windows-x64-portable.zip` | `b43d09a327efa9d5039aa620b1b19e31a51ea5200b76443dd27cce6ecbf32244` |
| Linux x64（Debian / Ubuntu） | `SRL_0.1.0_amd64.deb` | `e79d1276f6de68ab6043fdaeee744ec42a50915ad1004f68ab1ca52f399a7548` |
| 源码快照 | `SRL-0.1.0-source.tar.gz` | 见 `SHA256SUMS` |

完整的 `SHA256SUMS` 随发布包提供。

## 安装

### macOS

应用未经过 Apple 公证，首次打开可能被 Gatekeeper 拦截：

```bash
xattr -dr com.apple.quarantine /Applications/SRL.app
```

或在 Finder 中右键 → 打开 → 仍要打开。

### Windows

- 完整解压 ZIP 后再运行，不要在压缩包预览中直接启动。
- 运行 `Install.cmd`：会按需安装 WebView2、复制程序并创建快捷方式。
- 未签名，SmartScreen 可能提示；请先核对 SHA-256，再选择“更多信息 → 仍要运行”。
- 仅提供 x64；ARM64 Windows 未构建。

### Linux

```bash
sudo apt install ./SRL_0.1.0_amd64.deb
```

需要 WebKitGTK 4.1、GTK 3 等运行库。当前未提供 AppImage。

## 主要能力

- 官方版本清单、分类浏览、一键安装、文件完整性检查。
- Fabric / Quilt / Forge / NeoForge 加载器安装与版本推荐。
- 多实例、独立账号与窗口配置；实例 ZIP 导出 / 导入。
- 模组、材质包、光影包、地图存档管理。
- Modrinth / CurseForge 资源搜索与安装；`.mrpack`、CurseForge `manifest.zip`、普通整合 ZIP 导入。
- 离线账号、Microsoft 设备码登录（需自备 Azure client id）、皮肤库与本地皮肤。
- 崩溃报告分析、日志查看、环境诊断。
- 鸦鸭 AI 助手（可选 DeepSeek key）：本地证据分析、只读工具、安全修复计划、带依赖的模组安装与回滚。
- 下载任务持久化、SHA-1 校验、安全断点续传、国内镜像回退。

## 版本说明

`v0.1.0` 是 SRL 的首个公开发布。发布包为此前构建的 0.1.0 产物；仓库源码快照包含发布前完成的内部修复。如果你需要与发布包一致的构建，请从本 tag 自行构建。

## 已知限制

1. Microsoft 登录需要在设置中填写自己的 Azure 应用 client id；未配置时明确禁用。
2. Forge / NeoForge 在后端执行官方安装流程，不显示官方安装器窗口。
3. 应用重启后，未完成任务会标记为“中断”，不会自动续传。
4. 断点续传仅在服务器提供 `ETag` / `Last-Modified` 时生效。
5. 光影包需要 Iris 或 OptiFine；启动器只管理 `shaderpacks/` 并安装对应模组。
6. Java 自动安装仅支持 Eclipse Temurin。
7. 大型整合包可能触及 Windows 命令行长度限制。
8. 整合包完整导出、应用自动更新、鸦鸭的整合包级操作尚未实现。
9. 主要在 macOS x86_64 上验证；Windows / Linux 构建与运行验证有限。

## 源码与许可

- 源码：本仓库 `v0.1.0` tag，或 `SRL-0.1.0-source.tar.gz`。
- 许可证：MIT，见 `LICENSE`。
- 第三方依赖与许可证：见 `THIRD_PARTY_NOTICES.md`。
- 本项目不包含任何 Minecraft 游戏文件或 Mojang 素材。
