# Java 运行环境管理

## 现有能力

- 扫描 `JAVA_HOME`、`PATH`、macOS `JavaVirtualMachines`、Linux/Windows 常见目录，以及 SRL 自己的 `<app data>/java/`。
- 通过 `java -XshowSettings:properties -version` 读取真实 `java.version`、vendor、arch。
- 读取 Minecraft 版本元数据中的 `javaVersion.majorVersion`，不按版本号硬编码。
- `java::select_for_launch` 的兼容范围：目标 major 到 `max_supported_major(required)`。
- Eclipse Temurin（Adoptium）下载、清华 TUNA 镜像回退、SHA-256 校验、安全解压。
- 下载任务进度、取消与错误显示。

## 启动前选择顺序

1. 实例显式指定的 Java（必须先通过兼容性检查，永不自动替换）。
2. 已安装运行时中与 required major 完全匹配的版本（优先用户默认路径）。
3. 用户全局默认 Java（仅当它在安全兼容范围内）。
4. 安全范围内最接近 required 的版本。
5. 没有安全候选时返回错误，不强行使用过新/过旧环境。

`java::choose_for_launch` 是唯一选择入口，`launch_instance` 不再自行拼装选择逻辑。

## 缺失 Java 的启动流程

`launch_instance` 返回 `JAVA_INVALID` 或 `JAVA_INCOMPATIBLE` 时，首页/实例页打开 `JavaInstallPrompt`：

1. 调用 `java_launch_check(instance_id)`，展示动态读取的：
   - 需要 Java major；
   - 兼容范围；
   - 当前选择；
   - 实例是否显式指定 Java；
   - 是否可安装 / 是否需要重置实例 Java。
2. 用户点击“下载并安装 Java N”。
3. 调用 `install_java_for_version(major, setAsDefault=false)`：
   - 使用现有 Temurin 安装器；
   - 使用现有下载任务与 SHA-256；
   - 安装后重新扫描 Java 环境；
   - **不修改全局默认 Java**。
4. 重新执行 `java_launch_check`；兼容后用户点击继续启动，只启动一次游戏进程。
5. 安装失败显示真实错误，可重试或取消。

## 全局默认 Java 的边界

- `install_java_runtime(major, setAsDefault=true)` 保留给“Java 环境”页面的手动安装；当已有默认 Java 时 UI 传 `false`。
- 自动补全缺失 Java 永远不覆盖用户默认值。
- 用户仍可在“Java 环境”中显式把某个运行环境设为默认。
- 实例级 Java 指定不受全局默认影响；不兼容时给出明确错误并提供改回自动选择的方式。

## 平台

保留现有支持：Windows x64、macOS x64/ARM64、Linux x64。其他平台/架构由 Adoptium 资产解析返回明确错误，不会显示“下载成功”但后台没有安装。

## 测试

- `parse_major`：Java 8 / 17 / 21 / 25、`1.8.0_392`、`25-ea`。
- `max_supported_major` / `is_compatible` / `check_compatibility`。
- `choose_for_launch`：实例固定 Java、全局默认不兼容、完全匹配优先、没有候选时 Missing。
- 现有 `java_runtime_network` 真实下载测试默认忽略。
