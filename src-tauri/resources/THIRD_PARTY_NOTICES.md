# Third-party notices

SR Launcher 当前版本不再内置或调用 EasyTier。此前 Structural Connect（跨网络联机）
功能已从代码库中移除，相关第三方组件与 LGPL 义务一并取消。

如需查看历史版本的分发说明，请参考 git 历史。

## Minecraft 联机

SRL 不内置 FRPC 二进制。联机中心使用 52FRP 开放 API，并从 fatedier/frp 官方 GitHub Release
下载当前平台的 FRPC 客户端（含官方 SHA-256 校验文件）。FRPC 的许可以上游 fatedier/frp 发布物为准；
SRL 仅调用 52FRP 隧道接口，不修改 FRPC。
