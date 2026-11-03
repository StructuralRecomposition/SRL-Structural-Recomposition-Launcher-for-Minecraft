# 用户数据

## 目录

### 应用数据目录

- macOS：`~/Library/Application Support/app.srlauncher.desktop`
- Windows：`%APPDATA%\app.srlauncher.desktop`
- Linux：`~/.local/share/app.srlauncher.desktop`

| 路径 | 内容 | 是否含敏感信息 |
|---|---|---|
| `settings.json` | 游戏目录、并发数、主题、默认 Java | 否 |
| `accounts.json` | 账号 ID、用户名、离线 UUID、类型、状态、所选皮肤 ID/模型 | 否（无令牌） |
| `skins/skins.json` | 皮肤库元数据（名称、模型、来源、尺寸、SHA-1） | 否 |
| `skins/<id>.png` | 原始皮肤 PNG（64×64 / 128×128 / 64×32） | 否 |
| `skins/<id>.64.png` | 规范化 64×64 兼容副本 | 否 |
| `logs/launcher.log*` | 启动器运行日志 | 已脱敏 |
| `logs/instances/*.log` | 游戏 stdout/stderr | 游戏可能输出路径/用户名；导出前需检查 |
| `cache/version_manifest_v2.json` | 官方版本清单缓存 | 否 |

### 游戏目录（默认 `.../minecraft`，可修改）

```
minecraft/
  versions/<id>/<id>.json      已解析元数据（安装成功后写入）
  versions/<id>/<id>.jar       客户端 JAR
  libraries/                   Maven 布局库文件
  assets/indexes/<id>.json     资源索引
  assets/objects/<hh>/<hash>   资源对象（内容寻址）
  assets/virtual/<id>/         旧版虚拟资源
  natives/<id>/                旧版 natives 解压目录
  log_configs/<sha1>           log4j 配置
  instances/<id>/              实例目录（存档、options.txt、mods 等）
  instances/<id>/resourcepacks/SRLocalSkin.zip   离线皮肤资源包（启动时生成，仅本机）
```

## 删除行为

| 操作 | 影响 |
|---|---|
| 删除实例（不勾选删除文件） | 仅删除 `instance.json`，实例目录保留 |
| 删除实例（勾选） | 递归删除实例目录，**不进回收站** |
| 移除账号档案 | 仅删除 `accounts.json` 记录；`skins/` 中的皮肤文件保留 |
| 删除皮肤 | 删除 `skins/` 中对应文件，并清除账号引用 |
| 清除任务记录 | 仅清空内存中的任务列表 |
| 卸载应用 | 不自动删除游戏目录 |

## 备份建议

- 实例存档：备份 `instances/<id>/saves`
- 配置：`instances/<id>/options.txt`、`config/`
- 模组：`instances/<id>/mods`

## 实例导出

`sr-instance-1` ZIP 归档包含：

- `sr-instance.json`：名称、版本、内存、JVM/游戏参数、分辨率、加载器信息
- 实例目录中的存档、模组、配置、资源包、光影等文件

默认排除：

- `logs/`、`crash-reports/`
- `instance.json` 与启动器内部文件
- 符号链接
- 账号数据与令牌（从未写入实例目录）

导入前会显示预览，并校验路径穿越、重复路径、压缩比、文件数量/大小与 Windows 保留名。导入要求对应的 Minecraft 版本已安装。

CurseForge/Modrinth 等整合包格式尚未实现。
