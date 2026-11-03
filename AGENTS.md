# AGENTS.md — SR Launcher 项目约定

本文件约束在此仓库工作的自动化代理与贡献者。

## 不可违反

1. 不复制 PCL2 / HMCL / HMCL-docs 的代码、素材、文案或大段实现。详见 `research/LICENSE_AUDIT.md`。
2. 不伪造功能、进度、下载速度、登录状态或测试结果。
3. 不声称"测试通过"而未实际运行。
4. 不把未实现功能描述为可用。
5. 不引入 Tauri shell/fs 插件来"图方便"。
5b. 仅 Forge/NeoForge 安装允许运行官方安装器，且必须由用户在 UI 确认后触发；不得静默执行，不得使用非官方/未校验安装器。
6. 不记录账号令牌、密码或 Authorization header。
7. 不让任何未校验的元数据路径直接落盘。
8. 不在未经确认的情况下删除用户游戏目录。

## 架构约束

- 业务逻辑放 `crates/sr-core`；`src-tauri` 只做 IPC 薄封装。
- 新功能先写核心函数 + 单元测试，再写命令与 UI。
- 前端不得直接发网络请求或执行系统命令。
- 所有跨边界输入在 Rust 侧重新校验。
- 文件写入使用 `atomic_write`；下载使用唯一临时文件 + 校验 + rename。
- 启动使用参数数组，禁止 shell 拼接。
- 新依赖必须说明用途、许可证与安全影响。

## 完成定义

一个功能只有满足以下条件才算完成：

- [ ] 核心逻辑有测试（成功 + 失败路径）
- [ ] `cargo test -p sr-core` 通过
- [ ] `npm run build` 通过
- [ ] 相关 UI 状态真实（空/加载/错误/禁用原因）
- [ ] 文档状态更新（README 功能表、FEATURE_MATRIX）
- [ ] 无新增敏感信息泄露路径

## 当前阶段

阶段 1 与 1.5 完成；阶段 2/3 部分完成。下一步优先级：

1. Windows/Linux 实机/CI 构建验证
2. 真实 Microsoft OAuth（需 Azure 配置）
3. OptiFine 图形安装器的可行集成方式
4. 整合包格式（CurseForge/Modrinth）与自动更新

## 快速命令

```bash
cargo test -p sr-core
npx vitest run
npm run build
npm run dev:desktop
```

真实网络测试（约 60 MB）：

```bash
cargo test -p sr-core --test real_install -- --ignored --nocapture
```
