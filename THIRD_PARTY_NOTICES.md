# Third-party notices

SR Launcher 当前版本不再内置或调用 EasyTier。此前 Structural Connect（跨网络联机）
功能已从代码库中移除，相关第三方组件与 LGPL 义务一并取消。

如需查看历史版本的分发说明，请参考 git 历史。

## Minecraft 联机

SRL 不内置 FRPC 二进制。联机中心通过 LoCyanFrp 官方 API `/v3/software/frp` 获取当前平台的官方
FRPC 客户端，下载到本地用户数据目录并在停止联机时保留或清理；FRPC 的许可与使用条款以
LoCyanFrp 官方发布物为准。SRL 仅调用官方隧道接口，不修改 FRPC。
