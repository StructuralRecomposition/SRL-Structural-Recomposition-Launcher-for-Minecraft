# SRL 发布包

**简体中文** · [English](README.en.md)

当前源码版本：**v0.2.1**（中英双语 · 内置 FRPC · 整合包导出）

正式发布以 GitHub Releases 为准；本目录仅保留本地验证产物与说明。

| 平台 | 文件 |
|---|---|
| macOS（Intel + Apple Silicon） | `SRL_0.2.1_universal.dmg` |
| Windows 10/11 x64 | `SRL_0.2.1_windows-x64-portable.zip` |
| Linux x64（Debian / Ubuntu） | `SRL_0.2.1_amd64.deb` |
| Linux x64（通用） | `SRL_0.2.1_amd64.AppImage` |

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
sudo apt install ./SRL_0.2.1_amd64.deb
```

或运行 AppImage；需要 WebKitGTK 4.1 与 GTK 3。

## v0.2.1 更新

- 中英双语界面，默认简体中文，设置中切换。
- 安装包内置官方 frpc，首次使用校验 SHA-256 后解压运行，创建房间不再依赖 GitHub。
- 新增 Modrinth `.mrpack` 与 CurseForge `manifest.json` 整合包导出。

## 数据位置

| 平台 | 路径 |
|---|---|
| macOS | `~/Library/Application Support/app.srlauncher.desktop` |
| Windows | `%APPDATA%\app.srlauncher.desktop` |
| Linux | `~/.local/share/app.srlauncher.desktop` |

卸载程序不会删除启动器数据。
