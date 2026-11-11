import { useMemo, useState } from "react";
import { api, ApiError, type ImportCheck, type Instance } from "../lib/api";
import { formatBytes, formatDate, relativeTime, splitLines } from "../lib/format";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, Field, IconButton } from "../components/ui";
import { Modal } from "../components/Modal";
import { Icon } from "../components/Icon";
import { ModManager } from "../components/ModManager";
import { ShaderManager } from "../components/ShaderManager";
import { MapManager } from "../components/MapManager";
import { ResourcePackManager } from "../components/ResourcePackManager";
import { t } from "../lib/i18n";
import { JavaInstallPrompt } from "../components/JavaInstallPrompt";


export function InstancesPage() {
  const { bootstrap, reload, running, markRunning, pushToast, setPage } = useApp();
  const instances = bootstrap?.instances ?? [];
  const versions = bootstrap?.installedVersions ?? [];
  const javas = bootstrap?.java ?? [];

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<Instance | null>(null);
  const [deleting, setDeleting] = useState<Instance | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [javaPrompt, setJavaPrompt] = useState<Instance | null>(null);

  const [name, setName] = useState("");
  const [versionId, setVersionId] = useState(versions[0]?.id ?? "");
  const [memory, setMemory] = useState(4096);
  const [editName, setEditName] = useState("");
  const [editMemory, setEditMemory] = useState(4096);
  const [editJava, setEditJava] = useState("");
  const [editJvm, setEditJvm] = useState("");
  const [editGame, setEditGame] = useState("");
  const [editAccount, setEditAccount] = useState("");
  const [editWidth, setEditWidth] = useState("");
  const [editHeight, setEditHeight] = useState("");
  const [deleteFiles, setDeleteFiles] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [exporting, setExporting] = useState<Instance | null>(null);
  const [exportPath, setExportPath] = useState("");
  const [overwrite, setOverwrite] = useState(false);
  const [packFor, setPackFor] = useState<Instance | null>(null);
  const [packFormat, setPackFormat] = useState<"modrinth" | "curseforge">("modrinth");
  const [packPath, setPackPath] = useState("");
  const [packName, setPackName] = useState("");
  const [packVersion, setPackVersion] = useState("1.0.0");
  const [packOverwrite, setPackOverwrite] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [importPath, setImportPath] = useState("");
  const [importCheck, setImportCheck] = useState<ImportCheck | null>(null);
  const [importName, setImportName] = useState("");
  const [checking, setChecking] = useState(false);
  const [modsFor, setModsFor] = useState<Instance | null>(null);
  const [shadersFor, setShadersFor] = useState<Instance | null>(null);
  const [mapsFor, setMapsFor] = useState<Instance | null>(null);
  const [texturesFor, setTexturesFor] = useState<Instance | null>(null);

  const javasSorted = useMemo(() => [...javas].sort((a, b) => b.major - a.major), [javas]);

  function openCreate() {
    setName("");
    setVersionId(versions[0]?.id ?? "");
    setMemory(4096);
    setFormError(null);
    setCreateOpen(true);
  }

  function openEdit(inst: Instance) {
    setEditing(inst);
    setEditName(inst.name);
    setEditMemory(inst.memoryMb);
    setEditJava(inst.javaPath ?? "");
    setEditJvm(inst.jvmArgs.join("\n"));
    setEditGame(inst.gameArgs.join("\n"));
    setEditAccount(inst.accountId ?? "");
    setEditWidth(inst.resolutionWidth ? String(inst.resolutionWidth) : "");
    setEditHeight(inst.resolutionHeight ? String(inst.resolutionHeight) : "");
    setFormError(null);
  }

  async function create() {
    setFormError(null);
    if (!name.trim()) {
      setFormError(t("请输入实例名称"));
      return;
    }
    if (!versionId) {
      setFormError(t("请先安装一个游戏版本"));
      return;
    }
    setBusy("create");
    try {
      await api.createInstance({ name: name.trim(), versionId, memoryMb: memory });
      setCreateOpen(false);
      pushToast(t("实例已创建"), "success", `${name.trim()} · ${versionId}`);
      await reload();
    } catch (e) {
      setFormError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function saveEdit() {
    if (!editing) return;
    setBusy("edit");
    setFormError(null);
    try {
      await api.updateInstance(editing.id, {
        name: editName.trim(),
        memoryMb: editMemory,
        javaPath: editJava,
        jvmArgs: splitLines(editJvm),
        gameArgs: splitLines(editGame),
        accountId: editAccount,
        resolutionWidth: Number(editWidth) || 0,
        resolutionHeight: Number(editHeight) || 0,
      });
      setEditing(null);
      pushToast(t("实例设置已保存"), "success");
      await reload();
    } catch (e) {
      setFormError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function launch(inst: Instance) {
    setBusy(inst.id);
    try {
      const result = await api.launchInstance(inst.id);
      markRunning(inst.id, true);
      pushToast(t("游戏进程已启动"), "success", `PID ${result.pid}`);
      if (result.warnings.length > 0) {
        pushToast(t("启动提示"), "warning", result.warnings.join("；"));
      }
    } catch (e) {
      if (
        e instanceof ApiError &&
        (e.code === "JAVA_INVALID" || e.code === "JAVA_INCOMPATIBLE")
      ) {
        setJavaPrompt(inst);
      } else {
        pushToast(t("启动失败"), "error", e instanceof Error ? e.message : String(e));
      }
    } finally {
      setBusy(null);
    }
  }

  async function stop(inst: Instance) {
    try {
      await api.stopInstance(inst.id);
      pushToast(t("正在终止游戏进程"), "warning");
    } catch (e) {
      pushToast(t("无法终止进程"), "error", e instanceof Error ? e.message : String(e));
    }
  }

  async function duplicate(inst: Instance) {
    setBusy(inst.id);
    try {
      const copy = await api.duplicateInstance(inst.id, t("{0} 副本", [inst.name]));
      pushToast(t("实例已复制"), "success", copy.name);
      await reload();
    } catch (e) {
      pushToast(t("复制失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function confirmDelete() {
    if (!deleting) return;
    setBusy(deleting.id);
    try {
      await api.deleteInstance(deleting.id, deleteFiles);
      setDeleting(null);
      setDeleteFiles(false);
      pushToast(t("实例已删除"), "info", deleteFiles ? t("实例目录已一并删除") : t("游戏文件已保留"));
      await reload();
    } catch (e) {
      pushToast(t("删除失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function doExport() {
    if (!exporting) return;
    setBusy(exporting.id);
    try {
      const summary = await api.exportInstanceArchive(
        exporting.id,
        exportPath.trim(),
        overwrite,
      );
      setExporting(null);
      setExportPath("");
      setOverwrite(false);
      pushToast(
        t("实例已导出"),
        "success",
        t("{0} 个文件 · {1}{2}", [summary.files, formatBytes(summary.bytes), summary.skipped ? t("· 跳过 {0} 个符号链接", [summary.skipped]) : ""]),
      );
    } catch (e) {
      setFormError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function doPackExport() {
    if (!packFor) return;
    setBusy("modpack");
    setFormError(null);
    try {
      const args = [
        packFor.id,
        packPath.trim(),
        packOverwrite,
        packName.trim() || undefined,
        packVersion.trim() || undefined,
      ] as const;
      const summary =
        packFormat === "modrinth"
          ? await api.exportInstanceModpack(...args)
          : await api.exportInstanceCurseforgePack(...args);
      setPackFor(null);
      pushToast(
        packFormat === "modrinth" ? t("Modrinth 整合包已导出") : t("CurseForge 整合包已导出"),
        summary.warnings.length ? "warning" : "success",
        t("{0} 个模组写入索引 · {1} 个放入 overrides · {2}", [summary.resolvedMods, summary.bundledMods, formatBytes(summary.bytes)]),
      );
    } catch (e) {
      setFormError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  function choosePackFormat(next: "modrinth" | "curseforge") {
    setPackFormat(next);
    setPackPath((prev) => {
      const stem = prev.replace(/\.(mrpack|zip)$/i, "").trim();
      return `${stem || packFor?.name || "modpack"}.${next === "modrinth" ? "mrpack" : "zip"}`;
    });
    setFormError(null);
  }

  async function checkImport() {
    setChecking(true);
    setImportCheck(null);
    setFormError(null);
    try {
      const check = await api.inspectInstanceArchive(importPath.trim());
      setImportCheck(check);
      setImportName(check.manifest.name);
    } catch (e) {
      setFormError(e instanceof Error ? e.message : String(e));
    } finally {
      setChecking(false);
    }
  }

  async function doImport() {
    if (!importCheck) return;
    setBusy("import");
    try {
      const inst = await api.importInstanceArchive(
        importPath.trim(),
        importName.trim() || undefined,
      );
      setImportOpen(false);
      setImportPath("");
      setImportCheck(null);
      pushToast(t("实例已导入"), "success", inst.name);
      await reload();
    } catch (e) {
      setFormError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="page">
      <div className="toolbar">
        <div>
          <h2>{t("游戏实例")}</h2>
          <p className="muted">{t("每个实例有独立的游戏目录与启动配置，互不影响。")}</p>
        </div>
        <div className="toolbar__actions">
          <Button
            icon="box"
            onClick={() => {
              setImportOpen(true);
              setImportCheck(null);
              setImportPath("");
              setImportName("");
              setFormError(null);
            }}
          >
            {t("导入实例")}</Button>
          <Button icon="plus" variant="primary" onClick={openCreate} disabled={versions.length === 0}>
            {t("新建实例")}</Button>
        </div>
      </div>

      {versions.length === 0 ? (
        <Card>
          <EmptyState
            icon="layers"
            title={t("还没有已安装的版本")}
            description={t("创建实例前需要先安装一个 Minecraft 版本。安装过程会真实下载并校验文件。")}
            action={
              <Button variant="primary" icon="download" onClick={() => setPage("versions")}>
                {t("前往版本管理")}</Button>
            }
          />
        </Card>
      ) : instances.length === 0 ? (
        <Card>
          <EmptyState
            icon="box"
            title={t("还没有游戏实例")}
            description={t("新建一个实例，选择已安装的版本与内存配置。")}
            action={
              <Button variant="primary" icon="plus" onClick={openCreate}>
                {t("新建实例")}</Button>
            }
          />
        </Card>
      ) : (
        <div className="instance-grid">
          {instances.map((inst) => {
            const isRunning = running.has(inst.id);
            return (
              <Card key={inst.id} className={`instance-card ${isRunning ? "instance-card--running" : ""}`}>
                <div className="instance-card__head">
                  <div className="instance-card__title">
                    <div className="instance-avatar" aria-hidden="true">
                      {inst.name.slice(0, 1).toUpperCase()}
                    </div>
                    <div>
                      <h3>{inst.name}</h3>
                      <span className="mono muted">{inst.id}</span>
                    </div>
                  </div>
                  <Badge tone={isRunning ? "success" : "neutral"}>{isRunning ? t("运行中") : t("空闲")}</Badge>
                </div>

                <dl className="instance-meta">
                  <div>
                    <dt>{t("版本")}</dt>
                    <dd>{inst.versionId}</dd>
                  </div>
                  <div>
                    <dt>{t("加载器")}</dt>
                    <dd>
                      {inst.modLoader
                        ? `${inst.modLoader.kind} ${inst.modLoader.version}`
                        : t("原版")}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("内存")}</dt>
                    <dd>{inst.memoryMb} MB</dd>
                  </div>
                  <div>
                    <dt>Java</dt>
                    <dd className="ellipsis" title={inst.javaPath ?? t("自动选择")}>
                      {inst.javaPath ?? t("自动选择")}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("分辨率")}</dt>
                    <dd>
                      {inst.resolutionWidth && inst.resolutionHeight
                        ? `${inst.resolutionWidth} × ${inst.resolutionHeight}`
                        : t("默认")}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("上次启动")}</dt>
                    <dd>{relativeTime(inst.lastLaunchedAt)}</dd>
                  </div>
                </dl>

                <p className="instance-path mono ellipsis" title={inst.gameDir}>
                  {inst.gameDir}
                </p>

                <div className="instance-card__actions">
                  {isRunning ? (
                    <Button size="sm" variant="danger" icon="stop" onClick={() => stop(inst)}>
                      {t("终止")}</Button>
                  ) : (
                    <Button
                      size="sm"
                      variant="primary"
                      icon="play"
                      loading={busy === inst.id}
                      onClick={() => launch(inst)}
                    >
                      {t("启动")}</Button>
                  )}
                  <Button size="sm" icon="edit" onClick={() => openEdit(inst)}>
                    {t("设置")}</Button>
                  <IconButton
                    icon="copy"
                    label={t("复制实例")}
                    onClick={() => duplicate(inst)}
                    disabled={busy === inst.id}
                  />
                  <IconButton
                    icon="map"
                    label={t("地图")}
                    onClick={() => setMapsFor(inst)}
                  />
                  <IconButton
                    icon="palette"
                    label={t("材质")}
                    onClick={() => setTexturesFor(inst)}
                  />
                  <IconButton
                    icon="plug"
                    label={t("管理模组")}
                    onClick={() => setModsFor(inst)}
                  />
                  <IconButton
                    icon="sun"
                    label={t("光影管理")}
                    onClick={() => setShadersFor(inst)}
                  />
                  <IconButton
                    icon="download"
                    label={t("导出实例")}
                    onClick={() => {
                      setExporting(inst);
                      setExportPath(`${inst.name}.zip`);
                      setOverwrite(false);
                      setFormError(null);
                    }}
                  />
                  <IconButton
                    icon="box"
                    label={t("导出整合包")}
                    onClick={() => {
                      setPackFor(inst);
                      setPackFormat("modrinth");
                      setPackPath(`${inst.name}.mrpack`);
                      setPackName(inst.name);
                      setPackVersion("1.0.0");
                      setPackOverwrite(false);
                      setFormError(null);
                    }}
                  />
                  <IconButton
                    icon="trash"
                    label={t("删除实例")}
                    onClick={() => {
                      setDeleting(inst);
                      setDeleteFiles(false);
                    }}
                  />
                </div>
                <span className="instance-card__created muted">{t("创建于")}{formatDate(inst.createdAt)}</span>
              </Card>
            );
          })}
        </div>
      )}

      <Modal
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        title={t("新建实例")}
        subtitle={t("实例目录创建在游戏目录的 instances/ 下。")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setCreateOpen(false)}>
              {t("取消")}</Button>
            <Button variant="primary" loading={busy === "create"} onClick={create}>
              {t("创建")}</Button>
          </>
        }
      >
        <Field label={t("实例名称")} htmlFor="inst-name">
          <input
            id="inst-name"
            value={name}
            maxLength={64}
            placeholder={t("例如：生存 1.20")}
            onChange={(e) => setName(e.target.value)}
          />
        </Field>
        <Field label={t("Minecraft 版本")} hint={t("仅列出本地已安装的版本")}>
          <select value={versionId} onChange={(e) => setVersionId(e.target.value)}>
            {versions.map((v) => (
              <option key={v.id} value={v.id}>
                {v.id} {v.versionType ? `(${v.versionType})` : ""}
                {v.hasJar ? "" : t(" · 缺少 JAR")}
              </option>
            ))}
          </select>
        </Field>
        <Field label={t("最大内存 (MB)")} hint={t("范围 512 - 65536")}>
          <input
            type="number"
            min={512}
            max={65536}
            step={256}
            value={memory}
            onChange={(e) => setMemory(Number(e.target.value))}
          />
        </Field>
        {formError ? <p className="form-error">{formError}</p> : null}
      </Modal>

      <Modal
        open={editing !== null}
        onClose={() => setEditing(null)}
        title={t("实例设置")}
        subtitle={editing ? `${editing.name} · ${editing.versionId}` : ""}
        footer={
          <>
            <Button variant="ghost" onClick={() => setEditing(null)}>
              {t("取消")}</Button>
            <Button variant="primary" loading={busy === "edit"} onClick={saveEdit}>
              {t("保存")}</Button>
          </>
        }
      >
        <div className="form-grid">
          <Field label={t("实例名称")}>
            <input value={editName} maxLength={64} onChange={(e) => setEditName(e.target.value)} />
          </Field>
          <Field label={t("最大内存 (MB)")}>
            <input
              type="number"
              min={512}
              max={65536}
              step={256}
              value={editMemory}
              onChange={(e) => setEditMemory(Number(e.target.value))}
            />
          </Field>
        </div>
        <Field label={t("Java 可执行文件")} hint={t("留空即可：启动器按版本自动匹配（1.12.2→Java 8，1.21.1→Java 21，26.3→Java 25），不会用 Java 25 硬跑旧版本")}>
          <select value={editJava} onChange={(e) => setEditJava(e.target.value)}>
            <option value="">{t("自动选择")}</option>
            {javasSorted.map((j) => (
              <option key={j.path} value={j.path}>
                Java {j.major} — {j.path}
              </option>
            ))}
            {editJava && !javas.some((j) => j.path === editJava) ? (
              <option value={editJava}>{editJava}{t("（未检测）")}</option>
            ) : null}
          </select>
        </Field>
        <div className="form-grid">
          <Field label={t("账号")} hint={t("留空则跟随当前账号")}>
            <select value={editAccount} onChange={(e) => setEditAccount(e.target.value)}>
              <option value="">{t("跟随当前账号")}</option>
              {(bootstrap?.accounts ?? []).map((a) => (
                <option key={a.id} value={a.id}>
                  {a.username}（{a.kind === "offline" ? t("离线") : "Microsoft"}）
                </option>
              ))}
            </select>
          </Field>
          <Field label={t("窗口分辨率")} hint={t("留空使用游戏默认；宽 320-7680，高 240-4320")}>
            <div className="resolution-inputs">
              <input
                type="number"
                min={320}
                max={7680}
                placeholder={t("宽")}
                value={editWidth}
                onChange={(e) => setEditWidth(e.target.value)}
              />
              <span>×</span>
              <input
                type="number"
                min={240}
                max={4320}
                placeholder={t("高")}
                value={editHeight}
                onChange={(e) => setEditHeight(e.target.value)}
              />
            </div>
          </Field>
        </div>
        <div className="form-grid">
          <Field label={t("额外 JVM 参数")} hint={t("每行一个，原样传给 Java")}>
            <textarea rows={4} value={editJvm} onChange={(e) => setEditJvm(e.target.value)} placeholder="-Dfoo=bar" />
          </Field>
          <Field label={t("额外游戏参数")} hint={t("每行一个，追加到主类之后")}>
            <textarea rows={4} value={editGame} onChange={(e) => setEditGame(e.target.value)} placeholder="--demo" />
          </Field>
        </div>
        <p className="muted small">
          {t("实例目录：")}<span className="mono">{editing?.gameDir}</span>
        </p>
        {formError ? <p className="form-error">{formError}</p> : null}
      </Modal>

      <Modal
        open={deleting !== null}
        onClose={() => setDeleting(null)}
        title={t("删除实例")}
        subtitle={deleting?.name}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleting(null)}>
              {t("取消")}</Button>
            <Button variant="danger" loading={busy === deleting?.id} onClick={confirmDelete}>
              {t("确认删除")}</Button>
          </>
        }
      >
        <p>
          {t("将删除实例配置")}<span className="mono">{deleting?.id}</span>{t("。此操作不可撤销。")}</p>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={deleteFiles}
            onChange={(e) => setDeleteFiles(e.target.checked)}
          />
          <span>
            {t("同时删除实例目录中的全部文件（存档、模组、配置）")}<small className="mono">{deleting?.gameDir}</small>
          </span>
        </label>
        {!deleteFiles ? (
          <p className="notice notice--warning">
            <Icon name="info" size={16} />
            {t("不勾选时，仅移除启动器中的实例记录，游戏文件会保留在磁盘上。")}</p>
        ) : (
          <p className="form-error">{t("危险：该目录会被递归删除，且不会进入回收站。")}</p>
        )}
      </Modal>

      <Modal
        open={exporting !== null}
        onClose={() => setExporting(null)}
        title={t("导出实例")}
        subtitle={exporting ? `${exporting.name} · ${exporting.versionId}` : ""}
        footer={
          <>
            <Button variant="ghost" onClick={() => setExporting(null)}>
              {t("取消")}</Button>
            <Button
              variant="primary"
              loading={busy === exporting?.id}
              onClick={doExport}
              disabled={!exportPath.trim()}
            >
              {t("导出")}</Button>
          </>
        }
      >
        <Field
          label={t("导出文件路径")}
          hint={t("必须为 .zip；不包含日志、崩溃报告或任何账号令牌。")}
        >
          <input
            className="mono"
            value={exportPath}
            onChange={(e) => setExportPath(e.target.value)}
            placeholder="/Users/you/Desktop/instance.zip"
          />
        </Field>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={overwrite}
            onChange={(e) => setOverwrite(e.target.checked)}
          />
          <span>{t("允许覆盖已存在的文件")}</span>
        </label>
        <p className="muted small">
          {t("默认包含存档、模组、配置、资源包与光影；排除")}<span className="mono">logs/</span>、
          <span className="mono">crash-reports/</span> {t("和启动器内部配置。")}</p>
        {formError ? <p className="form-error">{formError}</p> : null}
      </Modal>

      <Modal
        open={packFor !== null}
        onClose={() => setPackFor(null)}
        title={t("导出整合包")}
        subtitle={packFor ? `${packFor.name} · ${packFor.versionId}` : ""}
        footer={
          <>
            <Button variant="ghost" onClick={() => setPackFor(null)}>
              {t("取消")}</Button>
            <Button
              variant="primary"
              loading={busy === "modpack"}
              onClick={doPackExport}
              disabled={!packPath.trim()}
            >
              {packFormat === "modrinth" ? t("导出 .mrpack") : t("导出 ZIP")}
            </Button>
          </>
        }
      >
        <Field label={t("导出格式")}>
          <div className="inline-form" role="radiogroup" aria-label={t("整合包格式")}>
            <Button
              size="sm"
              variant={packFormat === "modrinth" ? "primary" : "ghost"}
              onClick={() => choosePackFormat("modrinth")}
            >
              Modrinth .mrpack
            </Button>
            <Button
              size="sm"
              variant={packFormat === "curseforge" ? "primary" : "ghost"}
              onClick={() => choosePackFormat("curseforge")}
            >
              CurseForge ZIP
            </Button>
          </div>
        </Field>
        <Field label={t("整合包名称")}>
          <input
            value={packName}
            onChange={(e) => setPackName(e.target.value)}
            placeholder={packFor?.name ?? t("整合包名称")}
          />
        </Field>
        <Field label={t("整合包版本")} hint={t("用于区分同一整合包的不同发布版本。")}>
          <input
            value={packVersion}
            onChange={(e) => setPackVersion(e.target.value)}
            placeholder="1.0.0"
          />
        </Field>
        <Field
          label={t("导出文件路径")}
          hint={
            packFormat === "modrinth"
              ? t("必须为 .mrpack；可在 Modrinth 客户端、Prism、ATLauncher 等启动器中导入。")
              : t("必须为 .zip；包含 manifest.json，可在 CurseForge App、Prism 等启动器中导入。")
          }
        >
          <input
            className="mono"
            value={packPath}
            onChange={(e) => setPackPath(e.target.value)}
            placeholder={
              packFormat === "modrinth"
                ? "/Users/you/Desktop/modpack.mrpack"
                : "/Users/you/Desktop/modpack.zip"
            }
          />
        </Field>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={packOverwrite}
            onChange={(e) => setPackOverwrite(e.target.checked)}
          />
          <span>{t("允许覆盖已存在的文件")}</span>
        </label>
        <p className="muted small">
          {packFormat === "modrinth" ? (
            <>
              {t("能在 Modrinth 识别到的模组会写入")}{" "}
              <span className="mono">modrinth.index.json</span>{t("，由导入方重新下载；")}</>
          ) : (
            <>
              {t("能通过 CurseForge 官方指纹识别的模组会写入")}{" "}
              <span className="mono">manifest.json</span> {t("的 projectID / fileID；")}</>
          )}
          {t("其余模组和配置、资源包、光影会打包进")}<span className="mono">overrides/</span>{t("。 不会包含存档、日志或账号令牌。")}</p>
        {formError ? <p className="form-error">{formError}</p> : null}
      </Modal>

      <Modal
        open={importOpen}
        onClose={() => setImportOpen(false)}
        title={t("导入实例")}
        subtitle={t("导入前会检查压缩包路径、大小与版本依赖。")}
        wide
        footer={
          <>
            <Button variant="ghost" onClick={() => setImportOpen(false)}>
              {t("取消")}</Button>
            <Button
              variant="primary"
              loading={busy === "import"}
              onClick={doImport}
              disabled={!importCheck || !importCheck.versionInstalled}
            >
              {t("导入为新实例")}</Button>
          </>
        }
      >
        <div className="inline-form">
          <Field label={t("实例归档路径")}>
            <input
              className="mono"
              value={importPath}
              onChange={(e) => {
                setImportPath(e.target.value);
                setImportCheck(null);
              }}
              placeholder="/Users/you/Desktop/instance.zip"
            />
          </Field>
          <Button onClick={checkImport} loading={checking} disabled={!importPath.trim()}>
            {t("检查归档")}</Button>
        </div>
        {importCheck ? (
          <>
            <div className="detail-list import-preview">
              <div>
                <span>{t("实例名称")}</span>
                <strong>{importCheck.manifest.name}</strong>
              </div>
              <div>
                <span>Minecraft</span>
                <strong>{importCheck.manifest.versionId}</strong>
              </div>
              <div>
                <span>{t("文件")}</span>
                <strong>
                  {importCheck.fileCount} {t("个 ·")}{formatBytes(importCheck.totalBytes)}
                </strong>
              </div>
              <div>
                <span>{t("导出时间")}</span>
                <strong>{formatDate(importCheck.manifest.exportedAt)}</strong>
              </div>
              <div>
                <span>{t("版本状态")}</span>
                <strong>
                  {importCheck.versionInstalled
                    ? t("已安装")
                    : t("未安装，请先安装 {0}", [importCheck.manifest.versionId])}
                </strong>
              </div>
              {importCheck.manifest.modLoader ? (
                <div>
                  <span>{t("加载器")}</span>
                  <strong>
                    {importCheck.manifest.modLoader.kind}{" "}
                    {importCheck.manifest.modLoader.version}
                  </strong>
                </div>
              ) : null}
            </div>
            {importCheck.warnings.map((w) => (
              <p className="notice notice--warning" key={w}>
                {w}
              </p>
            ))}
            <Field label={t("新实例名称")}>
              <input
                value={importName}
                maxLength={64}
                onChange={(e) => setImportName(e.target.value)}
              />
            </Field>
          </>
        ) : null}
        {formError ? <p className="form-error">{formError}</p> : null}
      </Modal>

      {modsFor ? <ModManager instance={modsFor} onClose={() => setModsFor(null)} /> : null}
      {shadersFor ? <ShaderManager instance={shadersFor} onClose={() => setShadersFor(null)} /> : null}
      {mapsFor ? <MapManager instance={mapsFor} onClose={() => setMapsFor(null)} /> : null}
      {texturesFor ? (
        <ResourcePackManager instance={texturesFor} onClose={() => setTexturesFor(null)} />
      ) : null}
      {javaPrompt ? (
        <JavaInstallPrompt
          instanceId={javaPrompt.id}
          instanceName={javaPrompt.name}
          onClose={() => setJavaPrompt(null)}
          onInstalled={() => {
            const target = javaPrompt;
            setJavaPrompt(null);
            void launch(target);
          }}
        />
      ) : null}
    </div>
  );
}
