import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type DependencyAnalysis,
  type DependencyNode,
  type DependencyRelation,
  type Instance,
  type ResourceProject,
  type ResourceTaskAccepted,
  type ResourceVersion,
} from "../lib/api";
import { useApp } from "../state/AppContext";
import { Badge, Button, Field, ProgressBar } from "./ui";
import { Modal } from "./Modal";
import { Icon } from "./Icon";
import { t } from "../lib/i18n";
import { formatBytes } from "../lib/format";
import { isActive, taskProgress } from "../lib/task";
import {
  RELATION_COLORS,
  edgePath,
  layoutDependencyGraph,
} from "../lib/dependencyGraph";

interface Props {
  project: ResourceProject;
  version: ResourceVersion;
  instances: Instance[];
  defaultGameVersion: string;
  defaultLoader: string;
  onClose: () => void;
}

function relationLabel(relation: DependencyRelation) {
  switch (relation) {
    case "required":
      return t("必需");
    case "optional":
      return t("可选");
    case "incompatible":
      return t("不兼容");
    case "embedded":
      return t("内嵌");
    case "tool":
      return t("工具");
    case "include":
      return t("包含");
    default:
      return t("未知");
  }
}

function relationBadge(relation: DependencyRelation) {
  switch (relation) {
    case "required":
      return <Badge tone="accent">{t("必需")}</Badge>;
    case "optional":
      return <Badge tone="info">{t("可选")}</Badge>;
    case "incompatible":
      return <Badge tone="danger">{t("不兼容")}</Badge>;
    case "embedded":
      return <Badge tone="neutral">{t("内嵌")}</Badge>;
    case "tool":
      return <Badge tone="warning">{t("工具")}</Badge>;
    case "include":
      return <Badge tone="success">{t("包含")}</Badge>;
    default:
      return <Badge tone="neutral">{t("未知")}</Badge>;
  }
}

function loaderOf(instance: Instance): string {
  if (instance.modLoader?.kind) return instance.modLoader.kind.toLowerCase();
  const id = instance.versionId.toLowerCase();
  if (id.includes("fabric-loader-")) return "fabric";
  if (id.includes("quilt-loader-")) return "quilt";
  if (id.includes("neoforge-")) return "neoforge";
  if (id.includes("-forge-")) return "forge";
  return "";
}

function baseVersion(instance: Instance): string {
  const id = instance.versionId;
  if (id.includes("-neoforge-")) return id.split("-neoforge-")[0];
  if (id.includes("-forge-")) return id.split("-forge-")[0];
  if (id.startsWith("fabric-loader-") || id.startsWith("quilt-loader-")) {
    return id.split("-").slice(-1)[0] ?? id;
  }
  return id;
}

const LOADERS = ["fabric", "forge", "neoforge", "quilt"];

