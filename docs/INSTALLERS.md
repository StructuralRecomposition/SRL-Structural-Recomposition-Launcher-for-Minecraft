# 安装包与发布说明

## 已生成（macOS universal）

- `release/SRL_0.1.0_universal.dmg`（SHA-256 `2a59983e…e57c`）
- 同时包含 `x86_64`（Intel）与 `arm64`（Apple Silicon），可用 `lipo -info` 验证。
- 未签名、未公证：首次打开需要右键 → 打开，或在“隐私与安全性”中允许。
- DMG 只包含 `SRL.app`，不包含 settings、账号、聊天记录、Java、示例实例或测试数据。
- 首次启动会在 `~/Library/Application Support/app.srlauncher.desktop` 创建干净的数据目录；默认主题为浅色。

校验：

```bash
shasum -a 256 release/SRL_0.1.0_universal.dmg
```

## Windows x64 便携测试包（当前可下载）

- `release/SRL_0.1.0_windows-x64-portable.zip`（SHA-256 `e56bf31a…e375`）
- 解压后运行 `Install.cmd`：复制到 `%LOCALAPPDATA%\SRL`，创建桌面/开始菜单快捷方式并启动；`Uninstall.cmd` 卸载程序但保留数据。
- 这是 `cargo-xwin` 交叉编译的未签名测试包；Windows 资源（exe 图标/版本信息）不完整，快捷方式图标由安装脚本提供。已修复 Common Controls v6 清单缺失导致的 `TaskDialogIndirect` 入口错误。正式 NSIS/MSI 仍建议在 Windows 或 CI 上构建。
- 需要 Windows 10/11 x64 与 WebView2 Runtime。

## Linux / Windows

当前开发机是 macOS，无法在这里生成并验证 Linux/Windows 的原生安装包；没有用未验证的交叉编译产物冒充“可安装”。

已提供 `.github/workflows/build-installers.yml`：

- macOS：universal（.app + .dmg）
- Linux（ubuntu-22.04）：.deb + .AppImage
- Windows（windows-latest）：NSIS .exe + .msi

推送到 GitHub 后可在 Actions 中手动运行，或在推送 `v*` 标签时自动构建并下载 artifacts。每个 runner 都是原生系统，避免交叉编译风险。

本地等价命令：

```bash
# macOS universal
npm run tauri build -- --target universal-apple-darwin --bundles app,dmg

# Linux（在 Linux 上）
npm run tauri build -- --bundles deb,appimage

# Windows（在 Windows 上）
npm run tauri build -- --bundles nsis,msi
```

## 干净构建检查

```bash
bash scripts/verify-clean-bundle.sh path/to/SRL.app
```

脚本会拒绝：settings/accounts/instance/日志/皮肤/Java/测试样例文件、测试 fixture 字符串、`sk-...` 形式的 DeepSeek key。

DeepSeek API key 只存在于用户系统凭据库，不写入源码、settings.json、日志或安装包。
