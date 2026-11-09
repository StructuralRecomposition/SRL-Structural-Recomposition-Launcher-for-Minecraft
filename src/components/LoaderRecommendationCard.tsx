import { useEffect, useMemo, useState } from "react";
import { api, type LoaderRecommendation, type LoaderRecommendations } from "../lib/api";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, ProgressBar } from "./ui";
import { formatBytes, formatSpeed } from "../lib/format";
import { isActive, taskProgress } from "../lib/task";
import { Icon } from "./Icon";
import { t as tr } from "../lib/i18n";


const LOADER_IDS = ["fabric", "quilt", "forge", "neoforge"];
const SHADER_IDS = ["iris", "optifine"];

function tone(status: LoaderRecommendation["status"]) {
  if (status === "recommended") return "success" as const;
  if (status === "available") return "info" as const;
  if (status === "unavailable") return "warning" as const;
  return "neutral" as const;
}

function statusLabel(status: LoaderRecommendation["status"]) {
  if (status === "recommended") return tr("推荐");
  if (status === "available") return tr("可用");
  if (status === "unavailable") return tr("不可用");
  return tr("不支持");
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
      pushToast(tr("{0} 安装任务已开始", [item.name]), "info", item.version);
    } catch (e) {
      pushToast(tr("无法开始 {0} 安装", [item.name]), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function download(item: LoaderRecommendation) {
    if (!item.downloadUrl) return;
    try {
      await api.openTrustedUrl(item.downloadUrl);
    } catch (e) {
      pushToast(tr("无法打开下载页"), "error", e instanceof Error ? e.message : String(e));
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
              <span>{tr("版本")}</span>
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
              {tr("安装")}</Button>
          ) : null}
          {item.downloadUrl && item.id === "optifine" ? (
            <Button size="sm" icon="download" onClick={() => void download(item)}>
              {tr("下载安装器")}</Button>
          ) : null}
          {item.id === "iris" ? (
            <Button size="sm" variant="ghost" onClick={() => setPage("instances")}>
              {tr("去实例光影")}</Button>
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
          <h3>{tr("按版本推荐")}</h3>
          <p>{tr("根据已安装的游戏版本检查 Fabric / Quilt / Forge / NeoForge 与光影方案。")}</p>
        </div>
        <div className="reco-toolbar">
          <select
            value={version}
            onChange={(e) => setVersion(e.target.value)}
            aria-label={tr("推荐使用的游戏版本")}
          >
            {baseVersions.map((v) => (
              <option key={v.id} value={v.id}>
                {v.id}
              </option>
            ))}
          </select>
          <Button size="sm" icon="refresh" loading={loading} onClick={() => void load()}>
            {tr("重新检测")}</Button>
        </div>
      </div>

      {baseVersions.length === 0 ? (
        <EmptyState
          icon="layers"
          title={tr("需要先安装原版版本")}
          description={tr("安装一个正式版或快照后，才能按版本推荐加载器与光影。")}
          action={
            <Button variant="primary" icon="download" onClick={() => setPage("versions")}>
              {tr("前往版本管理")}</Button>
          }
        />
      ) : error ? (
        <div className="notice notice--warning">
          <Icon name="alert" size={16} />
          <div>
            <strong>{tr("无法获取推荐")}</strong>
            <p>{error}</p>
          </div>
          <Button size="sm" onClick={() => void load()}>
            {tr("重试")}</Button>
        </div>
      ) : loading && !data ? (
        <p className="muted">{tr("正在检测兼容版本…（Quilt 官方源较慢时会等待最多 20 秒）")}</p>
      ) : (
        <div className="reco-groups">
          <div>
            <h4>{tr("模组加载器")}</h4>
            <ul className="reco-list">{loaders.map(row)}</ul>
          </div>
          {activeTask ? (
            <div className="fabric-task">
              <div className="fabric-task__head">
                <strong>{activeTask.title}</strong>
                <Badge tone="accent">{tr("安装中")}</Badge>
              </div>
              <ProgressBar
                value={taskProgress(activeTask)}
                label={
                  activeTask.totalBytes <= 0
                    ? tr("已下载 {0}", [formatBytes(activeTask.doneBytes)])
                    : taskProgress(activeTask) === null
                      ? tr("下载中 · {0}/{1} 个文件", [activeTask.doneFiles, activeTask.totalFiles])
                      : `${formatBytes(activeTask.doneBytes)} / ${formatBytes(activeTask.totalBytes)} · ${formatSpeed(activeTask.speedBps)}`
                }
              />
              <span className="muted small">{activeTask.currentFile ?? tr("准备中…")}</span>
            </div>
          ) : null}
          <div>
            <h4>{tr("光影插件")}</h4>
            <ul className="reco-list">{shaders.map(row)}</ul>
            <p className="muted small">
              {tr("Iris 是 Fabric/Quilt 的推荐光影方案；OptiFine 需要运行官方安装器。安装 Iris 后，在实例的「光影管理」里导入 `.zip` 光影包。")}</p>
          </div>
        </div>
      )}
    </Card>
  );
}
