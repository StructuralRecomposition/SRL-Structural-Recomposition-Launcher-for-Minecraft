import { useMemo, useState } from "react";
import { api, type ImportCheck, type Instance } from "../lib/api";
import { formatBytes, formatDate, relativeTime, splitLines } from "../lib/format";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, Field, IconButton } from "../components/ui";
import { Modal } from "../components/Modal";
import { Icon } from "../components/Icon";
import { ModManager } from "../components/ModManager";
import { ShaderManager } from "../components/ShaderManager";
import { MapManager } from "../components/MapManager";
import { ResourcePackManager } from "../components/ResourcePackManager";

export function InstancesPage() {
  const { bootstrap, reload, running, markRunning, pushToast, setPage } = useApp();
  const instances = bootstrap?.instances ?? [];
  const versions = bootstrap?.installedVersions ?? [];
  const javas = bootstrap?.java ?? [];

  const [createOpen, setCreateOpen] = useState(false);
  const [editing, setEditing] = useState<Instance | null>(null);
  const [deleting, setDeleting] = useState<Instance | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

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
      setFormError("请输入实例名称");
      return;
    }
    if (!versionId) {
      setFormError("请先安装一个游戏版本");
      return;
    }
    setBusy("create");
    try {
      await api.createInstance({ name: name.trim(), versionId, memoryMb: memory });
      setCreateOpen(false);
      pushToast("实例已创建", "success", `${name.trim()} · ${versionId}`);
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
      pushToast("实例设置已保存", "success");
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
      pushToast("游戏进程已启动", "success", `PID ${result.pid}`);
      if (result.warnings.length > 0) {
        pushToast("启动提示", "warning", result.warnings.join("；"));
      }
    } catch (e) {
      pushToast("启动失败", "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function stop(inst: Instance) {
    try {
      await api.stopInstance(inst.id);
      pushToast("正在终止游戏进程", "warning");
    } catch (e) {
      pushToast("无法终止进程", "error", e instanceof Error ? e.message : String(e));
    }
  }

  async function duplicate(inst: Instance) {
    setBusy(inst.id);
    try {
      const copy = await api.duplicateInstance(inst.id, `${inst.name} 副本`);
      pushToast("实例已复制", "success", copy.name);
      await reload();
    } catch (e) {
      pushToast("复制失败", "error", e instanceof Error ? e.message : String(e));
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
      pushToast("实例已删除", "info", deleteFiles ? "实例目录已一并删除" : "游戏文件已保留");
      await reload();
    } catch (e) {
      pushToast("删除失败", "error", e instanceof Error ? e.message : String(e));
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
        "实例已导出",
        "success",
        `${summary.files} 个文件 · ${formatBytes(summary.bytes)}${summary.skipped ? ` · 跳过 ${summary.skipped} 个符号链接` : ""}`,
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
        packFormat === "modrinth" ? "Modrinth 整合包已导出" : "CurseForge 整合包已导出",
        summary.warnings.length ? "warning" : "success",
        `${summary.resolvedMods} 个模组写入索引 · ${summary.bundledMods} 个放入 overrides · ${formatBytes(summary.bytes)}`,
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
      pushToast("实例已导入", "success", inst.name);
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
          <h2>游戏实例</h2>
          <p className="muted">每个实例有独立的游戏目录与启动配置，互不影响。</p>
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
            导入实例
          </Button>
          <Button icon="plus" variant="primary" onClick={openCreate} disabled={versions.length === 0}>
            新建实例
          </Button>
        </div>
      </div>

      {versions.length === 0 ? (
        <Card>
          <EmptyState
            icon="layers"
            title="还没有已安装的版本"
            description="创建实例前需要先安装一个 Minecraft 版本。安装过程会真实下载并校验文件。"
            action={
              <Button variant="primary" icon="download" onClick={() => setPage("versions")}>
                前往版本管理
              </Button>
            }
          />
        </Card>
      ) : instances.length === 0 ? (
        <Card>
          <EmptyState
            icon="box"
            title="还没有游戏实例"
            description="新建一个实例，选择已安装的版本与内存配置。"
            action={
              <Button variant="primary" icon="plus" onClick={openCreate}>
                新建实例
              </Button>
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
                  <Badge tone={isRunning ? "success" : "neutral"}>{isRunning ? "运行中" : "空闲"}</Badge>
                </div>

                <dl className="instance-meta">
                  <div>
                    <dt>版本</dt>
                    <dd>{inst.versionId}</dd>
                  </div>
                  <div>
                    <dt>加载器</dt>
                    <dd>
                      {inst.modLoader
                        ? `${inst.modLoader.kind} ${inst.modLoader.version}`
                        : "原版"}
                    </dd>
                  </div>
                  <div>
                    <dt>内存</dt>
                    <dd>{inst.memoryMb} MB</dd>
                  </div>
                  <div>
                    <dt>Java</dt>
                    <dd className="ellipsis" title={inst.javaPath ?? "自动选择"}>
                      {inst.javaPath ?? "自动选择"}
                    </dd>
                  </div>
                  <div>
                    <dt>分辨率</dt>
                    <dd>
                      {inst.resolutionWidth && inst.resolutionHeight
                        ? `${inst.resolutionWidth} × ${inst.resolutionHeight}`
                        : "默认"}
                    </dd>
                  </div>
                  <div>
                    <dt>上次启动</dt>
                    <dd>{relativeTime(inst.lastLaunchedAt)}</dd>
                  </div>
                </dl>

                <p className="instance-path mono ellipsis" title={inst.gameDir}>
                  {inst.gameDir}
                </p>

                <div className="instance-card__actions">
                  {isRunning ? (
                    <Button size="sm" variant="danger" icon="stop" onClick={() => stop(inst)}>
                      终止
                    </Button>
                  ) : (
                    <Button
                      size="sm"
                      variant="primary"
                      icon="play"
                      loading={busy === inst.id}
                      onClick={() => launch(inst)}
                    >
                      启动
                    </Button>
                  )}
                  <Button size="sm" icon="edit" onClick={() => openEdit(inst)}>
                    设置
                  </Button>
                  <IconButton
                    icon="copy"
                    label="复制实例"
                    onClick={() => duplicate(inst)}
                    disabled={busy === inst.id}
                  />
                  <IconButton
                    icon="map"
                    label="地图"
                    onClick={() => setMapsFor(inst)}
                  />
                  <IconButton
                    icon="palette"
                    label="材质"
                    onClick={() => setTexturesFor(inst)}
                  />
                  <IconButton
                    icon="plug"
                    label="管理模组"
                    onClick={() => setModsFor(inst)}
                  />
                  <IconButton
                    icon="sun"
                    label="光影管理"
                    onClick={() => setShadersFor(inst)}
                  />
                  <IconButton
                    icon="download"
                    label="导出实例"
                    onClick={() => {
                      setExporting(inst);
                      setExportPath(`${inst.name}.zip`);
                      setOverwrite(false);
                      setFormError(null);
                    }}
                  />
                  <IconButton
                    icon="box"
                    label="导出整合包"
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
                    label="删除实例"
                    onClick={() => {
                      setDeleting(inst);
                      setDeleteFiles(false);
                    }}
                  />
                </div>
                <span className="instance-card__created muted">创建于 {formatDate(inst.createdAt)}</span>
              </Card>
            );
          })}
        </div>
      )}

      <Modal
        open={createOpen}
        onClose={() => setCreateOpen(false)}
        title="新建实例"
        subtitle="实例目录创建在游戏目录的 instances/ 下。"
        footer={
          <>
            <Button variant="ghost" onClick={() => setCreateOpen(false)}>
              取消
            </Button>
            <Button variant="primary" loading={busy === "create"} onClick={create}>
              创建
            </Button>
          </>
        }
      >
        <Field label="实例名称" htmlFor="inst-name">
          <input
            id="inst-name"
            value={name}
            maxLength={64}
            placeholder="例如：生存 1.20"
            onChange={(e) => setName(e.target.value)}
          />
        </Field>
        <Field label="Minecraft 版本" hint="仅列出本地已安装的版本">
          <select value={versionId} onChange={(e) => setVersionId(e.target.value)}>
            {versions.map((v) => (
              <option key={v.id} value={v.id}>
                {v.id} {v.versionType ? `(${v.versionType})` : ""}
                {v.hasJar ? "" : " · 缺少 JAR"}
              </option>
            ))}
          </select>
        </Field>
        <Field label="最大内存 (MB)" hint="范围 512 - 65536">
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
        title="实例设置"
        subtitle={editing ? `${editing.name} · ${editing.versionId}` : ""}
        footer={
          <>
            <Button variant="ghost" onClick={() => setEditing(null)}>
              取消
            </Button>
            <Button variant="primary" loading={busy === "edit"} onClick={saveEdit}>
              保存
            </Button>
          </>
        }
      >
        <div className="form-grid">
          <Field label="实例名称">
            <input value={editName} maxLength={64} onChange={(e) => setEditName(e.target.value)} />
          </Field>
          <Field label="最大内存 (MB)">
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
        <Field label="Java 可执行文件" hint="留空即可：启动器按版本自动匹配（1.12.2→Java 8，1.21.1→Java 21，26.3→Java 25），不会用 Java 25 硬跑旧版本">
          <select value={editJava} onChange={(e) => setEditJava(e.target.value)}>
            <option value="">自动选择</option>
            {javasSorted.map((j) => (
              <option key={j.path} value={j.path}>
                Java {j.major} — {j.path}
              </option>
            ))}
            {editJava && !javas.some((j) => j.path === editJava) ? (
              <option value={editJava}>{editJava}（未检测）</option>
            ) : null}
          </select>
        </Field>
        <div className="form-grid">
          <Field label="账号" hint="留空则跟随当前账号">
            <select value={editAccount} onChange={(e) => setEditAccount(e.target.value)}>
              <option value="">跟随当前账号</option>
              {(bootstrap?.accounts ?? []).map((a) => (
                <option key={a.id} value={a.id}>
                  {a.username}（{a.kind === "offline" ? "离线" : "Microsoft"}）
                </option>
              ))}
            </select>
          </Field>
          <Field label="窗口分辨率" hint="留空使用游戏默认；宽 320-7680，高 240-4320">
            <div className="resolution-inputs">
              <input
                type="number"
                min={320}
                max={7680}
                placeholder="宽"
                value={editWidth}
                onChange={(e) => setEditWidth(e.target.value)}
              />
              <span>×</span>
              <input
                type="number"
                min={240}
                max={4320}
                placeholder="高"
                value={editHeight}
                onChange={(e) => setEditHeight(e.target.value)}
              />
            </div>
          </Field>
        </div>
        <div className="form-grid">
          <Field label="额外 JVM 参数" hint="每行一个，原样传给 Java">
            <textarea rows={4} value={editJvm} onChange={(e) => setEditJvm(e.target.value)} placeholder="-Dfoo=bar" />
          </Field>
          <Field label="额外游戏参数" hint="每行一个，追加到主类之后">
            <textarea rows={4} value={editGame} onChange={(e) => setEditGame(e.target.value)} placeholder="--demo" />
          </Field>
        </div>
        <p className="muted small">
          实例目录：<span className="mono">{editing?.gameDir}</span>
        </p>
        {formError ? <p className="form-error">{formError}</p> : null}
      </Modal>

      <Modal
        open={deleting !== null}
        onClose={() => setDeleting(null)}
        title="删除实例"
        subtitle={deleting?.name}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleting(null)}>
              取消
            </Button>
            <Button variant="danger" loading={busy === deleting?.id} onClick={confirmDelete}>
              确认删除
            </Button>
          </>
        }
      >
        <p>
          将删除实例配置 <span className="mono">{deleting?.id}</span>。此操作不可撤销。
        </p>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={deleteFiles}
            onChange={(e) => setDeleteFiles(e.target.checked)}
          />
          <span>
            同时删除实例目录中的全部文件（存档、模组、配置）
            <small className="mono">{deleting?.gameDir}</small>
          </span>
        </label>
        {!deleteFiles ? (
          <p className="notice notice--warning">
            <Icon name="info" size={16} />
            不勾选时，仅移除启动器中的实例记录，游戏文件会保留在磁盘上。
          </p>
        ) : (
          <p className="form-error">危险：该目录会被递归删除，且不会进入回收站。</p>
        )}
      </Modal>

      <Modal
        open={exporting !== null}
        onClose={() => setExporting(null)}
        title="导出实例"
        subtitle={exporting ? `${exporting.name} · ${exporting.versionId}` : ""}
        footer={
          <>
            <Button variant="ghost" onClick={() => setExporting(null)}>
              取消
            </Button>
            <Button
              variant="primary"
              loading={busy === exporting?.id}
              onClick={doExport}
              disabled={!exportPath.trim()}
            >
              导出
            </Button>
          </>
        }
      >
        <Field
          label="导出文件路径"
          hint="必须为 .zip；不包含日志、崩溃报告或任何账号令牌。"
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
          <span>允许覆盖已存在的文件</span>
        </label>
        <p className="muted small">
          默认包含存档、模组、配置、资源包与光影；排除 <span className="mono">logs/</span>、
          <span className="mono">crash-reports/</span> 和启动器内部配置。
        </p>
        {formError ? <p className="form-error">{formError}</p> : null}
      </Modal>

      <Modal
        open={packFor !== null}
        onClose={() => setPackFor(null)}
        title="导出整合包"
        subtitle={packFor ? `${packFor.name} · ${packFor.versionId}` : ""}
        footer={
          <>
            <Button variant="ghost" onClick={() => setPackFor(null)}>
              取消
            </Button>
            <Button
              variant="primary"
              loading={busy === "modpack"}
              onClick={doPackExport}
              disabled={!packPath.trim()}
            >
              {packFormat === "modrinth" ? "导出 .mrpack" : "导出 ZIP"}
            </Button>
          </>
        }
      >
        <Field label="导出格式">
          <div className="inline-form" role="radiogroup" aria-label="整合包格式">
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
        <Field label="整合包名称">
          <input
            value={packName}
            onChange={(e) => setPackName(e.target.value)}
            placeholder={packFor?.name ?? "整合包名称"}
          />
        </Field>
        <Field label="整合包版本" hint="用于区分同一整合包的不同发布版本。">
          <input
            value={packVersion}
            onChange={(e) => setPackVersion(e.target.value)}
            placeholder="1.0.0"
          />
        </Field>
        <Field
          label="导出文件路径"
          hint={
            packFormat === "modrinth"
              ? "必须为 .mrpack；可在 Modrinth 客户端、Prism、ATLauncher 等启动器中导入。"
              : "必须为 .zip；包含 manifest.json，可在 CurseForge App、Prism 等启动器中导入。"
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
          <span>允许覆盖已存在的文件</span>
        </label>
        <p className="muted small">
          {packFormat === "modrinth" ? (
            <>
              能在 Modrinth 识别到的模组会写入{" "}
              <span className="mono">modrinth.index.json</span>，由导入方重新下载；
            </>
          ) : (
            <>
              能通过 CurseForge 官方指纹识别的模组会写入{" "}
              <span className="mono">manifest.json</span> 的 projectID / fileID；
            </>
          )}
          其余模组和配置、资源包、光影会打包进 <span className="mono">overrides/</span>。
          不会包含存档、日志或账号令牌。
        </p>
        {formError ? <p className="form-error">{formError}</p> : null}
      </Modal>

      <Modal
        open={importOpen}
        onClose={() => setImportOpen(false)}
        title="导入实例"
        subtitle="导入前会检查压缩包路径、大小与版本依赖。"
        wide
        footer={
          <>
            <Button variant="ghost" onClick={() => setImportOpen(false)}>
              取消
            </Button>
            <Button
              variant="primary"
              loading={busy === "import"}
              onClick={doImport}
              disabled={!importCheck || !importCheck.versionInstalled}
            >
              导入为新实例
            </Button>
          </>
        }
      >
        <div className="inline-form">
          <Field label="实例归档路径">
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
            检查归档
          </Button>
        </div>
        {importCheck ? (
          <>
            <div className="detail-list import-preview">
              <div>
                <span>实例名称</span>
                <strong>{importCheck.manifest.name}</strong>
              </div>
              <div>
                <span>Minecraft</span>
                <strong>{importCheck.manifest.versionId}</strong>
              </div>
              <div>
                <span>文件</span>
                <strong>
                  {importCheck.fileCount} 个 · {formatBytes(importCheck.totalBytes)}
                </strong>
              </div>
              <div>
                <span>导出时间</span>
                <strong>{formatDate(importCheck.manifest.exportedAt)}</strong>
              </div>
              <div>
                <span>版本状态</span>
                <strong>
                  {importCheck.versionInstalled
                    ? "已安装"
                    : `未安装，请先安装 ${importCheck.manifest.versionId}`}
                </strong>
              </div>
              {importCheck.manifest.modLoader ? (
                <div>
                  <span>加载器</span>
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
            <Field label="新实例名称">
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
    </div>
  );
}
