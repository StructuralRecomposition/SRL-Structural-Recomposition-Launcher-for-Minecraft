# 账号与凭据安全

## 当前实现

### Microsoft 设备码登录

已实现（需在设置中填写 Azure 公开 client id）：

1. `login.microsoftonline.com/consumers/oauth2/v2.0/devicecode` 获取设备码。
2. 用户在浏览器输入代码；启动器按 `interval` 轮询 `/token`，处理 `authorization_pending`、`slow_down`、`expired_token`、`authorization_declined`。
3. Microsoft access token → Xbox Live `user.auth.xboxlive.com` → XSTS `xsts.auth.xboxlive.com`。
4. XSTS token → `api.minecraftservices.com/authentication/login_with_xbox`。
5. `/minecraft/profile` 获取 UUID 与用户名；404 明确表示未拥有游戏。
6. Minecraft access token / refresh token 以 JSON 存入系统凭据库（key `microsoft:<uuid>`），`accounts.json` 不含令牌。
7. 启动时若 Minecraft 令牌过期，用 refresh token 重新走 Xbox/Minecraft 交换。

未配置 client id 时所有入口返回 `NOT_CONFIGURED`，UI 按钮禁用。

代码链路已完成并通过模拟 HTTP 全链路测试；真实登录需要项目所有者提供一个自己的 Azure 应用 client id（Microsoft 要求每个启动器使用自己的应用注册，不能共用/伪造）。账号页已内置 client id 配置向导，填入后即可用设备码完成真实登录。

### 离线档案

- 用户名 3-16 位字母/数字/下划线。
- UUID 由 `md5("OfflinePlayer:" + name)` 生成，并按 RFC 4122 设置版本/变体位，与常见离线启动器一致。
- 启动时 `accessToken = "0"`、`userType = "legacy"`。
- UI 与账号列表明确标注"离线档案：仅用于本地游戏，不是已验证的正版账号"。

离线档案不会：

- 连接 Mojang 验证；
- 进入正版服务器；
- 被显示为"已授权"；
- 生成或保存任何令牌。

### 皮肤

- 皮肤库位于应用数据目录 `skins/`；`accounts.json` 只保存皮肤 ID 与模型，不含图片数据。
- 内置默认皮肤从用户本机已安装的客户端 JAR 读取，不随启动器分发 Mojang 素材。
- 导入 PNG 只接受 64×64、128×128 与旧版 64×32，限制 2 MB；解码前检查尺寸，避免恶意头部造成大分配；128×128 生成 64×64 兼容副本。
- 按正版玩家名查询只访问 Mojang 公开档案接口；纹理下载只允许 `textures.minecraft.net`（HTTPS）。
- Microsoft 上传调用官方 `POST /minecraft/profile/skins`；Minecraft 访问令牌只从系统凭据库读取，不写日志、不回传前端。
- 离线皮肤通过实例目录下的 `resourcepacks/SRLocalSkin.zip` 生效：
  - 仅本机可见，会覆盖默认皮肤贴图，不能改变手臂模型；
  - `options.txt` 只修改 `resourcePacks:` 行；该行不是合法 JSON 时保持原样并报错，不覆盖其他设置；
  - 切换到正版账号或清除皮肤后，启动时移除该资源包与启用项。

### Microsoft 登录配置

设备码链路已经实现；唯一外部前置条件是 Azure 应用注册：

1. Azure 门户 → Microsoft Entra ID → 应用注册 → 新注册。
2. 复制 Application (client) ID。
3. 身份验证 → 启用“允许公共客户端流”。
4. 在账号页或设置中填入 client id。

设备码流程不需要 client secret，也不需要本地回调端口。未配置时会明确返回 `AUTH_NOT_CONFIGURED`，不会伪造登录。
5. 令牌写入系统凭据库（keyring），**不写入** `accounts.json`。
6. 刷新失败时标记 `Expired`，要求重新授权。
7. 注销时删除系统凭据并清除内存副本。

约束：

- 不记录令牌、Authorization header、授权码。
- 不在命令行参数中传递令牌（启动游戏时 JVM 要求 `--accessToken`，属平台限制；不写入日志）。
- 不上传账号信息。
- 遵守 Microsoft/Xbox/Minecraft 服务条款。

## 凭据存储

`crates/sr-core/src/account.rs` 定义 `SecretStore`：

- `KeyringStore`：macOS Keychain / Windows Credential Manager / Linux Secret Service。
- `MemorySecretStore`：仅测试使用。

当前版本没有写入任何真实令牌；keyring 封装已编译并通过单元测试（内存实现）。

Linux 无 Secret Service 时，keyring 会失败；届时应禁用"记住登录"，不得回退到明文文件。

## 已知风险

1. 离线 UUID 可被他人使用相同用户名复现，不代表所有权。
2. JVM 命令行对同机其他用户可见（操作系统限制）。
3. 未实现令牌轮换与撤销。
4. 未做 OAuth 回调端口的 CSRF/状态校验（实现时必须加 `state` + PKCE）。
