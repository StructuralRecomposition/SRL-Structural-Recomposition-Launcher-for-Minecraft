# SRL 发布包

当前公开发布：**v0.1.0**

下载地址：**https://github.com/sry8n5rjz5-debug/SRL/releases/tag/v0.1.0**

| 平台 | 文件 |
|---|---|
| macOS（Intel + Apple Silicon） | `SRL_0.1.0_universal.dmg` |
| Windows 10/11 x64 | `SRL_0.1.0_windows-x64-portable.zip` |
| Linux x64（Debian / Ubuntu） | `SRL_0.1.0_amd64.deb` |
| 源码快照 | `SRL-0.1.0-source.tar.gz` |

校验值见 [`RELEASE_NOTES_v0.1.0.md`](RELEASE_NOTES_v0.1.0.md) 或 `SHA256SUMS`。

正式发布以 GitHub Releases 为准；本目录仅保留发布产物与说明。

## 首次运行

### macOS

```bash
xattr -dr com.apple.quarantine /Applications/SRL.app
```

或 Finder 右键 → 打开 → 仍要打开。

### Windows

完整解压 ZIP，运行 `Install.cmd`。未签名，SmartScreen 可能提示；请先核对 SHA-256。

### Linux

```bash
sudo apt install ./SRL_0.1.0_amd64.deb
```

需要 WebKitGTK 4.1、GTK 3 等运行库。

## 说明

- 发布包为 0.1.0 构建；仓库源码快照包含内部 0.1.1 / 0.1.2 的修复，版本号仍为 0.1.2。
- macOS 未公证，Windows 未签名；首次运行需手动放行。
- 主要在 macOS x86_64 上验证；Windows / Linux 验证有限。
- 本目录不包含任何 Minecraft 游戏文件或 Mojang 素材。

详细功能与限制见仓库根目录 [`README.md`](../README.md)。
