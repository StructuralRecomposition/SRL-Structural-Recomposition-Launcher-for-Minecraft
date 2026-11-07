# SRL 发布包

当前源码版本：**v0.2.0**（Minecraft 联机中心 · 52FRP）

正式发布以 GitHub Releases 为准；本目录仅保留本地验证产物与说明。

| 平台 | 文件 |
|---|---|
| macOS（Intel + Apple Silicon） | `SRL_0.2.0_universal.dmg` |
| Windows 10/11 x64 | `SRL_0.2.0_windows-x64-portable.zip` |
| Linux x64（Debian / Ubuntu） | `SRL_0.2.0_amd64.deb` |
| Linux x64（通用） | `SRL_0.2.0_amd64.AppImage` |

校验值见 `release/v0.2.0/SHA256SUMS`。

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
sudo apt install ./SRL_0.2.0_amd64.deb
```

需要 WebKitGTK 4.1、GTK 3 等运行库。

## Minecraft 联机

1. 登录 <https://www.52frp.com/user/#/user-center>。
2. 在「API 访问令牌」创建 `hz_` 开头的令牌。
3. 打开 SRL →「Minecraft 联机」，粘贴令牌并连接。

令牌只保存在系统凭据库，不写入日志或普通配置文件。

## 说明

- macOS 未公证，Windows 未签名；首次运行需手动放行。
- 主要在 macOS x86_64 上验证；Windows / Linux 验证有限。
- 本目录不包含任何 Minecraft 游戏文件或 Mojang 素材。
