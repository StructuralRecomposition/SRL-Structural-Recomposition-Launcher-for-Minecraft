<a id="zh"></a>

# SRL v0.2.1 — 中英双语 · 内置 FRPC · 整合包导出

**简体中文** · [English](#en)

> 这是 v0.2.0「Minecraft 联机中心」之后的第一个功能更新。下载、启动与联机逻辑保持真实实现；本次重点是把联机客户端的网络依赖去掉，并把实例变成可分享、可复现的整合包。

## 亮点

### 1. 中英双语界面

- 设置 → 界面语言：简体中文 / English，默认简体中文。
- 切换后立即保存并重新加载窗口；导航、页面、表单、按钮、提示与帮助文案均使用对应语言。
- 语言偏好保存在本地设置与本地存储中，升级后保留。

### 2. FRPC 安装包内置

- 安装包按平台/架构携带 fatedier/frp 官方 `frpc` 压缩包（当前锁定 0.71.0）。
- 首次创建房间时校验 SHA-256、解压到应用数据目录并运行，**不再访问 GitHub**。
- 仅当安装包缺少当前平台客户端时，才回退官方 Release 下载；仍保留官方 SHA-256 校验和多镜像回退。
- Windows 便携包的 `Install.cmd` 会一并安装 `frpc/`。
- 修复中国网络下 FRPC 下载慢、超时、校验文件拉取失败导致的建房失败。

### 3. 整合包导出

实例卡片新增「导出整合包」，支持两种格式：

| 格式 | 产物 | 模组识别 |
|---|---|---|
| Modrinth | `.mrpack` | SHA-1 查询 Modrinth |
| CurseForge | `manifest.json` ZIP | 官方 Murmur2 指纹查询 projectID / fileID |

- 可识别的模组写入索引，由导入方重新下载并校验。
- 未识别、已禁用、同项目重复版本的模组，连同 `config`、`defaultconfigs`、`kubejs`、`scripts`、资源包、光影进入 `overrides/`。
- 自动解析 Minecraft 版本与 Fabric / Quilt / Forge / NeoForge 加载器依赖。
- 不包含存档、日志、截图或账号令牌。
- CurseForge 使用内置公共 API key；限流或断网时自动回退为 overrides 模式并给出警告。

## 验证

- Rust 206 个库测试通过；真实网络测试覆盖 Modrinth 导出与 CurseForge 指纹导出（GeckoLib → projectID 388172 / fileID 8945638）。
- 前端 45 个测试通过，覆盖中英文翻译、插值与后端错误映射。
- `cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`tsc -b` 通过。
- macOS DMG 挂载启动 smoke 通过；安装包内确认包含 `Resources/frpc`；`verify-clean-bundle.sh` 通过。

## 安装包

| 平台 | 文件 |
|---|---|
| macOS 10.15+ | `SRL_0.2.1_universal.dmg` |
| Windows 10/11 x64 | `SRL_0.2.1_windows-x64-portable.zip` |
| Linux x64 | `SRL_0.2.1_amd64.deb` |
| Linux x64 | `SRL_0.2.1_amd64.AppImage` |

校验值见 `SHA256SUMS`。

## 已知限制

- Microsoft 正版登录仍需自行注册 Azure 应用并填写 `client id`。
- macOS 包未签名、未公证，首次打开可能需要右键 → 打开。
- 放入 `overrides/` 的模组会随整合包分发，分享前请确认模组许可。
- CurseForge 使用公共 key，频繁导出可能触发限流；此时会回退为 overrides 并提示。

完整变更：见 [`CHANGELOG.md`](../CHANGELOG.md)。

---

<a id="en"></a>

# SRL v0.2.1 — Bilingual UI · Bundled FRPC · Modpack Export

[简体中文](#zh) · **English**

> The first feature update after v0.2.0 “Minecraft Multiplayer Center”. Downloads, launching and tunneling remain real implementations; this release removes FRPC's runtime network dependency and turns instances into shareable, reproducible modpacks.

## Highlights

### 1. Bilingual UI

- Settings → Interface language: Simplified Chinese / English, Chinese by default.
- Switching saves immediately and reloads the window; navigation, pages, forms, buttons, notices and help text all use the selected language.
- The preference is stored in local settings and local storage, and survives upgrades.

### 2. Bundled FRPC

- Each installer ships the official fatedier/frp `frpc` archive for its platform/architecture (currently pinned to 0.71.0).
- On first room creation SRL verifies the SHA-256, extracts it into the app data directory and runs it — **no GitHub request**.
- Only when the installer lacks the current platform client does SRL fall back to the official release download, still with official SHA-256 verification and mirror fallback.
- The Windows portable `Install.cmd` now installs `frpc/` alongside the executable.
- Fixes room-creation failures caused by slow or timed-out FRPC downloads and checksum fetches on Chinese networks.

### 3. Modpack export

The instance card now has “Export modpack” with two formats:

| Format | Output | Mod matching |
|---|---|---|
| Modrinth | `.mrpack` | SHA-1 lookup on Modrinth |
| CurseForge | `manifest.json` ZIP | Official Murmur2 fingerprint → projectID / fileID |

- Recognized mods are written to the index and re-downloaded and verified by the importing launcher.
- Unknown, disabled or duplicate-project mods, plus `config`, `defaultconfigs`, `kubejs`, `scripts`, resource packs and shaders go into `overrides/`.
- Minecraft version and Fabric / Quilt / Forge / NeoForge loader dependencies are resolved automatically.
- Worlds, logs, screenshots and account tokens are excluded.
- CurseForge uses the built-in public API key; on rate limits or offline it falls back to overrides mode with a warning.

## Verification

- 206 Rust library tests pass; real-network tests cover Modrinth export and CurseForge fingerprint export (GeckoLib → projectID 388172 / fileID 8945638).
- 45 frontend tests pass, covering Chinese/English translation, interpolation and backend error mapping.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `tsc -b` pass.
- The macOS DMG mounts and launches; `Resources/frpc` is present; `verify-clean-bundle.sh` passes.

## Downloads

| Platform | File |
|---|---|
| macOS 10.15+ | `SRL_0.2.1_universal.dmg` |
| Windows 10/11 x64 | `SRL_0.2.1_windows-x64-portable.zip` |
| Linux x64 | `SRL_0.2.1_amd64.deb` |
| Linux x64 | `SRL_0.2.1_amd64.AppImage` |

See `SHA256SUMS` in the release assets.

## Known limitations

- Microsoft login still requires your own Azure application `client id`.
- The macOS build is unsigned and not notarized; use right-click → Open on first launch if needed.
- Mods placed in `overrides/` are redistributed with the pack; verify their licenses before sharing.
- CurseForge uses a public key; frequent exports may be rate-limited, in which case SRL falls back to overrides and warns.

Full changelog: [`CHANGELOG.md`](../CHANGELOG.md).
