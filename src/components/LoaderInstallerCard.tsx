import { useEffect, useMemo, useState } from "react";
import {
  api,
  type FabricLoader,
  type LoaderInstallAccepted,
} from "../lib/api";
import { formatBytes, formatSpeed } from "../lib/format";
import { TASK_LABELS, isActive, taskProgress, taskTone } from "../lib/task";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, ProgressBar } from "./ui";

interface Props {
  kind: "fabric" | "quilt";
  title: string;
  description: string;
  metaHost: string;
  stableKnown: boolean;
  verified?: boolean;
  note?: string;
  installedPrefix: string;
  listLoaders: (gameVersion: string) => Promise<FabricLoader[]>;
  install: (gameVersion: string, loaderVersion: string) => Promise<LoaderInstallAccepted>;
}

export function LoaderInstallerCard({
  kind,
  title,
  description,
  metaHost,
  stableKnown,
  verified = true,
  note,
  installedPrefix,
  listLoaders,
  install,
}: Props) {
  const { bootstrap, tasks, reload, pushToast, setPage } = useApp();
  const installed = bootstrap?.installedVersions ?? [];
  const baseVersions = useMemo(
    () => installed.filter((v) => !v.id.startsWith("fabric-loader-") && !v.id.startsWith("quilt-loader-")),
    [installed],
  );
  const loaderVersions = useMemo(
    () => installed.filter((v) => v.id.startsWith(installedPrefix)),
    [installed, installedPrefix],
  );

  const [gameVersion, setGameVersion] = useState(baseVersions[0]?.id ?? "");
  const [loaders, setLoaders] = useState<FabricLoader[]>([]);
  const [loaderVersion, setLoaderVersion] = useState("");
  const [loading, setLoading] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!baseVersions.some((v) => v.id === gameVersion)) {
      setGameVersion(baseVersions[0]?.id ?? "");
      setLoaders([]);
      setLoaderVersion("");
    }
  }, [baseVersions, gameVersion]);

  // Fetch the loader list as soon as a base version is selected. The button is
  // a manual refresh, not a prerequisite for installation.
  useEffect(() => {
    if (gameVersion) void fetchLoaders();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [gameVersion]);

  const taskTitlePrefix = `安装 ${title}`;
  const activeTask = tasks.find(
    (t) =>
      t.kind === "install" &&
      isActive(t.status) &&
      t.title.startsWith(taskTitlePrefix) &&
      t.title.includes(gameVersion) &&
      (loaderVersion === "" || t.title.includes(loaderVersion)),
  );

  async function fetchLoaders() {
    if (!gameVersion) return;
    setLoading(true);
    setError(null);
    try {
      const list = (await listLoaders(gameVersion)) ?? [];
      setLoaders(list);
      const firstStable = list.find((l) => l.stable) ?? list[0];
      setLoaderVersion(firstStable?.version ?? "");
      if (list.length === 0) {
        setError("该 Minecraft 版本没有可用的加载器");
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setLoaders([]);
      setLoaderVersion("");
    } finally {
      setLoading(false);
    }
  }

  async function runInstall() {
    if (!gameVersion || !loaderVersion) return;
    setInstalling(true);
    setError(null);
    try {
      const accepted = await install(gameVersion, loaderVersion);
      pushToast(`${title} 安装任务已开始`, "info", accepted.versionId);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      pushToast(`无法开始 ${title} 安装`, "error", message);
    } finally {
      setInstalling(false);
    }
  }

  async function createInstance(versionId: string) {
    const meta = splitLoaderId(versionId, installedPrefix, baseVersions.map((v) => v.id));
    try {
      const inst = await api.createInstance({
        name: `${versionId} 实例`,
        versionId,
        modLoader: meta.loader
          ? { kind, version: meta.loader, installed: true }
          : undefined,
      });
      pushToast("实例已创建", "success", inst.name);
      await reload();
      setPage("instances");
    } catch (e) {
      pushToast("创建实例失败", "error", e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <Card className="fabric-card">
      <div className="card-head">
        <div>
          <h3>{title}</h3>
          <p>
            {description} · 官方 meta：<span className="mono">{metaHost}</span>
          </p>
        </div>
        <Badge tone={verified ? "success" : "warning"}>
          {verified ? "已实现并测试" : "已实现 · 未完整验证"}
        </Badge>
      </div>
      {note ? <p className="muted small">{note}</p> : null}

      {baseVersions.length === 0 ? (
        <EmptyState
          icon="layers"
          title="需要先安装原版版本"
          description={`${title} 安装在已安装的原版版本之上，并共享客户端 JAR。`}
          action={
            <Button variant="primary" icon="download" onClick={() => setPage("versions")}>
              前往版本管理
            </Button>
          }
        />
      ) : (
        <>
          <div className="fabric-form">
            <label>
              <span>基础版本</span>
              <select
                value={gameVersion}
                onChange={(e) => {
                  setGameVersion(e.target.value);
                  setLoaders([]);
                  setLoaderVersion("");
                  setError(null);
                }}
              >
                {baseVersions.map((v) => (
                  <option key={v.id} value={v.id}>
                    {v.id}
                  </option>
                ))}
              </select>
            </label>
            <Button icon="refresh" onClick={fetchLoaders} loading={loading}>
              获取加载器版本
            </Button>
            {loaders.length > 0 ? (
              <>
                <label className="fabric-form__loader">
                  <span>{title} 加载器</span>
                  <select
                    value={loaderVersion}
                    onChange={(e) => setLoaderVersion(e.target.value)}
                  >
                    {loaders.slice(0, 80).map((l) => (
                      <option key={l.version} value={l.version}>
                        {l.version}
                        {stableKnown ? (l.stable ? "（稳定）" : "（测试）") : ""}
                      </option>
                    ))}
                  </select>
                </label>
                <Button
                  variant="primary"
                  icon="download"
                  onClick={runInstall}
                  loading={installing}
                  disabled={Boolean(activeTask)}
                >
                  安装 {title}
                </Button>
              </>
            ) : null}
          </div>

          <p className="muted small">
            安装仅下载加载器库文件；原版 JAR、库与资源沿用已安装的基础版本。无哈希的
            Maven 构件会先读取官方 <span className="mono">.sha1</span> sidecar 再校验。
          </p>

          {error ? <p className="form-error">{error}</p> : null}

          {activeTask ? (
            <div className="fabric-task">
              <div className="fabric-task__head">
                <strong>{activeTask.title}</strong>
                <Badge tone={taskTone(activeTask.status)}>{TASK_LABELS[activeTask.status]}</Badge>
              </div>
              <ProgressBar
                value={taskProgress(activeTask)}
                label={
                  activeTask.totalBytes <= 0
                    ? `已下载 ${formatBytes(activeTask.doneBytes)} · 总量未知`
                    : taskProgress(activeTask) === null
                      ? `下载中 · ${activeTask.doneFiles}/${activeTask.totalFiles} 个文件`
                      : `${formatBytes(activeTask.doneBytes)} / ${formatBytes(activeTask.totalBytes)} · ${formatSpeed(activeTask.speedBps)}`
                }
              />
              <span className="muted small">{activeTask.currentFile ?? "准备中…"}</span>
            </div>
          ) : null}

          {loaderVersions.length > 0 ? (
            <div className="installed-loaders">
              <h4>已安装的 {title} 版本</h4>
              <ul>
                {loaderVersions.map((v) => {
                  const meta = splitLoaderId(
                    v.id,
                    installedPrefix,
                    baseVersions.map((b) => b.id),
                  );
                  return (
                    <li key={v.id}>
                      <div>
                        <strong className="mono">{v.id}</strong>
                        <span className="muted small">
                          {title} {meta.loader} · Minecraft {meta.base || "?"}
                        </span>
                      </div>
                      <Badge tone={v.hasJar ? "neutral" : "warning"}>
                        {v.hasJar ? "共享原版 JAR" : "JAR 缺失"}
                      </Badge>
                      <Button size="sm" onClick={() => createInstance(v.id)}>
                        创建实例
                      </Button>
                    </li>
                  );
                })}
              </ul>
            </div>
          ) : null}
        </>
      )}
    </Card>
  );
}

function splitLoaderId(
  id: string,
  prefix: string,
  baseIds: string[],
): { base: string; loader: string } {
  const rest = id.startsWith(prefix) ? id.slice(prefix.length) : id;
  const base = [...baseIds]
    .sort((a, b) => b.length - a.length)
    .find((candidate) => rest.endsWith(`-${candidate}`));
  if (!base) return { base: "", loader: rest };
  return {
    base,
    loader: rest.slice(0, rest.length - base.length - 1),
  };
}
