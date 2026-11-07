# SRL v0.2.0 — Minecraft 联机中心

SRL 现在可以直接创建 Minecraft 联机房间，不要求公网 IP、路由器端口映射或 FRP 知识。

## 新增

- 「Minecraft 联机」页面：账户、实例、版本类型、本地服务、节点、房间与 FRPC 状态集中在一页。
- LoCyanFrp 官方 API v3：OAuth 2.0、用户、节点、随机端口、隧道创建/查询/删除、隧道配置、FRPC 资源。
- Java Edition 使用 TCP，自动识别局域网动态端口；Bedrock Edition 使用 UDP，支持 19132 与手动端口。
- 创建过程逐步展示真实检查结果；失败自动回滚隧道与 FRPC。
- 停止联机与删除临时隧道分离；只删除 `SRL-MP-` 前缀的 SRL 隧道。
- 设置页新增 LoCyanFrp OAuth 应用 ID 与权限节点。

## 安全

- refresh token 只存系统凭据库，access token 只在内存。
- FRPC 配置使用 `0600` 临时文件，停止时删除；日志脱敏。
- FRPC 下载只接受官方 HTTPS 来源，并校验可执行文件格式。

## 需要你配置

1. 在 <https://dashboard.locyanfrp.cn/app/create> 创建 OAuth2.0 应用。
2. 回调地址填 `https://dashboard.locyanfrp.cn/auth/oauth/redirect-localhost`。
3. 在 SRL 设置页填写应用 ID，保存后回到联机页连接账户。

## 已知限制

- 本次开发网络无法解析 `api.locyanfrp.cn`，真实账户在线联调尚未完成；代码按官方 v3 文档实现，单元测试覆盖解析、端口检测、节点评分、FRPC 生命周期与回滚。
- 官方 `/software/frp` 未提供 SHA-256 字段；SRL 校验来源、压缩包和可执行文件格式，并记录本地哈希。
- Bedrock 需要本机已有 UDP 服务监听。
