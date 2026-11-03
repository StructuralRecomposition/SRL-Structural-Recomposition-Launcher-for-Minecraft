import { useEffect, useMemo, useState } from "react";
import { api, type LoaderRecommendation, type LoaderRecommendations } from "../lib/api";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, ProgressBar } from "./ui";
import { formatBytes, formatSpeed } from "../lib/format";
import { isActive, taskProgress } from "../lib/task";
import { Icon } from "./Icon";

const LOADER_IDS = ["fabric", "quilt", "forge", "neoforge"];
const SHADER_IDS = ["iris", "optifine"];

function tone(status: LoaderRecommendation["status"]) {
  if (status === "recommended") return "success" as const;
  if (status === "available") return "info" as const;
  if (status === "unavailable") return "warning" as const;
  return "neutral" as const;
}

function statusLabel(status: LoaderRecommendation["status"]) {
  if (status === "recommended") return "推荐";
  if (status === "available") return "可用";
  if (status === "unavailable") return "不可用";
  return "不支持";
}

export function LoaderRecommendationCard() {
  const { bootstrap, pushToast, setPage, tasks } = useApp();
  const baseVersions = useMemo(
    () =>
      (bootstrap?.installedVersions ?? []).filter(
        (v) => !v.id.startsWith("fabric-loader-") && !v.id.startsWith("quilt-loader-"),
      ),
    [bootstrap?.installedVersions],
  );
  const [version, setVersion] = useState(baseVersions[0]?.id ?? "");
  const [data, setData] = useState<LoaderRecommendations | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [picked, setPicked] = useState<Record<string, string>>({});

  useEffect(() => {
    if (!baseVersions.some((v) => v.id === version)) {
      setVersion(baseVersions[0]?.id ?? "");
    }
  }, [baseVersions, version]);

  async function load(target = version) {
    if (!target) return;
    setLoading(true);
    setError(null);
    setData(null);
    try {
      const next = await api.loaderRecommendations(target);
      setData(next);
      const defaults: Record<string, string> = {};
      for (const item of next.items) {
        if (item.versions.length > 0) defaults[item.id] = item.version ?? item.versions[0];
      }
      setPicked(defaults);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setData(null);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (version) void load(version);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version]);

  async function install(item: LoaderRecommendation) {
    if (!item.version) return;
    setBusy(item.id);
    try {
      if (item.id === "forge") {
        await api.installForge(version, picked[item.id] ?? item.version ?? undefined);
      } else if (item.id === "neoforge") {
        await api.installNeoForge(version, picked[item.id] ?? item.version ?? undefined);
      } else if (item.id === "fabric") {
        await api.installFabric(version, item.version);
      } else if (item.id === "quilt") {
        await api.installQuilt(version, item.version);
      } else {
        return;
      }
      pushToast(`${item.name} 安装任务已开始`, "info", item.version);
    } catch (e) {
      pushToast(`无法开始 ${item.name} 安装`, "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function download(item: LoaderRecommendation) {
    if (!item.downloadUrl) return;
    try {
      await api.openTrustedUrl(item.downloadUrl);
    } catch (e) {
      pushToast("无法打开下载页", "error", e instanceof Error ? e.message : String(e));
    }
  }

  function row(item: LoaderRecommendation) {
    return (
      <li key={item.id} className="reco-row">
        <div className="reco-row__main">
          <div className="reco-row__title">
            <strong>{item.name}</strong>
            <Badge tone={tone(item.status)}>{statusLabel(item.status)}</Badge>
            {item.version ? <span className="mono muted">{item.version}</span> : null}
          </div>
          <p>{item.note}</p>
          {(item.id === "forge" || item.id === "neoforge") && item.versions.length > 0 ? (
            <label className="reco-version">
              <span>版本</span>
              <select
                value={picked[item.id] ?? item.version ?? item.versions[0]}
                onChange={(e) =>
                  setPicked((prev) => ({ ...prev, [item.id]: e.target.value }))
                }
              >
                {item.versions.map((v) => (
                  <option key={v} value={v}>
                    {v}
                  </option>
                ))}
              </select>
            </label>
          ) : null}
        </div>
        <div className="reco-row__actions">
          {item.installSupported && item.version ? (
            <Button
              size="sm"
              variant="primary"
              icon="download"
              loading={busy === item.id}
              onClick={() => void install(item)}
            >
              安装
            </Button>
          ) : null}
          {item.downloadUrl && item.id === "optifine" ? (
            <Button size="sm" icon="download" onClick={() => void download(item)}>
              下载安装器
            </Button>
          ) : null}
          {item.id === "iris" ? (
            <Button size="sm" variant="ghost" onClick={() => setPage("instances")}>
              去实例光影
            </Button>
          ) : null}
        </div>
      </li>
    );
  }

  const loaders = data?.items.filter((i) => LOADER_IDS.includes(i.id)) ?? [];
  const shaders = data?.items.filter((i) => SHADER_IDS.includes(i.id)) ?? [];
  const activeTask = tasks.find(
    (t) =>
      t.kind === "install" &&
      isActive(t.status) &&
      t.title.includes(version) &&
      /Forge|Fabric|Quilt|NeoForge/.test(t.title),
  );

  return (
    <Card className="reco-card">
      <div className="card-head">
        <div>
          <h3>按版本推荐</h3>
          <p>根据已安装的游戏版本检查 Fabric / Quilt / Forge / NeoForge 与光影方案。</p>
        </div>
        <div className="reco-toolbar">
          <select
            value={version}
            onChange={(e) => setVersion(e.target.value)}
            aria-label="推荐使用的游戏版本"
          >
            {baseVersions.map((v) => (
              <option key={v.id} value={v.id}>
                {v.id}
              </option>
            ))}
          </select>
          <Button size="sm" icon="refresh" loading={loading} onClick={() => void load()}>
            重新检测
          </Button>
        </div>
      </div>

      {baseVersions.length === 0 ? (
        <EmptyState
          icon="layers"
          title="需要先安装原版版本"
          description="安装一个正式版或快照后，才能按版本推荐加载器与光影。"
          action={
            <Button variant="primary" icon="download" onClick={() => setPage("versions")}>
              前往版本管理
            </Button>
          }
        />
      ) : error ? (
        <div className="notice notice--warning">
          <Icon name="alert" size={16} />
          <div>
            <strong>无法获取推荐</strong>
            <p>{error}</p>
          </div>
          <Button size="sm" onClick={() => void load()}>
            重试
          </Button>
        </div>
      ) : loading && !data ? (
        <p className="muted">正在检测兼容版本…（Quilt 官方源较慢时会等待最多 20 秒）</p>
      ) : (
        <div className="reco-groups">
          <div>
            <h4>模组加载器</h4>
            <ul className="reco-list">{loaders.map(row)}</ul>
          </div>
          {activeTask ? (
            <div className="fabric-task">
              <div className="fabric-task__head">
                <strong>{activeTask.title}</strong>
                <Badge tone="accent">安装中</Badge>
              </div>
              <ProgressBar
                value={taskProgress(activeTask)}
                label={
                  activeTask.totalBytes <= 0
                    ? `已下载 ${formatBytes(activeTask.doneBytes)}`
                    : taskProgress(activeTask) === null
                      ? `下载中 · ${activeTask.doneFiles}/${activeTask.totalFiles} 个文件`
                      : `${formatBytes(activeTask.doneBytes)} / ${formatBytes(activeTask.totalBytes)} · ${formatSpeed(activeTask.speedBps)}`
                }
              />
              <span className="muted small">{activeTask.currentFile ?? "准备中…"}</span>
            </div>
          ) : null}
          <div>
            <h4>光影插件</h4>
            <ul className="reco-list">{shaders.map(row)}</ul>
            <p className="muted small">
              Iris 是 Fabric/Quilt 的推荐光影方案；OptiFine 需要运行官方安装器。安装 Iris
              后，在实例的「光影管理」里导入 `.zip` 光影包。
            </p>
          </div>
        </div>
      )}
    </Card>
  );
}