export function ModDependencyDialog({
  project,
  version,
  instances,
  defaultGameVersion,
  defaultLoader,
  onClose,
}: Props) {
  const { tasks, pushToast, reload } = useApp();
  const [instanceId, setInstanceId] = useState("");
  const [gameVersion, setGameVersion] = useState(
    defaultGameVersion || version.gameVersions[version.gameVersions.length - 1] || "",
  );
  const [loader, setLoader] = useState(
    defaultLoader || (instances[0] ? loaderOf(instances[0]) : ""),
  );
  const [view, setView] = useState<"graph" | "list">("graph");
  const [analysis, setAnalysis] = useState<DependencyAnalysis | null>(null);
  const [analyzing, setAnalyzing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [activeNode, setActiveNode] = useState<DependencyNode | null>(null);
  const [accepted, setAccepted] = useState<ResourceTaskAccepted | null>(null);
  const [installing, setInstalling] = useState(false);
  const completed = useRef(false);

  const target = instances.find((item) => item.id === instanceId);
  const effectiveGame = target ? baseVersion(target) : gameVersion;
  const effectiveLoader = target ? loaderOf(target) : loader;
  const instanceReady = target ? Boolean(effectiveLoader) : true;

  const compatibleInstances = useMemo(
    () => instances.filter((item) => Boolean(loaderOf(item))),
    [instances],
  );
  const availableVersions = useMemo(() => {
    const list = new Set(version.gameVersions);
    if (defaultGameVersion) list.add(defaultGameVersion);
    return [...list];
  }, [version.gameVersions, defaultGameVersion]);

  const task = accepted ? tasks.find((item) => item.id === accepted.taskId) : undefined;
  useEffect(() => {
    if (task?.status === "completed" && !completed.current) {
      completed.current = true;
      pushToast(t("依赖安装完成"), "success", task.title);
      void reload();
    }
    if (task?.status === "failed" && !completed.current) {
      completed.current = true;
      setError(task.error?.message ?? t("依赖安装失败"));
      setAccepted(null);
    }
  }, [task?.status, task?.title, task?.error?.message, pushToast, reload]);

  const layout = useMemo(
    () => (analysis ? layoutDependencyGraph(analysis.nodes, analysis.edges) : null),
    [analysis],
  );
  const selectedNew = useMemo(() => {
    if (!analysis) return [];
    return analysis.nodes.filter(
      (node) =>
        selected.has(node.id) &&
        (node.depth === 0 || !node.installed) &&
        node.installable,
    );
  }, [analysis, selected]);
  const selectedBytes = selectedNew.reduce((sum, node) => sum + node.size, 0);

  async function analyze() {
    if (!effectiveGame.trim() || !effectiveLoader.trim()) {
      setError(t("请选择目标 Minecraft 版本和加载器"));
      return;
    }
    setAnalyzing(true);
    setError(null);
    setAnalysis(null);
    setAccepted(null);
    setActiveNode(null);
    completed.current = false;
    try {
      const result = await api.analyzeModDependencies({
        source: project.source,
        projectId: project.id,
        versionId: version.id,
        fileId: project.source === "curseforge" ? version.id : null,
        instanceId: target?.id ?? null,
        gameVersion: effectiveGame,
        loader: effectiveLoader,
      });
      setAnalysis(result);
      setSelected(
        new Set(
          result.nodes
            .filter((node) => node.relation === "required" || node.depth === 0)
            .map((node) => node.id),
        ),
      );
      setActiveNode(result.nodes.find((node) => node.depth === 0) ?? null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setAnalyzing(false);
    }
  }

  async function install() {
    if (!analysis || !target || selectedNew.length === 0) return;
    setInstalling(true);
    setError(null);
    try {
      const result = await api.installModDependencies(target.id, {
        analysisId: analysis.id,
        nodeIds: [...selected],
      });
      setAccepted(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setInstalling(false);
    }
  }

  function toggle(node: DependencyNode) {
    if (node.installed || !node.installable || node.relation === "incompatible") return;
    if (node.relation === "required" || node.depth === 0) return;
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(node.id)) next.delete(node.id);
      else next.add(node.id);
      return next;
    });
  }

  return (
    <Modal
      open
      onClose={onClose}
      title={t("依赖关系与兼容性")}
      subtitle={`${project.title} · ${version.versionNumber}`}
      wide
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("关闭")}
          </Button>
          <Button
            variant="primary"
            icon="download"
            disabled={
              !analysis ||
              !target ||
              Boolean(analysis.blockers.length) ||
              selectedNew.length === 0 ||
              installing ||
              accepted !== null
            }
            loading={installing}
            onClick={() => void install()}
          >
            {t("安装所选 {0} 个文件", [selectedNew.length])}
          </Button>
        </>
      }
    >
      <div className="dep-target">
        <Field
          label={t("目标实例")}
          hint={t("选择实例后会使用该实例真实的 Minecraft 版本与加载器。")}
        >
          <select
            value={instanceId}
            onChange={(event) => {
              setInstanceId(event.target.value);
              setAnalysis(null);
              setAccepted(null);
              setError(null);
            }}
            disabled={installing}
          >
            <option value="">{t("不绑定实例（使用当前筛选条件）")}</option>
            {compatibleInstances.map((item) => (
              <option key={item.id} value={item.id}>
                {item.name} · {baseVersion(item)} · {loaderOf(item)}
              </option>
            ))}
          </select>
        </Field>
        {!target ? (
          <>
            <Field label={t("Minecraft 版本")}>
              <select
                value={gameVersion}
                onChange={(event) => {
                  setGameVersion(event.target.value);
                  setAnalysis(null);
                  setAccepted(null);
                  setError(null);
                }}
                disabled={installing}
              >
                <option value="">{t("请选择")}</option>
                {availableVersions.map((value) => (
                  <option key={value} value={value}>
                    {value}
                  </option>
                ))}
              </select>
            </Field>
            <Field label={t("加载器")}>
              <select
                value={loader}
                onChange={(event) => {
                  setLoader(event.target.value);
                  setAnalysis(null);
                  setAccepted(null);
                  setError(null);
                }}
                disabled={installing}
              >
                <option value="">{t("请选择")}</option>
                {LOADERS.map((value) => (
                  <option key={value} value={value}>
                    {value}
                  </option>
                ))}
              </select>
            </Field>
          </>
        ) : (
          <div className="dep-target__basis">
            <span>{t("分析依据")}</span>
            <strong>
              Minecraft {effectiveGame} · {effectiveLoader}
            </strong>
          </div>
        )}
        <div className="dep-target__action">
          <Button
            variant="secondary"
            icon="search"
            loading={analyzing}
            disabled={!instanceReady || installing}
            onClick={() => void analyze()}
          >
            {analysis ? t("重新分析") : t("开始分析")}
          </Button>
        </div>
      </div>

      {!target ? (
        <p className="muted small">
          {t("未绑定实例时，分析只依据当前筛选条件；安装前仍需选择具体实例。")}
        </p>
      ) : null}
      {error ? <p className="form-error">{error}</p> : null}

      {analysis ? (
        <div className="dep-analysis">
          <div className="dep-stats">
            <div>
              <span>{t("节点")}</span>
              <strong>{analysis.stats.nodes}</strong>
            </div>
            <div>
              <span>{t("必需")}</span>
              <strong>{analysis.stats.required}</strong>
            </div>
            <div>
              <span>{t("可选")}</span>
              <strong>{analysis.stats.optional}</strong>
            </div>
            <div>
              <span>{t("已安装")}</span>
              <strong>{analysis.stats.installed}</strong>
            </div>
            <div>
              <span>{t("待安装文件")}</span>
              <strong>{selectedNew.length}</strong>
            </div>
            <div>
              <span>{t("预计下载")}</span>
              <strong>{formatBytes(selectedBytes)}</strong>
            </div>
          </div>

          {analysis.blockers.length > 0 ? (
            <div className="notice notice--danger">
              <Icon name="alert" size={16} />
              <div>
                <strong>{t("存在阻止安装的问题")}</strong>
                <ul>
                  {analysis.blockers.map((value) => (
                    <li key={value}>{value}</li>
                  ))}
                </ul>
              </div>
            </div>
          ) : null}
          {!analysis.complete ? (
            <div className="notice notice--warning">
              <Icon name="alert" size={16} />
              <span>{t("依赖图不完整：部分关系或本地模组无法读取，结果已如实标记。")}</span>
            </div>
          ) : null}
          {analysis.warnings.length > 0 ? (
            <details className="dep-warnings">
              <summary>{t("{0} 条提示", [analysis.warnings.length])}</summary>
              <ul>
                {analysis.warnings.map((value) => (
                  <li key={value}>{value}</li>
                ))}
              </ul>
            </details>
          ) : null}

          <div className="dep-viewbar">
            <div className="segmented">
              <button
                type="button"
                className={`segmented__item ${view === "graph" ? "segmented__item--active" : ""}`}
                onClick={() => setView("graph")}
              >
                {t("图形视图")}
              </button>
              <button
                type="button"
                className={`segmented__item ${view === "list" ? "segmented__item--active" : ""}`}
                onClick={() => setView("list")}
              >
                {t("列表视图")}
              </button>
            </div>
            <span className="muted small">
              {analysis.gameVersion} · {analysis.loader} ·{" "}
              {analysis.instanceName ?? t("未绑定实例")}
            </span>
          </div>

          {view === "graph" && layout ? (
            <div className="dep-graph-wrap">
              <svg
                className="dep-graph"
                width={layout.width}
                height={layout.height}
                role="img"
                aria-label={t("依赖关系图")}
              >
                {layout.edges.map((item) => {
                  const color = RELATION_COLORS[item.edge.relation] ?? RELATION_COLORS.unknown;
                  const x1 = item.from.x + item.from.width;
                  const y1 = item.from.y + item.from.height / 2;
                  const x2 = item.to.x;
                  const y2 = item.to.y + item.to.height / 2;
                  return (
                    <g key={`${item.edge.from}-${item.edge.to}-${item.edge.relation}`}>
                      <path d={edgePath(item.from, item.to)} fill="none" stroke={color} strokeWidth={1.5} />
                      {layout.edges.length <= 24 ? (
                        <text
                          x={(x1 + x2) / 2}
                          y={(y1 + y2) / 2 - 4}
                          className="dep-edge-label"
                          fill={color}
                          textAnchor="middle"
                        >
                          {relationLabel(item.edge.relation)}
                        </text>
                      ) : null}
                    </g>
                  );
                })}
                {layout.nodes.map((item) => {
                  const node = item.node;
                  const color = RELATION_COLORS[node.relation] ?? RELATION_COLORS.unknown;
                  const status = node.installed
                    ? t("已安装")
                    : node.relation === "incompatible"
                      ? t("不兼容")
                      : node.selectedByDefault || selected.has(node.id)
                        ? t("将安装")
                        : t("未选择");
                  return (
                    <g
                      key={node.id}
                      className="dep-node"
                      transform={`translate(${item.x}, ${item.y})`}
                      onClick={() => setActiveNode(node)}
                    >
                      <rect
                        width={item.width}
                        height={item.height}
                        rx={10}
                        className={activeNode?.id === node.id ? "dep-node__box dep-node__box--active" : "dep-node__box"}
                        stroke={color}
                      />
                      <rect x={0} y={0} width={4} height={item.height} rx={2} fill={color} />
                      <text x={14} y={20} className="dep-node__title">
                        {node.title.length > 24 ? `${node.title.slice(0, 23)}…` : node.title}
                      </text>
                      <text x={14} y={38} className="dep-node__meta">
                        {node.versionNumber ?? "—"}
                      </text>
                      <text x={14} y={53} className="dep-node__meta">
                        {status}
                        {node.requiredBy.length > 0 ? ` · ← ${node.requiredBy[0]}` : ""}
                      </text>
                    </g>
                  );
                })}
              </svg>
            </div>
          ) : null}

          {view === "list" ? (
            <div className="dep-list">
              {analysis.nodes.map((node) => {
                const required = node.relation === "required" || node.depth === 0;
                const checked = required || selected.has(node.id);
                return (
                  <div
                    key={node.id}
                    className={`dep-row ${activeNode?.id === node.id ? "dep-row--active" : ""}`}
                    onClick={() => setActiveNode(node)}
                  >
                    <input
                      type="checkbox"
                      checked={checked}
                      disabled={
                        required ||
                        node.installed ||
                        !node.installable ||
                        !node.compatible ||
                        (node.relation !== "optional" && node.depth > 0)
                      }
                      onChange={() => toggle(node)}
                      onClick={(event) => event.stopPropagation()}
                      aria-label={t("选择 {0}", [node.title])}
                    />
                    <div className="dep-row__main">
                      <div className="dep-row__title">
                        <strong>{node.title}</strong>
                        {relationBadge(node.relation)}
                        {node.installed ? <Badge tone="success">{t("已安装")}</Badge> : null}
                        {!node.compatible ? <Badge tone="danger">{t("版本不匹配")}</Badge> : null}
                      </div>
                      <span className="muted small">
                        {node.versionNumber ?? "—"}
                        {node.fileName ? ` · ${node.fileName}` : ""}
                        {node.size > 0 ? ` · ${formatBytes(node.size)}` : ""}
                      </span>
                      {node.requiredBy.length > 0 ? (
                        <span className="muted small">
                          {t("由 {0} 需要", [node.requiredBy.join("、")])}
                        </span>
                      ) : null}
                    </div>
                    <div className="dep-row__flags">
                      <span className="muted small">L{node.depth}</span>
                    </div>
                  </div>
                );
              })}
            </div>
          ) : null}

          {activeNode ? <NodeDetail node={activeNode} /> : null}

          {task ? (
            <div className="resource-task">
              <div className="resource-task__head">
                <Badge tone={task.status === "failed" ? "danger" : "info"}>{task.status}</Badge>
                <span className="ellipsis">{task.currentFile ?? task.title}</span>
              </div>
              <ProgressBar value={taskProgress(task)} label={t("安装依赖")} />
              <div className="resource-task__meta">
                <span>
                  {formatBytes(task.doneBytes)} / {formatBytes(task.totalBytes)}
                </span>
                <span>
                  {task.doneFiles}/{task.totalFiles} {t("文件")}
                </span>
                {isActive(task.status) ? <span>{t("关闭窗口不会取消任务")}</span> : null}
              </div>
            </div>
          ) : null}
        </div>
      ) : null}
    </Modal>
  );
}

