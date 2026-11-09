# 构建指南

## 1. 前置要求

| 工具 | 版本 | 验证命令 |
|---|---|---|
| Node.js | ≥ 20（本项目 24.8） | `node -v` |
| npm | ≥ 10 | `npm -v` |
| Rust | ≥ 1.77（本项目 1.99） | `rustc -V` |
| Cargo | 随 Rust | `cargo -V` |

平台依赖：

### macOS

- Xcode Command Line Tools：`xcode-select --install`
- 本项目在 macOS 26.7 / x86_64 验证。

### Windows

- Microsoft C++ Build Tools（Desktop development with C++）
- WebView2 Runtime（Windows 10/11 通常已预装）
- Rust target：`rustup default stable-x86_64-pc-windows-msvc`

### Linux

以 Debian/Ubuntu 为例：

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

不同发行版包名不同，请参考 Tauri 官方 prerequisites。

## 2. 安装依赖

```bash
npm install
```

若 npm 因网络中断失败，可重试：

```bash
npm install --fetch-retries=5 --fetch-timeout=180000 --maxsockets=3
```

## 3. 开发

```bash
npm run dev:desktop
```

该命令会同时启动 Vite（1420）与 Tauri 窗口。首次运行需要编译 Rust 依赖，耗时较长。

仅前端：

```bash
npm run dev
```

浏览器中会明确提示"需要在桌面应用中运行"，不会加载模拟数据。

## 4. 构建

```bash
npm run build           # tsc -b + vite build
npm run build:desktop   # Tauri 打包（dmg/app、msi/nsis、deb/rpm/AppImage）
```

`beforeBuildCommand` 会自动执行 `scripts/fetch-frpc.mjs`，把当前平台/架构的
官方 frpc 压缩包下载到 `src-tauri/resources/frpc/<platform>/<arch>/` 并校验
SHA-256；压缩包不提交到 git，首次构建需要网络。手动下载或重新锁定版本：

```bash
node scripts/fetch-frpc.mjs --targets=darwin-amd64,darwin-arm64
node scripts/fetch-frpc.mjs --version=0.71.0
```

仅验证 Rust 可执行文件、不生成安装包：

```bash
npx tauri build --debug --no-bundle
```

产物：`src-tauri/target/debug/sr-launcher`（或 release 对应路径）。

## 5. 图标

图标源文件：`icon-1024.png`（项目自绘）。重新生成：

```bash
npx tauri icon icon-1024.png
```

生成 `src-tauri/icons/` 下的 PNG/ICNS/ICO 与移动端资源。`tauri.conf.json` 引用这些文件。

## 6. 测试

```bash
cargo test -p sr-core
npx vitest run
cargo check -p sr-launcher
```

真实网络安装（可选，约 60 MB）：

```bash
cargo test -p sr-core --test real_install -- --ignored --nocapture
```

真实清单拉取（可选）：

```bash
cargo test -p sr-core version::tests::fetches_manifest_from_official_source -- --ignored --nocapture
```

## 7. 常见问题

| 现象 | 原因 | 处理 |
|---|---|---|
| `npm error ECONNRESET` | 网络中断 | 降低并发重试，见上 |
| `failed to load manifest for workspace member` | `src-tauri` 缺少 Cargo.toml | 确认在仓库根目录执行 |
| Tauri 窗口空白 | 前端未构建/端口未启动 | 使用 `npm run dev:desktop`，不要直接打开 `index.html` |
| `esbuild` 安装脚本被拦截 | npm 11 allowScripts | `npm rebuild esbuild` |
| macOS 构建提示图标缺失 | icons 未生成 | `npx tauri icon icon-1024.png` |
| Linux 缺少 `webkit2gtk-4.1` | 系统依赖缺失 | 安装对应开发包 |
| 启动提示未发现 Java | 系统无 JVM 或未被扫描 | 在 Java 页面手动指定 `.../bin/java` |

## 8. 交叉编译说明

Tauri 不推荐直接交叉编译桌面应用。Windows/Linux 产物应在对应平台或 CI runner 上构建。本项目当前仅在 macOS 本机验证。
