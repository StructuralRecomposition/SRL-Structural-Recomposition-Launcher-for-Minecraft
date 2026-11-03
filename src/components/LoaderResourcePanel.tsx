import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type GameVersionInfo,
  type Instance,
  type LoaderVersionOption,
  type ResourceTaskAccepted,
} from "../lib/api";
import { formatBytes } from "../lib/format";
import { isActive, taskProgress, taskTone, TASK_LABELS } from "../lib/task";
import { baseVersionOfInstance, instanceLoaderKind } from "../lib/resourceTags";
import { useApp } from "../state/AppContext";
import { Badge, Button, Field, ProgressBar } from "../components/ui";
import { Modal } from "../components/Modal";

const LOADERS = [
  { id: "fabric", label: "Fabric", hint: "轻量、更新快，适合大多数 Fabric 模组" },
  { id: "quilt", label: "Quilt", hint: "Fabric 生态衍生加载器，兼容大部分 Fabric 模组" },
  { id: "forge", label: "Forge", hint: "老牌加载器，适合大型整合包；安装时会运行官方安装器" },
  { id: "neoforge", label: "NeoForge", hint: "Forge 分支，适合 1.20.2+ 的新整合包" },
  { id: "iris", label: "Iris", hint: "Fabric/Quilt 光影模组，安装到实例 mods/" },
  { id: "optifine", label: "OptiFine", hint: "仅支持 Forge 实例，作为模组安装到 mods/" },
] as const;

type LoaderId = (typeof LOADERS)[number]["id"];

function loaderLabel(loader: LoaderId) {
  return LOADERS.find((item) => item.id === loader)?.label ?? loader;
}

function optionKey(option: LoaderVersionOption) {
  return `${option.version}::${option.variant ?? ""}`;
}

