# 模组依赖关系（Deterministic Dependencies）

SRL 的依赖分析完全由程序逻辑和官方 API 元数据完成，**不调用任何 AI 模型推断依赖**。

## 数据来源

| 平台 | 接口 | 依赖字段 |
|---|---|---|
| Modrinth | `GET /v2/version/{id}` / `GET /v2/project/{id}/version` | `dependencies[].dependency_type`：`required` / `optional` / `incompatible` / `embedded` |
| CurseForge | `GET /v1/mods/{modId}/files/{fileId}` / `.../files` | `dependencies[].modId` + `relationType` |

CurseForge `relationType` 映射：

| 值 | 官方语义 | SRL 统一关系 | 自动安装 |
|---|---|---|---|
| 1 | EmbeddedLibrary | `embedded` | 否 |
| 2 | OptionalDependency | `optional` | 否，用户勾选 |
| 3 | RequiredDependency | `required` | 是 |
| 4 | Tool | `tool` | 否，仅展示 |
| 5 | Incompatible | `incompatible` | 阻止安装 |
| 6 | Include | `include` | 否，仅展示 |
| 其他 | 未知 | `unknown` | 否，保留未知 |

Modrinth 与 CurseForge 不共享同一套语义；适配器保留原始 `rawType`，不把 `EmbeddedLibrary` 直接当成可安装依赖。

## 解析规则

- 根模组必须是用户选择的**具体版本**（Modrinth `version_id`；CurseForge `file_id`）。
- 依赖有精确 `version_id` 时优先使用；没有时才按目标 Minecraft 版本和加载器筛选。
- 稳定版优先；没有稳定兼容版本时才选择 beta/alpha，并在提示中明确标注。
- 版本必须能证明支持目标 Minecraft 版本和加载器；否则标记 `compatible=false`，必需依赖会阻止安装。
- 最大递归深度 16 层，最大 API 请求 120 次，最大节点 200 个；超过后保留已解析部分并标记 `complete=false`。
- 循环引用通过已访问项目集合截断，重复边只保留一条。

## 已安装模组识别

只读取真实 JAR 元数据：

- `fabric.mod.json`：`id`、`version`、`provides`
- `quilt.mod.json`：`quilt_loader.id`、`version`、`provides`
- `META-INF/mods.toml`：`[[mods]] modId`、`version`
- `META-INF/neoforge.mods.toml`：同上
- CurseForge：Murmur2 指纹通过官方 `/v1/fingerprints` 匹配 `projectID/fileID`

限制：

- ZIP 条目上限 8192，单个元数据读取上限 512 KB，JAR 上限 512 MB。
- 只读取元数据条目，不解压整个 JAR。
- Modrinth 没有按哈希反查已安装项目的公开接口；SRL 只有在声明的 mod id 与项目 slug 一致时才判定为已安装。文件名或标题相似度**不会**作为依据。
- CurseForge 指纹最多扫描前 400 个 JAR，避免大实例分析过慢。

## 安装

- UI 必须先展示关系图/列表、安装计划、预计大小、冲突与警告。
- 渲染进程只提交 `analysisId` 和选中的节点 ID；URL、大小、SHA-1 从服务端保存的分析记录读取。
- 必需依赖自动包含；可选依赖必须用户主动勾选；不兼容关系阻止安装。
- 安装复用 `mod_install::apply_files`：
  - 每个文件必须有 SHA-1；
  - 下载到临时目录并校验；
  - 同名文件先备份；
  - 原子替换；
  - 失败回滚；
  - 任务进度和取消由现有下载任务系统提供。
- 若所有选中文件都已安装，返回“没有需要下载的文件”，不会伪造安装。

## 已知限制

- 依赖分析依赖平台 API 可用性；限流或 5xx 会显示为警告/未解析，不会猜测。
- CurseForge 依赖只提供 modId，无法指定精确文件；SRL 按目标版本/加载器选择该 mod 的文件。
- 本地已安装版本与依赖要求版本不一致时，SRL 只能确认“已安装”，不能证明版本满足要求；界面会显示已安装版本供人工判断。
- 没有引入图形可视化库；依赖图使用 SVG + 确定性分层布局。

- 分析快照写入 `cache/dependencies/<id>.json`，仅保留最近 200 份，避免无限缓存。
