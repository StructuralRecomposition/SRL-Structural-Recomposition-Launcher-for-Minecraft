# SRL v0.2.6 — 界面系统重构

这次更新主要重做界面与设置体验，并加入启动器更新检查。Rust 后端、下载校验、实例数据和启动流程保持不变。

## 亮点

- **更清楚的首页。** 当前实例、启动按钮和运行信息集中在一个启动面板；日志与最近活动改为双栏。
- **更安静的界面。** 统一字体、间距、颜色、圆角和图标；去掉多余渐变、发光与卡片嵌套。
- **更简洁的设置。** 设置改为单列分组；“下载与皮肤”“网络与来源”“API 与集成”“数据与存储”按需展开。
- **API 集中管理。** 52FRP、CurseForge、DeepSeek 统一收进“API 与集成”，不再逐页堆叠。
- **经典黑白按钮。** 主按钮改为结构重组黑白配色；青绿色只用于状态、徽标和焦点。
- **实例更好管理。** 实例改为列表；复制、地图、材质、模组、光影、导出和删除收进“更多操作”菜单。
- **导入更直接。** 选择实例 ZIP 后自动检查，不再手动填写路径。
- **修复细节。** 账号头像完整显示；路径复制按钮不再溢出。

## 启动器更新

- 每次启动自动检查 GitHub 最新 Release。
- 发现新版本时弹窗询问，显示版本号和更新说明。
- 支持镜像下载，校验 SHA-256 后按平台安装并重启。
- 设置 → 游戏与下载可手动检查；已是最新版本时会提示。

## 下载

| 平台 | 文件 |
|---|---|
| macOS（Intel + Apple Silicon） | `SRL_0.2.6_universal.dmg` |
| Windows 10/11 x64 | `SRL_0.2.6_windows-x64-portable.zip` |
| Linux x64（Debian / Ubuntu） | `SRL_0.2.6_amd64.deb` |
| Linux x64（通用） | `SRL_0.2.6_amd64.AppImage` |

校验值见 Release 附件中的 `SHA256SUMS`。

## 说明

- macOS 构建未签名、未公证，首次打开可能需要在 Finder 中右键 → 打开。
- Windows 便携版需要 WebView2 Runtime；解压后运行 `Install.cmd`。
- Linux deb 需要 WebKitGTK 4.1 等运行库；AppImage 需要 FUSE 或 `--appimage-extract-and-run`。
- 完整设计说明见 [`docs/design-system-v3.md`](../docs/design-system-v3.md)。
