import { useEffect, useMemo, useState } from "react";
import {
  api,
  type LoaderInstallAccepted,
  type LoaderVersionOption,
  type ModLoaderInfo,
} from "../lib/api";
import { useApp } from "../state/AppContext";
import { Badge, Button, Field, ProgressBar } from "./ui";
import { Modal } from "./Modal";
import { taskProgress } from "../lib/task";
import { formatBytes, formatSpeed } from "../lib/format";
import { t as tr } from "../lib/i18n";


type LoaderId = "none" | "fabric" | "quilt" | "forge" | "neoforge";
type ShaderId = "none" | "iris" | "optifine";

const LOADER_LABELS: Record<LoaderId, string> = {
  none: tr("原版（不安装加载器）"),
  fabric: "Fabric",
  quilt: "Quilt",
  forge: "Forge",
  neoforge: "NeoForge",
};

const LOADER_KIND: Record<Exclude<LoaderId, "none">, string> = {
  fabric: "Fabric",
  quilt: "Quilt",
  forge: "Forge",
  neoforge: "NeoForge",
};

export function InstallVersionModal({
  versionId,
  versionType,
  defaultAssets,
  onClose,
}: {
  versionId: string;
  versionType: string;
  defaultAssets: boolean;
  onClose: () => void;
}) {
  const { bootstrap, tasks, reload, pushToast, setPage, setReloadPaused } = useApp();
  const alreadyInstalled = Boolean(
    bootstrap?.installedVersions.some((v) => v.id === versionId),
  );

  const [assets, setAssets] = useState(defaultAssets);
  const [loader, setLoader] = useState<LoaderId>("none");
  const [loaderVersions, setLoaderVersions] = useState<string[]>([]);
  const [loaderVersion, setLoaderVersion] = useState("");
  const [loadingLoaders, setLoadingLoaders] = useState(false);
  const [loaderError, setLoaderError] = useState<string | null>(null);
  const [shader, setShader] = useState<ShaderId>("none");
  const [shaderVersions, setShaderVersions] = useState<LoaderVersionOption[]>([]);
  const [shaderVersion, setShaderVersion] = useState("");
  const [loadingShader, setLoadingShader] = useState(false);
  const [shaderError, setShaderError] = useState<string | null>(null);
  const [createInstance, setCreateInstance] = useState(true);
  const [name, setName] = useState(tr("{0} 实例", [versionId]));
  const [memory, setMemory] = useState(4096);
  const [installing, setInstalling] = useState(false);
  const [phase, setPhase] = useState("");
  const [activeTaskId, setActiveTaskId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const activeTask = useMemo(
    () => tasks.find((t) => t.id === activeTaskId) ?? null,
    [tasks, activeTaskId],
  );

  useEffect(() => {
    setReloadPaused(true);
    return () => setReloadPaused(false);
  }, [setReloadPaused]);

  useEffect(() => {
    if (loader === "none") {
      setLoaderVersions([]);
      setLoaderVersion("");
      setLoaderError(null);
      return;
    }
    let cancelled = false;
    setLoadingLoaders(true);
    setLoaderError(null);
    const load = async () => {
      try {
        const list =
          loader === "fabric"
            ? (await api.listFabricLoaders(versionId)).map((l) => l.version)
            : loader === "quilt"
              ? (await api.listQuiltLoaders(versionId)).map((l) => l.version)
              : loader === "forge"
                ? await api.listForgeVersions(versionId)
                : await api.listNeoForgeVersions(versionId);
        if (cancelled) return;
        const next = list ?? [];
        setLoaderVersions(next);
        setLoaderVersion(next[0] ?? "");
        if (next.length === 0) setLoaderError(tr("该版本没有可用的加载器版本"));
      } catch (e) {
        if (!cancelled) {
          setLoaderVersions([]);
          setLoaderVersion("");
          setLoaderError(e instanceof Error ? e.message : String(e));
        }
      } finally {
        if (!cancelled) setLoadingLoaders(false);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [loader, versionId]);

  useEffect(() => {
    const allowed: ShaderId[] =
      loader === "fabric" || loader === "quilt"
        ? ["none", "iris"]
        : loader === "forge"
          ? ["none", "optifine"]
          : ["none"];
    if (!allowed.includes(shader)) setShader("none");
    if (shader !== "none") setCreateInstance(true);
  }, [loader, shader]);

  useEffect(() => {
    if (shader === "none") {
      setShaderVersions([]);
      setShaderVersion("");
      setShaderError(null);
      return;
    }
    let cancelled = false;
    setLoadingShader(true);
    setShaderError(null);
    const load = async () => {
      try {
        const list = (await api.loaderResourceVersions(shader, versionId)) ?? [];
        const filtered =
          shader === "iris"
            ? list.filter((item) => item.variant === "fabric")
            : list;
        if (cancelled) return;
        setShaderVersions(filtered);
        setShaderVersion(filtered[0]?.version ?? "");
        if (filtered.length === 0) {
          setShaderError(tr("该版本没有可用的光影插件版本"));
        }
      } catch (e) {
        if (!cancelled) {
          setShaderVersions([]);
          setShaderVersion("");
          setShaderError(e instanceof Error ? e.message : String(e));
        }
      } finally {
        if (!cancelled) setLoadingShader(false);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [shader, versionId, loader]);

  async function waitForTask(taskId: string, label: string) {
    setActiveTaskId(taskId);
    const deadline = Date.now() + 45 * 60 * 1000;
    while (Date.now() < deadline) {
      const list = await api.listTasks();
      const task = list.find((t) => t.id === taskId);
      if (task) {
        setPhase(task.currentFile ?? label);
        if (task.status === "completed") return;
        if (task.status === "failed") {
          throw new Error(task.error?.message ?? tr("{0}失败", [label]));
        }
        if (task.status === "cancelled") throw new Error(tr("{0}已取消", [label]));
      }
      await new Promise((r) => window.setTimeout(r, 1000));
    }
    throw new Error(tr("{0}超时", [label]));
  }

  function loaderInfo(): ModLoaderInfo | undefined {
    if (loader === "none") return undefined;
    return {
      kind: LOADER_KIND[loader],
      version: loaderVersion,
      installed: true,
    };
  }

  async function start() {
    if (loader !== "none" && !loaderVersion) {
      setError(tr("请选择加载器版本"));
      return;
    }
    if (shader !== "none" && !shaderVersion) {
      setError(tr("请选择光影插件版本"));
      return;
    }
    setInstalling(true);
    setError(null);
    setPhase(tr("准备安装…"));
    try {
      let currentVersion = versionId;
      if (!alreadyInstalled) {
        setPhase(tr("安装 Minecraft 本体…"));
        const accepted = await api.installVersion(versionId, assets);
        await waitForTask(accepted.taskId, tr("Minecraft 安装"));
        currentVersion = accepted.versionId;
      }

      if (loader !== "none") {
        setPhase(tr("安装 {0}…", [LOADER_LABELS[loader]]));
        let accepted: LoaderInstallAccepted;
        if (loader === "fabric") {
          accepted = await api.installFabric(versionId, loaderVersion);
        } else if (loader === "quilt") {
          accepted = await api.installQuilt(versionId, loaderVersion);
        } else if (loader === "forge") {
          accepted = await api.installForge(versionId, loaderVersion);
        } else {
          accepted = await api.installNeoForge(versionId, loaderVersion);
        }
        await waitForTask(accepted.taskId, tr("{0} 安装", [LOADER_LABELS[loader]]));
        currentVersion = accepted.versionId;
      }

      let instanceId: string | undefined;
      if (createInstance || shader !== "none") {
        setPhase(tr("创建实例…"));
        const inst = await api.createInstance({
          name: name.trim() || tr("{0} 实例", [versionId]),
          versionId: currentVersion,
          memoryMb: memory,
          modLoader: loaderInfo(),
        });
        instanceId = inst.id;
      }

      if (shader !== "none" && instanceId) {
        const label = shader === "iris" ? "Iris" : "OptiFine";
        const option = shaderVersions.find((item) => item.version === shaderVersion);
        setPhase(tr("安装 {0} {1}…", [label, shaderVersion]));
        const accepted = await api.installLoaderResource(
          shader,
          versionId,
          shaderVersion,
          option?.variant ?? null,
          instanceId,
        );
        await waitForTask(accepted.taskId, tr("{0} 安装", [label]));
      }

      setReloadPaused(false);
      await reload();
      pushToast(
        tr("安装完成"),
        "success",
        `${versionId}${loader === "none" ? "" : ` + ${LOADER_LABELS[loader]}`}${shader === "none" ? "" : ` + ${shader === "iris" ? "Iris" : "OptiFine"}`}`,
      );
      setInstalling(false);
      onClose();
      if (instanceId) setPage("instances");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setReloadPaused(false);
      await reload();
      setInstalling(false);
    }
  }

  return (
    <Modal
      open
      onClose={installing ? () => undefined : onClose}
      title={tr("安装 Minecraft {0}", [versionId])}
      subtitle={tr("{0} · 可同时安装模组加载器与光影", [versionType])}
      wide
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={installing}>
            {tr("取消")}</Button>
          <Button variant="primary" loading={installing} onClick={() => void start()}>
            {tr("开始安装")}</Button>
        </>
      }
    >
      <div className="install-options">
        <label className="install-option">
          <input type="checkbox" checked={assets} onChange={(e) => setAssets(e.target.checked)} />
          <span>
            <strong>{tr("下载资源文件（约数百 MB）")}</strong>
            <small>{tr("缺少资源会影响声音与部分贴图；之后可重新安装补齐。")}</small>
          </span>
        </label>
        {alreadyInstalled ? (
          <div className="install-option install-option--static">
            <Badge tone="success">{tr("游戏本体已安装")}</Badge>
            <span>
              <strong>{tr("仅安装加载器或光影")}</strong>
              <small>{tr("不会重复下载已有的客户端文件。")}</small>
            </span>
          </div>
        ) : null}
      </div>

      <div className="form-grid">
        <Field label={tr("模组加载器")} hint={tr("不选则安装原版；选择后会把加载器与游戏一起装好")}>
          <select
            value={loader}
            onChange={(e) => setLoader(e.target.value as LoaderId)}
            disabled={installing}
          >
            {(Object.keys(LOADER_LABELS) as LoaderId[]).map((id) => (
              <option key={id} value={id}>
                {LOADER_LABELS[id]}
              </option>
            ))}
          </select>
        </Field>
        {loader !== "none" ? (
          <Field label={tr("{0} 版本", [LOADER_LABELS[loader]])} hint={loadingLoaders ? tr("正在获取版本…") : undefined}>
            <select
              value={loaderVersion}
              onChange={(e) => setLoaderVersion(e.target.value)}
              disabled={installing || loadingLoaders || loaderVersions.length === 0}
            >
              {loaderVersions.map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </Field>
        ) : null}
      </div>

      <div className="form-grid">
        <Field label={tr("光影插件")} hint={tr("Iris 用于 Fabric/Quilt，OptiFine 用于 Forge")}>
          <select
            value={shader}
            onChange={(e) => setShader(e.target.value as ShaderId)}
            disabled={installing || loader === "none" || loader === "neoforge"}
          >
            <option value="none">{tr("不安装光影")}</option>
            {loader === "fabric" || loader === "quilt" ? (
              <option value="iris">{tr("Iris（推荐）")}</option>
            ) : null}
            {loader === "forge" ? <option value="optifine">OptiFine</option> : null}
          </select>
        </Field>
        {shader !== "none" ? (
          <Field
            label={tr("{0} 版本", [shader === "iris" ? "Iris" : "OptiFine"])}
            hint={loadingShader ? tr("正在获取版本…") : undefined}
          >
            <select
              value={shaderVersion}
              onChange={(e) => setShaderVersion(e.target.value)}
              disabled={installing || loadingShader || shaderVersions.length === 0}
            >
              {shaderVersions.map((item) => (
                <option key={`${item.version}-${item.variant ?? ""}`} value={item.version}>
                  {item.label}
                </option>
              ))}
            </select>
          </Field>
        ) : null}
      </div>

      <label className="checkbox-row">
        <input
          type="checkbox"
          checked={createInstance}
          onChange={(e) => setCreateInstance(e.target.checked)}
          disabled={installing || shader !== "none"}
        />
        <span>
          {tr("安装完成后创建实例")}<small>{tr("光影会安装到实例的 mods/ 或 shaderpacks/")}</small>
        </span>
      </label>

      {createInstance ? (
        <div className="form-grid">
          <Field label={tr("实例名称")}>
            <input
              value={name}
              maxLength={64}
              onChange={(e) => setName(e.target.value)}
              disabled={installing}
            />
          </Field>
          <Field label={tr("最大内存 (MB)")}>
            <input
              type="number"
              min={512}
              max={65536}
              step={256}
              value={memory}
              onChange={(e) => setMemory(Number(e.target.value))}
              disabled={installing}
            />
          </Field>
        </div>
      ) : null}

      {loaderError ? <p className="form-error">{loaderError}</p> : null}
      {shaderError ? <p className="form-error">{shaderError}</p> : null}
      {error ? <p className="form-error">{error}</p> : null}

      {installing ? (
        <div className="install-progress">
          <strong>{phase}</strong>
          {activeTask ? (
            <>
              <ProgressBar
                value={taskProgress(activeTask)}
                label={
                  activeTask.totalBytes <= 0
                    ? tr("已下载 {0}", [formatBytes(activeTask.doneBytes)])
                    : taskProgress(activeTask) === null
                      ? tr("处理中 · {0}/{1} 个文件", [activeTask.doneFiles, activeTask.totalFiles])
                      : `${formatBytes(activeTask.doneBytes)} / ${formatBytes(activeTask.totalBytes)} · ${formatSpeed(activeTask.speedBps)}`
                }
              />
              <span className="muted small">{activeTask.currentFile ?? tr("准备中…")}</span>
            </>
          ) : null}
        </div>
      ) : null}
    </Modal>
  );
}
