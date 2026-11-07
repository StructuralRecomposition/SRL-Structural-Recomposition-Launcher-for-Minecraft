# SRL v0.2.0 — Minecraft 联机中心

SRL 现在可以直接创建 Minecraft 联机房间，不要求公网 IP、路由器端口映射或 FRP 知识。

## 新增

- 「Minecraft 联机」页面：账户、实例、版本类型、本地服务、节点、房间与 FRPC 状态集中在一页。
- 52FRP 开放 API v1：账户、节点、节点负载、名称/端口检查、已占用端口、隧道创建/查询/删除、完整 FRPC TOML。
- Java Edition 使用 TCP，自动识别局域网动态端口；Bedrock Edition 使用 UDP，支持 19132 与手动端口。
- 创建过程逐步展示真实检查结果；失败自动回滚隧道与 FRPC。
- 停止联机与删除临时隧道分离；只删除 `SRL_` 前缀的 SRL 隧道。
- 设置页新增 52FRP API 访问令牌管理。

## 连接 52FRP

1. 登录 <https://www.52frp.com/user/#/user-center>。
2. 在「API 访问令牌」创建令牌，复制 `hz_` 开头的明文。
3. 打开 SRL →「Minecraft 联机」，粘贴令牌并点击「连接 52FRP」。

令牌只保存在系统凭据库，不写入日志或普通配置文件。

## 安全

- API 令牌存系统凭据库。
- FRPC 配置使用 `0600` 临时文件，停止时删除；日志脱敏。
- FRPC 使用 fatedier/frp 官方 Release，按 GitHub 资产 SHA-256 校验后才运行。

## 已知限制

- 需要真实 52FRP 账户与令牌完成在线联调；没有令牌无法伪造测试成功。
- 账户必须实名，且套餐覆盖目标节点。
- Bedrock 需要本机已有 UDP 服务监听。
