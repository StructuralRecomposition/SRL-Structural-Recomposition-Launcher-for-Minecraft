# 贡献指南

## 开始之前

1. 阅读 `README.md`、`docs/ARCHITECTURE.md` 与 `research/LICENSE_AUDIT.md`。
2. 不要复制 PCL2、HMCL 或 HMCL 文档的代码/文本/素材。
3. 不提交 Minecraft 游戏文件、账号令牌或日志。

## 开发

```bash
npm install
npm run dev:desktop
```

代码规范：

- Rust：`cargo fmt`，尽量通过 `cargo clippy`。
- TypeScript：严格模式，不使用 `any` 逃避类型。
- 业务逻辑放 `crates/sr-core`，Tauri 命令保持薄。
- 前端按钮必须有真实行为；未实现功能明确禁用并说明。

## 提交

- 一个提交聚焦一件事。
- 说明"为什么"，而不只是"改了什么"。
- 涉及安全/下载/路径的改动必须附带测试。

## 测试

```bash
cargo test -p sr-core
npx vitest run
npm run build
```

网络测试默认忽略；新增的真实网络测试必须标注 `#[ignore]` 并说明流量消耗。

## 安全

- 不记录令牌、密码、Authorization header。
- 新下载源必须经过白名单与哈希校验设计评审。
- 新解压逻辑必须防路径穿越与 zip bomb。
- 发现漏洞请按 `SECURITY.md` 私下报告。

## 许可证

提交即表示你同意以 MIT 许可证贡献代码，并确认贡献为原创或有权提交。
