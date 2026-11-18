# SRL 发布包

**简体中文** · [English](README.en.md)

当前源码版本：**v0.2.6 界面系统重构版**（新设计系统 · 更简洁的设置 · 启动器更新检查）

正式发布以 GitHub Releases 为准；本目录仅保留本地验证产物与说明。

| 平台 | 文件 |
|---|---|
| macOS（Intel + Apple Silicon） | `SRL_0.2.6_universal.dmg` |
| Windows 10/11 x64 | `SRL_0.2.6_windows-x64-portable.zip` |
| Linux x64（Debian / Ubuntu） | `SRL_0.2.6_amd64.deb` |
| Linux x64（通用） | `SRL_0.2.6_amd64.AppImage` |

校验值见 `SHA256SUMS`。

## 首次运行

### macOS

打开 DMG，把 SRL 拖入「应用程序」。构建未签名、未公证，Gatekeeper 可能提示。必要时：

```bash
xattr -dr com.apple.quarantine /Applications/SRL.app
```

### Windows

完整解压 ZIP，运行 `Install.cmd`（无需管理员权限），或直接运行 `sr-launcher.exe`。需要 WebView2 Runtime。

### Linux

```bash
sudo apt install ./SRL_0.2.6_amd64.deb
```

或运行 AppImage；需要 WebKitGTK 4.1 与 GTK 3。

## 说明

- 安装包由 GitHub Actions 在对应 tag 上构建；本地文件仅用于验证，不替代 Release 附件。
- 本版本不包含签名或公证；校验值以发布附件中的 `SHA256SUMS` 为准。

## v0.2.6 更新

- 全新界面：统一字体、间距、颜色和组件；主按钮改为经典黑白。
- 设置页更简洁：下载、网络、API、存储均可折叠；52FRP / CurseForge / DeepSeek 收进“API 与集成”。
- 修复路径复制按钮溢出、账号头像显示不完整；导入实例改用原生文件选择器。
- 新增启动器更新检查：启动时自动检查 GitHub Release，支持镜像下载、SHA-256 校验、安装并重启。
- 三平台安装包：macOS Universal DMG、Windows x64 便携 ZIP、Linux deb / AppImage。

## 数据位置

| 平台 | 路径 |
|---|---|
| macOS | `~/Library/Application Support/app.srlauncher.desktop` |
| Windows | `%APPDATA%\app.srlauncher.desktop` |
| Linux | `~/.local/share/app.srlauncher.desktop` |

卸载程序不会删除启动器数据。