function NodeDetail({ node }: { node: DependencyNode }) {
  return (
    <div className="dep-detail">
      <div className="dep-detail__head">
        <strong>{node.title}</strong>
        {relationBadge(node.relation)}
      </div>
      <dl className="dep-detail__grid">
        <div>
          <dt>{t("版本")}</dt>
          <dd>{node.versionNumber ?? "—"}</dd>
        </div>
        <div>
          <dt>{t("来源")}</dt>
          <dd>{node.source}</dd>
        </div>
        <div>
          <dt>{t("项目 ID")}</dt>
          <dd className="mono">{node.projectId}</dd>
        </div>
        <div>
          <dt>{t("文件")}</dt>
          <dd className="mono">{node.fileName ?? t("无文件")}</dd>
        </div>
        <div>
          <dt>{t("大小")}</dt>
          <dd>{node.size > 0 ? formatBytes(node.size) : "—"}</dd>
        </div>
        <div>
          <dt>{t("校验")}</dt>
          <dd>{node.hasChecksum ? t("已提供") : t("缺少校验值")}</dd>
        </div>
        <div>
          <dt>{t("安装状态")}</dt>
          <dd>
            {node.installed
              ? `${t("已安装")}${node.installedVersion ? ` · ${node.installedVersion}` : ""}`
              : t("未安装")}
          </dd>
        </div>
        <div>
          <dt>{t("兼容性")}</dt>
          <dd>{node.compatible ? t("匹配目标版本") : t("不匹配目标版本")}</dd>
        </div>
      </dl>
      {node.notes.length > 0 ? (
        <ul className="dep-detail__notes">
          {node.notes.map((value) => (
            <li key={value}>{value}</li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