export function LoaderResourcePanel() {
  const { bootstrap, pushToast, reload } = useApp();
  const instances = bootstrap?.instances ?? [];
  const [loader, setLoader] = useState<LoaderId>("fabric");
  const [gameVersions, setGameVersions] = useState<GameVersionInfo[]>([]);
  const [gameVersion, setGameVersion] = useState("");
  const [versions, setVersions] = useState<LoaderVersionOption[]>([]);
  const [selected, setSelected] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [target, setTarget] = useState<"instance" | "directory" | null>(null);
  const sequence = useRef(0);

  useEffect(() => {
    api
      .resourceGameVersions()
      .then((list) => {
        setGameVersions(list ?? []);
        setGameVersion((current) => {
          if (current) return current;
          return list?.find((v) => v.versionType === "release")?.version ?? "";
        });
      })
      .catch(() => {
        /* the manual selector below still works */
      });
  }, []);

  useEffect(() => {
    if (!gameVersion) return;
    const id = ++sequence.current;
    setLoading(true);
    setError(null);
    setVersions([]);
    setSelected("");
    api
      .loaderResourceVersions(loader, gameVersion)
      .then((list) => {
        if (id !== sequence.current) return;
        const next = list ?? [];
        setVersions(next);
        const first = next.find((v) => v.stable) ?? next[0];
        setSelected(first ? optionKey(first) : "");
        if (next.length === 0) setError("该 Minecraft 版本没有可用版本");
      })
      .catch((e) => {
        if (id !== sequence.current) return;
        setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (id === sequence.current) setLoading(false);
      });
  }, [loader, gameVersion]);

  const current = versions.find((v) => optionKey(v) === selected) ?? null;
  const releases = gameVersions.filter((v) => v.versionType === "release");
  const snapshots = gameVersions.filter((v) => v.versionType !== "release");
  const loaderHint = LOADERS.find((item) => item.id === loader)?.hint ?? "";

  return (
    <div className="loader-resource">
      <div className="resource-tag-row" aria-label="加载器类型">
        {LOADERS.map((item) => (
          <button
            key={item.id}
            type="button"
            className={`resource-tag ${loader === item.id ? "resource-tag--active" : ""}`}
            onClick={() => setLoader(item.id)}
          >
            {item.label}
          </button>
        ))}
      </div>

      <div className="loader-resource__form">
        <Field label="Minecraft 版本" hint="可自由选择，不需要先安装原版客户端。">
          <select value={gameVersion} onChange={(e) => setGameVersion(e.target.value)}>
            <option value="">选择 Minecraft 版本</option>
            {releases.length > 0 ? (
              <optgroup label="正式版">
                {releases.map((v) => (
                  <option key={`r-${v.version}`} value={v.version}>
                    {v.version}
                  </option>
                ))}
              </optgroup>
            ) : null}
            {snapshots.length > 0 ? (
              <optgroup label="快照 / 旧版">
                {snapshots.map((v) => (
                  <option key={`s-${v.version}`} value={v.version}>
                    {v.version}
                  </option>
                ))}
              </optgroup>
            ) : null}
          </select>
        </Field>

        <Field
          label={`${loaderLabel(loader)} 版本`}
          hint={loading ? "正在获取版本…" : `${versions.length} 个可用版本`}
        >
          <select
            value={selected}
            onChange={(e) => setSelected(e.target.value)}
            disabled={loading || versions.length === 0}
          >
            {versions.map((v) => (
              <option key={optionKey(v)} value={optionKey(v)}>
                {v.label}
                {v.stable === false ? "（测试）" : ""}
              </option>
            ))}
          </select>
        </Field>
      </div>

      <p className="muted small">
        {loaderHint}
        {loader === "fabric" || loader === "quilt"
          ? " 下载到本地会保存官方 profile JSON；安装到实例会先安装该加载器版本并切换实例。"
          : loader === "forge" || loader === "neoforge"
            ? " 下载到本地会保存官方安装器 JAR；安装到实例会运行官方安装器并把实例切换到该版本。"
            : loader === "iris"
              ? " 需要 Fabric/Quilt 实例；只会把对应 Iris JAR 放进 mods/。"
              : " 需要 Forge 实例；只会把 OptiFine 文件放进 mods/，不运行 GUI 安装器。"}
      </p>

      <div className="loader-resource__actions">
        <Button
          variant="primary"
          icon="download"
          disabled={!current || loading}
          onClick={() => setTarget("instance")}
        >
          安装到实例
        </Button>
        <Button
          icon="download"
          disabled={!current || loading}
          onClick={() => setTarget("directory")}
        >
          下载到本地
        </Button>
      </div>

      {error ? <p className="form-error">{error}</p> : null}

      {target && current ? (
        <LoaderTargetDialog
          mode={target}
          loader={loader}
          loaderLabel={loaderLabel(loader)}
          gameVersion={gameVersion}
          option={current}
          instances={instances}
          onClose={() => setTarget(null)}
          onFinished={async () => {
            setTarget(null);
            await reload();
          }}
          pushToast={pushToast}
        />
      ) : null}
    </div>
  );
}

