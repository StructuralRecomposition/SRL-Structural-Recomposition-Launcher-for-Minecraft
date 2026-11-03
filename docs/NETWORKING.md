# 网络设计

## 出站请求

| 用途 | 地址 | 校验 |
|---|---|---|
| 版本清单 | `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json` | HTTPS + 域名白名单 |
| 版本 JSON | `piston-meta.mojang.com` | 清单中的 SHA-1 |
| 客户端 JAR | `piston-data.mojang.com` / `launcher.mojang.com` | 元数据 SHA-1 + 大小 |
| 库文件 | `libraries.minecraft.net` | 元数据 SHA-1 |
| 资源对象 | `resources.download.minecraft.net` | 内容哈希（文件名即 SHA-1） |

白名单在 `crates/sr-core/src/download.rs` 中精确匹配；非 HTTPS、带凭据、未知域名一律拒绝。`allow_custom_hosts` 设置项存在但 UI 禁用，阶段 1 不开放镜像。

## 超时与重试

- 连接超时：15s
- 单请求总超时：1800s（大文件流式读取）
- 最大重试：3 次（初次 + 3 次重试）
- 退避：400ms × 2^attempt + 抖动，上限 6s
- 429：读取 `Retry-After`（上限 30s）
- 仅对网络错误、超时、HTTP 429/5xx 重试
- 校验失败会重下一次；仍失败则报 `CHECKSUM_MISMATCH`

## 缓存

- 版本清单：6 小时 TTL；网络失败时回退任意年龄的本地缓存，并在 UI 显示警告。
- 已下载文件：按 SHA-1 跳过；损坏文件重下。
- 资源对象：内容寻址，天然去重。
- 已安装版本：断网可启动，不依赖清单。

## 本地优先

- 启动不请求网络。
- 版本安装完成后，启动只需要本地元数据与文件。
- 应用启动时不会阻塞等待 Java 扫描或网络。

## DDoS 相关

客户端层面采取：

- 有限并发（默认 6，上限 16）
- 指数退避
- 不做无限轮询
- 清单缓存
- 失败不风暴式重试

明确限制：任何客户端措施都不能"彻底防止"DDoS。未来若引入自建服务，需要 CDN、速率限制、熔断、健康检查、监控与降级，并明确区分关键/非关键服务。当前项目不依赖自建后端。

## 代理

阶段 1 未实现应用内代理设置，使用系统代理/环境变量。若添加，必须：

- 不记录代理凭据；
- 对 HTTPS 证书保持校验；
- 明确提示代理可能看见目标域名。
