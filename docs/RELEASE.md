# 发布流程

以下为发布前必须完成的清单。v0.2.5 已按此流程执行；平台实机项仍需在对应设备上复核。

## 1. 版本与变更

- [ ] 更新 `package.json`、`Cargo.toml`（workspace）、`src-tauri/tauri.conf.json` 版本号
- [ ] 更新 `CHANGELOG.md`
- [ ] 确认 README 功能表与真实状态一致
- [ ] 确认 `research/FEATURE_MATRIX.md` 状态列已更新

## 2. 质量门禁

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
npx vitest run
npm run build
npx tauri build --debug --no-bundle
```

- [ ] 无失败测试
- [ ] 无高危 `cargo audit` / `npm audit` 结果（CI 已接入）
- [ ] 手动走查：安装 → 校验 → 创建实例 → 启动 → 退出 → 删除
- [ ] 断网启动已安装版本
- [ ] 取消安装后无残留 `.srpart` 文件

## 3. 平台

- [ ] macOS（x86_64 / arm64）
- [ ] Windows 10/11（MSVC）
- [ ] Linux（X11 + Wayland，至少一个发行版）
- [ ] 安装包安装/卸载
- [ ] 代码签名与公证（macOS）、Authenticode（Windows）

## 4. 安全

- [ ] Tauri capabilities 仅 `core:default`
- [ ] 生产 CSP 生效，无内联脚本
- [ ] 确认无 shell/fs/http 插件
- [ ] 路径穿越与 zip 测试通过
- [ ] 日志脱敏抽查
- [ ] 诊断导出预览与脱敏
- [ ] 更新公钥已内置（实现更新后）

## 5. 更新系统（未实现）

设计要求：

- 更新元数据必须签名（ed25519/minisign），公钥内置。
- 更新包下载后再校验签名与哈希。
- 校验目标平台与版本，拒绝未授权降级。
- 应用失败时保留旧版本并可回滚。
- 未配置公钥时更新功能禁用，不执行任何下载的可执行文件。
- 更新服务不可用不影响已有安装运行。

## 6. 分发

- [ ] 发布渠道与下载页
- [ ] 校验和/签名公示
- [ ] 已知问题列表
- [ ] 许可证与第三方声明
- [ ] 不包含 Minecraft 游戏文件或 Mojang 素材

## 7. 回滚

- 保留上一版本安装包与签名。
- 数据格式变更时提供迁移或备份提示。
- 不自动删除用户游戏目录。
