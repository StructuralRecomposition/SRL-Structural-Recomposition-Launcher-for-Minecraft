# Third-party notices

## FRPC（fatedier/frp）

SRL 联机中心内置 fatedier/frp 官方发布版中的 `frpc` 客户端（当前锁定版本见
`src-tauri/resources/frpc/frpc.lock.json`），用于连接 52FRP 提供的隧道服务。

- 上游项目：https://github.com/fatedier/frp
- 许可证：Apache License 2.0，全文见 `frpc/LICENSE`
- 分发方式：安装包只包含对应平台/架构的官方压缩包；首次使用时校验官方
  SHA-256 并解压到应用数据目录，不修改二进制。
- 回退路径：如果安装包未包含当前平台的内置客户端，SRL 才会从官方 GitHub
  Release 下载，并校验官方 SHA-256 校验文件。

SRL 仅调用 52FRP 隧道接口并生成 `frpc` 配置，不修改 `frpc` 本身。

## 历史说明

此前 Structural Connect（跨网络联机）功能已从代码库中移除；当前版本不再
内置或调用 EasyTier。