function LoaderTargetDialog({
  mode,
  loader,
  loaderLabel,
  gameVersion,
  option,
  instances,
  onClose,
  onFinished,
  pushToast,
}: {
  mode: "instance" | "directory";
  loader: LoaderId;
  loaderLabel: string;
  gameVersion: string;
  option: LoaderVersionOption;
  instances: Instance[];
  onClose: () => void;
  onFinished: () => Promise<void>;
  pushToast: (title: string, tone: "success" | "error" | "info" | "warning", detail?: string) => void;
}) {
  const { tasks } = useApp();
  const [instanceId, setInstanceId] = useState("");
  const [dir, setDir] = useState("");
  const [accepted, setAccepted] = useState<ResourceTaskAccepted | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const done = useRef(false);

  const eligible = useMemo(() => {
    return instances.map((instance) => {
      const base = baseVersionOfInstance(instance);
      const versionOk = base === gameVersion;
      const loaderKind = instanceLoaderKind(instance);
      const loaderOk =
        loader === "iris"
          ? loaderKind === "fabric" || loaderKind === "quilt"
          : loader === "optifine"
            ? loaderKind === "forge"
            : true;
      return { instance, ok: versionOk && loaderOk, versionOk, loaderOk };
    });
  }, [instances, gameVersion, loader]);
  const firstOk = eligible.find((row) => row.ok)?.instance;

  useEffect(() => {
    api
      .defaultResourceDir()
      .then((value) => setDir(value))
      .catch(() => {
        /* user can type a path */
      });
  }, []);

  useEffect(() => {
    if (!instanceId && firstOk) setInstanceId(firstOk.id);
  }, [instanceId, firstOk]);

  const task = accepted ? tasks.find((t) => t.id === accepted.taskId) : undefined;

  useEffect(() => {
    if (task?.status === "completed" && !done.current) {
      done.current = true;
      pushToast(
        mode === "instance" ? `${loaderLabel} 已安装` : `${loaderLabel} 已下载`,
        "success",
        `${option.label} · ${gameVersion}`,
      );
      void onFinished();
    } else if (task?.status === "failed" && !done.current) {
      done.current = true;
      setError(task.error?.message ?? "安装失败");
    }
  }, [
    task?.status,
    task?.title,
    task?.error,
    mode,
    loaderLabel,
    option.label,
    gameVersion,
    onFinished,
    pushToast,
  ]);

  async function start() {
    setError(null);
    setStarting(true);
    try {
      if (mode === "instance") {
        if (!instanceId) throw new Error("请选择目标实例");
        const result = await api.installLoaderResource(
          loader,
          gameVersion,
          option.version,
          option.variant,
          instanceId,
        );
        setAccepted(result);
      } else {
        if (!dir.trim()) throw new Error("请选择下载目录");
        const result = await api.downloadLoaderResource(
          loader,
          gameVersion,
          option.version,
          option.variant,
          dir.trim(),
        );
        setAccepted(result);
        pushToast("下载任务已开始", "info", option.label);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setStarting(false);
    }
  }

  return (
    <Modal
      open
      onClose={accepted && isActive(task?.status ?? "queued") ? () => undefined : onClose}
      title={mode === "instance" ? `安装 ${loaderLabel}` : `下载 ${loaderLabel}`}
      subtitle={`${option.label} · Minecraft ${gameVersion}`}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={Boolean(accepted)}>
            关闭
          </Button>
          <Button
            variant="primary"
            icon="download"
            loading={starting}
            disabled={accepted !== null}
            onClick={() => void start()}
          >
            {mode === "instance" ? "开始安装" : "开始下载"}
          </Button>
        </>
      }
    >
      {mode === "instance" ? (
        <>
          <div className="notice notice--warning">
            <p>
              安装会把目标实例切换到 <span className="mono">{loaderLabel}</span>{" "}
              版本，mods/ 与存档不会被删除。实例的基础版本必须与所选 Minecraft 一致。
            </p>
          </div>
          <Field label="目标实例">
            <select value={instanceId} onChange={(e) => setInstanceId(e.target.value)}>
              <option value="">选择实例</option>
              {eligible.map(({ instance, ok }) => (
                <option key={instance.id} value={instance.id} disabled={!ok}>
                  {instance.name} · {instance.versionId}
                  {ok ? "" : "（版本不匹配）"}
                </option>
              ))}
            </select>
          </Field>
          {eligible.every((row) => !row.ok) ? (
            <p className="form-error">
              没有基础版本为 {gameVersion} 的可用实例。请先在版本管理中安装并创建实例，或改为下载到本地。
            </p>
          ) : null}
        </>
      ) : (
        <Field label="下载目录" hint="会保存官方加载器文件；不会自动安装到游戏。">
          <input
            value={dir}
            onChange={(e) => setDir(e.target.value)}
            className="mono"
            placeholder="/Users/you/Downloads"
          />
        </Field>
      )}

      {error ? <p className="form-error">{error}</p> : null}

      {accepted ? (
        <div className="loader-task">
          <div className="resource-task__head">
            <Badge tone={taskTone(task?.status ?? "queued")}>
              {TASK_LABELS[task?.status ?? "queued"]}
            </Badge>
            <span className="ellipsis">{task?.currentFile ?? accepted.title}</span>
          </div>
          {task ? (
            <>
              <ProgressBar
                value={taskProgress(task)}
                label={
                  task.totalBytes > 0
                    ? `${formatBytes(task.doneBytes)} / ${formatBytes(task.totalBytes)}`
                    : `已处理 ${formatBytes(task.doneBytes)}`
                }
              />
              <span className="muted small">
                {task.doneFiles}/{task.totalFiles} 个文件
              </span>
            </>
          ) : (
            <p className="muted small">正在创建任务…</p>
          )}
        </div>
      ) : null}
    </Modal>
  );
}
