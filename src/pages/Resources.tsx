import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type GameVersionInfo,
  type ResourceFile,
  type ResourceHit,
  type ResourceKind,
  type ResourceProject,
  type ResourceSource,
  type ResourceTaskAccepted,
  type ResourceVersion,
} from "../lib/api";
import { formatBytes, formatCount, formatDate, relativeTime } from "../lib/format";
import { isActive, taskProgress, taskTone, TASK_LABELS } from "../lib/task";
import { MAX_UPLOAD_BYTES, pickFiles, readFileBytes } from "../lib/files";
import {
  RESOURCE_CF_TAGS,
  RESOURCE_KIND_LABEL,
  RESOURCE_KINDS,
  RESOURCE_LOADERS,
  RESOURCE_SORTS,
  RESOURCE_SOURCES,
  RESOURCE_TAGS,
  baseVersionOfInstance,
  isVersionCompatible,
} from "../lib/resourceTags";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, Field, ProgressBar } from "../components/ui";
import { Icon } from "../components/Icon";
import { Modal } from "../components/Modal";
import { DropZone } from "../components/DropZone";
import { LoaderResourcePanel } from "../components/LoaderResourcePanel";
import { t as tr } from "../lib/i18n";


type KindTab = ResourceKind | "map" | "loader";

interface DetailState {
  hit: ResourceHit;
  project: ResourceProject;
  versions: ResourceVersion[];
}

interface InstallState {
  hit: ResourceHit;
  project: ResourceProject;
  version: ResourceVersion;
  file: ResourceFile;
}

function kindLabel(kind: string) {
  if (kind === "loader") return tr("加载器");
  return RESOURCE_KIND_LABEL[kind as ResourceKind] ?? kind;
}

function sourceLabel(source: ResourceSource) {
  return source === "curseforge" ? "CurseForge" : "Modrinth";
}

function tagsFor(kind: ResourceKind, source: ResourceSource) {
  return source === "curseforge" ? RESOURCE_CF_TAGS[kind] : RESOURCE_TAGS[kind];
}

function typeTone(kind: string): "accent" | "info" | "success" | "warning" | "neutral" {
  switch (kind) {
    case "mod":
      return "accent";
    case "modpack":
      return "info";
    case "shader":
      return "warning";
    case "resourcepack":
      return "success";
    case "map":
      return "info";
    default:
      return "neutral";
  }
}

function versionTypeTone(type: string): "success" | "warning" | "neutral" {
  if (type === "release") return "success";
  if (type === "beta") return "warning";
  return "neutral";
}

function pickFile(kind: string, version: ResourceVersion): ResourceFile | undefined {
  if (kind === "modpack") {
    return (
      version.files.find((f) => f.filename.toLowerCase().endsWith(".mrpack")) ??
      version.files.find((f) => f.primary) ??
      version.files[0]
    );
  }
  return version.files.find((f) => f.primary) ?? version.files[0];
}

function ResourceIcon({ src, size = 48 }: { src: string | null; size?: number }) {
  const [failed, setFailed] = useState(false);
  if (!src || failed) {
    return (
      <div className="resource-icon resource-icon--fallback" style={{ width: size, height: size }}>
        <Icon name="box" size={Math.round(size / 2)} />
      </div>
    );
  }
  return (
    <img
      className="resource-icon"
      src={src}
      width={size}
      height={size}
      alt=""
      onError={() => setFailed(true)}
    />
  );
}

export function ResourcesPage() {
  const { bootstrap, pushToast, refreshTasks } = useApp();
  const [kind, setKind] = useState<KindTab>("mod");
  const [source, setSource] = useState<ResourceSource>("modrinth");
  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");
  const [gameVersion, setGameVersion] = useState("");
  const [loader, setLoader] = useState("");
  const [tagValue, setTagValue] = useState("");
  const [sort, setSort] = useState("relevance");
  const [hits, setHits] = useState<ResourceHit[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [gameVersions, setGameVersions] = useState<GameVersionInfo[]>([]);
  const [reloadKey, setReloadKey] = useState(0);
  const [detailHit, setDetailHit] = useState<ResourceHit | null>(null);
  const [detail, setDetail] = useState<DetailState | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [installState, setInstallState] = useState<InstallState | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const sequence = useRef(0);
  const detailSequence = useRef(0);
  const defaultsInitialized = useRef(false);

  useEffect(() => {
    const timer = window.setTimeout(() => setDebounced(query.trim()), 320);
    return () => window.clearTimeout(timer);
  }, [query]);

  useEffect(() => {
    if (kind === "shader" || kind === "resourcepack" || kind === "map") {
      setLoader("");
    }
    setTagValue("");
  }, [kind]);

  const effectiveSource: ResourceSource = kind === "map" ? "curseforge" : source;

  useEffect(() => {
    setTagValue("");
  }, [source]);

  useEffect(() => {
    if (effectiveSource === "curseforge" && sort === "follows") {
      setSort("downloads");
    }
  }, [effectiveSource, sort]);

  useEffect(() => {
    if (!bootstrap || defaultsInitialized.current) return;
    const first = bootstrap.instances[0];
    if (first) {
      setGameVersion(baseVersionOfInstance(first));
    } else {
      setGameVersion("");
    }
    defaultsInitialized.current = true;
  }, [bootstrap]);

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
        /* optional filter data; search still works without it */
      });
  }, []);

  useEffect(() => {
    if (kind === "loader") {
      setLoading(false);
      setError(null);
      setHits([]);
      setTotal(0);
      return;
    }
    const id = ++sequence.current;
    const tag = tagsFor(kind as ResourceKind, effectiveSource).find((t) => t.value === tagValue);
    const effectiveQuery = [debounced, tag?.query].filter(Boolean).join(" ").trim();
    setLoading(true);
    setError(null);
    const request =
      effectiveSource === "curseforge"
        ? api.searchCurseforge({
            kind: kind as ResourceKind,
            query: effectiveQuery,
            gameVersion: gameVersion || null,
            categoryId: tag?.facetId ?? null,
            loader: loader || null,
            sort,
            offset: 0,
            limit: 20,
          })
        : api.searchResources({
            query: effectiveQuery,
            kind: kind as ResourceKind,
            gameVersion: gameVersion || null,
            loader: loader || null,
            category: tag?.facet ?? null,
            index: sort,
            offset: 0,
            limit: 20,
          });
    request
      .then((page) => {
        if (id !== sequence.current) return;
        setHits(page.hits);
        setTotal(page.totalHits);
      })
      .catch((e) => {
        if (id !== sequence.current) return;
        setHits([]);
        setTotal(0);
        setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (id === sequence.current) setLoading(false);
      });
  }, [kind, effectiveSource, debounced, gameVersion, loader, tagValue, sort, reloadKey]);

  async function loadMore() {
    if (loadingMore || hits.length >= total) return;
    const id = sequence.current;
    const tag = tagsFor(kind as ResourceKind, effectiveSource).find((t) => t.value === tagValue);
    const effectiveQuery = [debounced, tag?.query].filter(Boolean).join(" ").trim();
    setLoadingMore(true);
    try {
      const page =
        effectiveSource === "curseforge"
          ? await api.searchCurseforge({
              kind: kind as ResourceKind,
              query: effectiveQuery,
              gameVersion: gameVersion || null,
              categoryId: tag?.facetId ?? null,
              loader: loader || null,
              sort,
              offset: hits.length,
              limit: 20,
            })
          : await api.searchResources({
              query: effectiveQuery,
              kind: kind as ResourceKind,
              gameVersion: gameVersion || null,
              loader: loader || null,
              category: tag?.facet ?? null,
              index: sort,
              offset: hits.length,
              limit: 20,
            });
      if (id !== sequence.current) return;
      setHits((prev) => [...prev, ...page.hits]);
      setTotal(page.totalHits);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoadingMore(false);
    }
  }

  async function openDetail(hit: ResourceHit) {
    const id = ++detailSequence.current;
    setDetailHit(hit);
    setDetail(null);
    setDetailError(null);
    setDetailLoading(true);
    try {
      const [project, versions] = await Promise.all([
        hit.source === "curseforge"
          ? api.getCurseforgeProject(Number(hit.id), hit.kind, gameVersion || null)
          : api.getResourceProject(hit.id),
        hit.source === "curseforge"
          ? api.getCurseforgeFiles(Number(hit.id), hit.kind, gameVersion || null, loader || null)
          : api.getResourceVersions(hit.id, gameVersion || null, loader || null),
      ]);
      if (id !== detailSequence.current) return;
      setDetail({ hit, project, versions: versions ?? [] });
    } catch (e) {
      if (id !== detailSequence.current) return;
      setDetailError(e instanceof Error ? e.message : String(e));
    } finally {
      if (id === detailSequence.current) setDetailLoading(false);
    }
  }

  function startInstall(version: ResourceVersion) {
    if (!detail) return;
    const file = pickFile(detail.project.kind, version);
    if (!file) {
      pushToast(tr("该版本没有可下载文件"), "warning");
      return;
    }
    if (!file.sha1) {
      pushToast(tr("该文件缺少 SHA-1 校验值，已拒绝安装"), "warning", file.filename);
      return;
    }
    setInstallState({
      hit: detail.hit,
      project: detail.project,
      version,
      file,
    });
  }

  const isLoader = kind === "loader";
  const tags = isLoader ? [] : tagsFor(kind as ResourceKind, effectiveSource);
  const sorts = isLoader
    ? RESOURCE_SORTS
    : effectiveSource === "curseforge"
      ? RESOURCE_SORTS.filter((s) => s.value !== "follows")
      : RESOURCE_SORTS;
  const releaseVersions = gameVersions.filter((v) => v.versionType === "release");
  const snapshotVersions = gameVersions.filter((v) => v.versionType !== "release");
  const activeTag = tags.find((t) => t.value === tagValue);

  return (
    <div className="page resource-page">
      <div className="toolbar">
        <div>
          <h2>{tr("资源")}</h2>
          <p className="muted">
            {tr("搜索 Modrinth / CurseForge 社区资源，或选择 Minecraft 版本下载 / 安装 Fabric、Quilt、Forge、NeoForge、Iris、OptiFine。")}</p>
        </div>
        <div className="toolbar__actions">
          <Badge tone="success">{tr("Modrinth + CurseForge + 加载器")}</Badge>
          <Button icon="plus" onClick={() => setImportOpen(true)}>
            {tr("导入本地整合包")}</Button>
        </div>
      </div>

      <Card className="resource-search-card">
        <div className="segmented resource-tabs" role="tablist" aria-label={tr("资源类型")}>
          {RESOURCE_KINDS.map((item) => (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={kind === item.id}
              className={`segmented__item ${kind === item.id ? "segmented__item--active" : ""}`}
              onClick={() => setKind(item.id)}
            >
              {item.label}
            </button>
          ))}
        </div>

        {isLoader ? (
          <LoaderResourcePanel />
        ) : (
          <>
        {kind === "map" ? (
          <p className="muted small resource-map-hint">
            {tr("地图来源：CurseForge（内置公共 API key，无需配置）。安装到实例时会解压到 saves/，不覆盖同名存档。")}</p>
        ) : (
          <div className="resource-source-row">
            <span className="muted small">{tr("来源")}</span>
            <div className="segmented" role="radiogroup" aria-label={tr("资源来源")}>
              {RESOURCE_SOURCES.map((item) => (
                <button
                  key={item.value}
                  type="button"
                  role="radio"
                  aria-checked={source === item.value}
                  className={`segmented__item ${source === item.value ? "segmented__item--active" : ""}`}
                  onClick={() => setSource(item.value)}
                >
                  {item.label}
                </button>
              ))}
            </div>
            <span className="muted small">
              {source === "curseforge"
                ? tr("CurseForge 使用内置公共 key，可直接搜索")
                : tr("Modrinth 开放 API，无需登录")}
            </span>
          </div>
        )}
            <div className="resource-search-row">
              <div className="search resource-search">
                <Icon name="search" size={16} />
                <input
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder={tr("搜索{0}…", [kindLabel(kind)])}
                  aria-label={tr("搜索资源")}
                />
              </div>
              <select
                className="resource-sort"
                value={sort}
                onChange={(e) => setSort(e.target.value)}
                aria-label={tr("排序方式")}
              >
                {sorts.map((s) => (
                  <option key={s.value} value={s.value}>
                    {s.label}
                  </option>
                ))}
              </select>
              <Button icon="refresh" onClick={() => setReloadKey((v) => v + 1)}>
                {tr("刷新")}</Button>
            </div>

            <div className="resource-filters">
              <label className="resource-filter">
                <span>{tr("Minecraft 版本")}</span>
                <select value={gameVersion} onChange={(e) => setGameVersion(e.target.value)}>
                  <option value="">{tr("全部版本")}</option>
                  {gameVersion &&
                  !gameVersions.some((v) => v.version === gameVersion) ? (
                    <option value={gameVersion}>{gameVersion}</option>
                  ) : null}
                  {releaseVersions.length > 0 ? (
                    <optgroup label={tr("正式版")}>
                      {releaseVersions.map((v) => (
                        <option key={`r-${v.version}`} value={v.version}>
                          {v.version}
                        </option>
                      ))}
                    </optgroup>
                  ) : null}
                  {snapshotVersions.length > 0 ? (
                    <optgroup label={tr("快照 / 旧版")}>
                      {snapshotVersions.map((v) => (
                        <option key={`s-${v.version}`} value={v.version}>
                          {v.version}
                        </option>
                      ))}
                    </optgroup>
                  ) : null}
                </select>
              </label>

              {kind === "mod" || kind === "modpack" ? (
                <div className="resource-filter">
                  <span>{tr("模组加载器")}</span>
                  <div className="segmented">
                    {RESOURCE_LOADERS.map((item) => (
                      <button
                        key={item.value || "all"}
                        type="button"
                        className={`segmented__item ${loader === item.value ? "segmented__item--active" : ""}`}
                        onClick={() => setLoader(item.value)}
                      >
                        {item.label}
                      </button>
                    ))}
                  </div>
                </div>
              ) : null}
            </div>

            {tags.length > 0 ? (
              <div className="resource-tag-row" aria-label={tr("资源标签")}>
                <button
                  type="button"
                  className={`resource-tag ${tagValue === "" ? "resource-tag--active" : ""}`}
                  onClick={() => setTagValue("")}
                >
                  {tr("全部")}</button>
                {tags.map((tag) => (
                  <button
                    key={tag.value}
                    type="button"
                    className={`resource-tag ${tagValue === tag.value ? "resource-tag--active" : ""}`}
                    onClick={() => setTagValue(tag.value)}
                  >
                    {tag.label}
                  </button>
                ))}
              </div>
            ) : null}
          </>
        )}
      </Card>

      {!isLoader ? (
        <>
          <div className="resource-result-head">
            <span>
              {loading
                ? tr("正在搜索…")
                : tr("共 {0} 个结果{1}", [formatCount(total), activeTag ? ` · ${activeTag.label}` : ""])}
            </span>
            <span className="muted small">
              {gameVersion || tr("全部版本")}
              {loader ? ` · ${loader}` : ""}
            </span>
          </div>

          {error ? <p className="form-error">{error}</p> : null}

          {loading ? (
            <div className="resource-grid">
              {Array.from({ length: 6 }).map((_, i) => (
                <div key={i} className="resource-card resource-card--skeleton" />
              ))}
            </div>
          ) : hits.length === 0 ? (
            <Card>
              <EmptyState
                icon="search"
                title={tr("没有找到资源")}
                description={tr("试试更换关键词、Minecraft 版本、加载器或标签。")}
              />
            </Card>
          ) : (
            <>
              <div className="resource-grid">
                {hits.map((hit) => (
                  <button
                    key={hit.id}
                    type="button"
                    className="resource-card"
                    onClick={() => void openDetail(hit)}
                  >
                    <ResourceIcon src={hit.iconUrl} size={48} />
                    <div className="resource-card__body">
                      <div className="resource-card__title">
                        <strong>{hit.title}</strong>
                        <Badge tone={typeTone(hit.kind)}>{kindLabel(hit.kind)}</Badge>
                      </div>
                      <p className="resource-card__desc">{hit.description}</p>
                      <div className="resource-card__meta">
                        <span>↓ {formatCount(hit.downloads)}</span>
                        <span>♡ {formatCount(hit.follows)}</span>
                        <span>{hit.author || sourceLabel(hit.source)}</span>
                      </div>
                      <div className="resource-card__tags">
                        {hit.categories.slice(0, 4).map((c) => (
                          <span key={c} className="resource-mini-tag">
                            {c}
                          </span>
                        ))}
                        {hit.dateModified ? (
                          <span className="muted small">{relativeTime(hit.dateModified)}</span>
                        ) : null}
                      </div>
                    </div>
                  </button>
                ))}
              </div>
              {hits.length < total ? (
                <div className="resource-more">
                  <Button loading={loadingMore} onClick={() => void loadMore()}>
                    {tr("加载更多")}</Button>
                </div>
              ) : null}
            </>
          )}
        </>
      ) : null}

      <Modal
        open={detailHit !== null}
        onClose={() => {
          setDetailHit(null);
          setDetail(null);
        }}
        title={detail?.project.title ?? detailHit?.title ?? tr("资源详情")}
        subtitle={detail ? `${kindLabel(detail.project.kind)} · ${detail.hit.author || sourceLabel(detail.hit.source)}` : undefined}
        wide
        footer={
          <Button
            variant="ghost"
            onClick={() => {
              setDetailHit(null);
              setDetail(null);
            }}
          >
            {tr("关闭")}</Button>
        }
      >
        {detailLoading ? (
          <p className="muted">{tr("正在读取资源详情…")}</p>
        ) : detailError ? (
          <p className="form-error">{detailError}</p>
        ) : detail ? (
          <>
            <div className="resource-detail-head">
              <ResourceIcon src={detail.project.iconUrl} size={72} />
              <div className="resource-detail-head__main">
                <div className="resource-detail-title">
                  <h3>{detail.project.title}</h3>
                  <Badge tone={typeTone(detail.project.kind)}>
                    {kindLabel(detail.project.kind)}
                  </Badge>
                  {detail.project.license ? (
                    <Badge tone="neutral">{detail.project.license}</Badge>
                  ) : null}
                </div>
                <p>{detail.project.description}</p>
                <div className="resource-detail-links">
                  <Button
                    size="sm"
                    icon="info"
                    onClick={() =>
                      api
                        .openTrustedUrl(detail.project.websiteUrl)
                        .catch((e) =>
                          pushToast(
                            tr("无法打开 {0} 页面", [sourceLabel(detail.project.source)]),
                            "warning",
                            e instanceof Error ? e.message : undefined,
                          ),
                        )
                    }
                  >
                    {tr("在")}{sourceLabel(detail.project.source)} {tr("查看")}</Button>
                  {detail.project.sourceUrl ? (
                    <span className="muted small ellipsis" title={detail.project.sourceUrl}>
                      {tr("源码：")}{detail.project.sourceUrl}
                    </span>
                  ) : null}
                </div>
              </div>
            </div>

            <div className="resource-stats">
              <div>
                <span>{tr("下载")}</span>
                <strong>{formatCount(detail.project.downloads)}</strong>
              </div>
              <div>
                <span>{tr("关注")}</span>
                <strong>{formatCount(detail.project.follows)}</strong>
              </div>
              <div>
                <span>{tr("更新")}</span>
                <strong>{detail.project.updated ? relativeTime(detail.project.updated) : "—"}</strong>
              </div>
              <div>
                <span>{tr("支持加载器")}</span>
                <strong>{detail.project.loaders.slice(0, 4).join(" / ") || tr("通用")}</strong>
              </div>
            </div>

            <div className="resource-detail-tags">
              {detail.project.categories.map((c) => (
                <span key={c} className="resource-mini-tag">
                  {c}
                </span>
              ))}
            </div>

            <div className="resource-section-head">
              <div>
                <h3>{tr("可用版本")}</h3>
                <p className="muted small">
                  {gameVersion ? `Minecraft ${gameVersion}` : tr("全部 Minecraft 版本")}
                  {loader ? ` · ${loader}` : ""}
                </p>
              </div>
              <span className="muted small">{detail.versions.length} {tr("个")}</span>
            </div>

            {detail.versions.length === 0 ? (
              <div className="notice notice--warning">
                <Icon name="alert" size={16} />
                <p>{tr("当前筛选条件下没有可用文件。请关闭弹窗后更换 Minecraft 版本或加载器。")}</p>
              </div>
            ) : (
              <ul className="resource-version-list">
                {detail.versions.map((version) => {
                  const file = pickFile(detail.project.kind, version);
                  return (
                    <li key={version.id} className="resource-version-row">
                      <div className="resource-version-main">
                        <div className="resource-version-title">
                          <strong>{version.versionNumber}</strong>
                          <Badge tone={versionTypeTone(version.versionType)}>
                            {version.versionType}
                          </Badge>
                          {file?.filename.toLowerCase().endsWith(".mrpack") ? (
                            <Badge tone="info">.mrpack</Badge>
                          ) : null}
                        </div>
                        <span className="muted small">
                          {version.gameVersions.slice(-4).join(", ") || tr("通用版本")} ·{" "}
                          {version.loaders.join(" / ") || tr("通用")}
                        </span>
                        <span className="muted small">
                          {formatDate(version.datePublished)} ·{" "}
                          {file ? formatBytes(file.size) : tr("无文件")}
                          {file && !file.sha1 ? tr(" · 缺少校验值") : ""}
                        </span>
                      </div>
                      <Button
                        size="sm"
                        variant="primary"
                        icon="download"
                        disabled={!file?.sha1}
                        onClick={() => startInstall(version)}
                      >
                        {tr("安装")}</Button>
                    </li>
                  );
                })}
              </ul>
            )}
          </>
        ) : null}
      </Modal>

      {installState ? (
        <InstallDialog state={installState} onClose={() => setInstallState(null)} />
      ) : null}
      {importOpen ? (
        <ImportModpackDialog
          onClose={() => setImportOpen(false)}
          onAccepted={async () => {
            await refreshTasks();
          }}
        />
      ) : null}
    </div>
  );
}

function TaskPanel({ taskId }: { taskId: string }) {
  const { tasks } = useApp();
  const task = tasks.find((t) => t.id === taskId);
  if (!task) {
    return <p className="muted small">{tr("正在创建下载任务…")}</p>;
  }
  const progress = taskProgress(task);
  return (
    <div className="resource-task">
      <div className="resource-task__head">
        <Badge tone={taskTone(task.status)}>{TASK_LABELS[task.status]}</Badge>
        <span className="ellipsis">{task.currentFile ?? task.title}</span>
      </div>
      <ProgressBar value={progress} label={TASK_LABELS[task.status]} />
      <div className="resource-task__meta">
        <span>
          {formatBytes(task.doneBytes)} / {formatBytes(task.totalBytes)}
        </span>
        <span>
          {task.doneFiles}/{task.totalFiles} {tr("文件")}</span>
        {isActive(task.status) ? <span>{tr("关闭窗口不会取消任务")}</span> : null}
      </div>
      {task.error ? <p className="form-error">{task.error.message}</p> : null}
    </div>
  );
}

function InstallDialog({ state, onClose }: { state: InstallState; onClose: () => void }) {
  const { bootstrap, tasks, refreshTasks, reload, pushToast, setPage } = useApp();
  const instances = bootstrap?.instances ?? [];
  const [mode, setMode] = useState<"instance" | "directory">("instance");
  const [instanceId, setInstanceId] = useState("");
  const [dir, setDir] = useState("");
  const [accepted, setAccepted] = useState<ResourceTaskAccepted | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const completed = useRef(false);

  const options = useMemo(
    () =>
      instances.map((instance) => ({
        instance,
        ok: isVersionCompatible(state.project.kind, state.version, instance),
      })),
    [instances, state.project.kind, state.version],
  );
  const firstOk = options.find((o) => o.ok)?.instance;

  useEffect(() => {
    api
      .defaultResourceDir()
      .then((value) => setDir(value))
      .catch(() => {
        /* the user can type a path */
      });
  }, []);

  useEffect(() => {
    if (instanceId || !firstOk) return;
    setInstanceId(firstOk.id);
  }, [instanceId, firstOk]);

  useEffect(() => {
    if (!firstOk && !instanceId) setMode("directory");
  }, [firstOk, instanceId]);

  const task = accepted ? tasks.find((t) => t.id === accepted.taskId) : undefined;
  useEffect(() => {
    if (task?.status === "completed" && !completed.current) {
      completed.current = true;
      pushToast(tr("资源安装完成"), "success", task.title);
      void reload();
    }
  }, [task?.status, task?.title, pushToast, reload]);

  async function start() {
    setError(null);
    if (mode === "instance" && !instanceId) {
      setError(tr("请选择一个游戏实例"));
      return;
    }
    if (mode === "directory" && !dir.trim()) {
      setError(tr("请输入下载目录"));
      return;
    }
    setStarting(true);
    try {
      const target =
        mode === "instance"
          ? ({ kind: "instance", instanceId } as const)
          : ({ kind: "directory", path: dir.trim() } as const);
      const result =
        state.project.source === "curseforge"
          ? await api.installCurseforge(
              Number(state.project.id),
              Number(state.version.id),
              state.project.kind,
              target,
            )
          : await api.installResource(state.project.id, state.version.id, target);
      setAccepted(result);
      await refreshTasks();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setStarting(false);
    }
  }

  const isModpack = state.project.kind === "modpack";
  const isResourcepack = state.project.kind === "resourcepack";
  const isShader = state.project.kind === "shader";
  const isMap = state.project.kind === "map";

  return (
    <Modal
      open
      onClose={onClose}
      title={tr("安装 {0}", [state.project.title])}
      subtitle={`${state.version.versionNumber} · ${state.file.filename}`}
      wide
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {tr("关闭")}</Button>
          <Button
            variant="primary"
            icon="download"
            loading={starting}
            disabled={accepted !== null}
            onClick={() => void start()}
          >
            {tr("开始")}</Button>
        </>
      }
    >
      <div className="install-summary">
        <ResourceIcon src={state.project.iconUrl} size={56} />
        <div>
          <div className="install-summary__title">
            <strong>{state.project.title}</strong>
            <Badge tone={typeTone(state.project.kind)}>{kindLabel(state.project.kind)}</Badge>
          </div>
          <span className="muted small">{state.project.description}</span>
          <span className="muted small">
            {tr("文件：")}{state.file.filename} · {formatBytes(state.file.size)} {tr("· SHA-1 已提供")}</span>
        </div>
      </div>

      <div className="segmented install-mode">
        <button
          type="button"
          className={`segmented__item ${mode === "instance" ? "segmented__item--active" : ""}`}
          onClick={() => setMode("instance")}
        >
          {tr("安装到实例")}</button>
        <button
          type="button"
          className={`segmented__item ${mode === "directory" ? "segmented__item--active" : ""}`}
          onClick={() => setMode("directory")}
        >
          {tr("下载到本地")}</button>
      </div>

      {mode === "instance" ? (
        instances.length === 0 ? (
          <div className="notice notice--warning">
            <Icon name="alert" size={16} />
            <div>
              <strong>{tr("还没有游戏实例")}</strong>
              <p>{tr("请先在“游戏实例”创建对应版本和加载器的实例，再安装资源。")}</p>
              <Button
                size="sm"
                onClick={() => {
                  onClose();
                  setPage("instances");
                }}
              >
                {tr("去创建实例")}</Button>
            </div>
          </div>
        ) : (
          <>
            <Field label={tr("目标实例")} hint={tr("只显示加载器与 Minecraft 版本匹配的实例。")}>
              <select value={instanceId} onChange={(e) => setInstanceId(e.target.value)}>
                <option value="">{tr("选择实例…")}</option>
                {options.map(({ instance, ok }) => (
                  <option key={instance.id} value={instance.id} disabled={!ok}>
                    {instance.name} · {instance.versionId}
                    {instance.modLoader ? ` · ${instance.modLoader.kind}` : tr(" · 原版")}
                    {ok ? "" : tr("（不兼容）")}
                  </option>
                ))}
              </select>
            </Field>
            {options.length > 0 && !firstOk ? (
              <div className="notice notice--warning">
                <Icon name="alert" size={16} />
                <p>
                  {tr("没有加载器与 Minecraft 版本都匹配的实例。请创建")}{" "}
                  {state.version.gameVersions.slice(-1)[0] ?? tr("对应版本")}{" "}
                  {state.version.loaders.join(" / ") || ""} {tr("实例，或改选“下载到本地”。")}</p>
              </div>
            ) : null}
            {isModpack ? (
              <div className="notice notice--warning">
                <Icon name="alert" size={16} />
                <p>
                  {tr("整合包会把文件写入所选实例目录，同名文件可能被覆盖。建议使用一个专门为该整合包新建的实例。")}</p>
              </div>
            ) : isResourcepack ? (
              <div className="notice">
                <Icon name="info" size={16} />
                <p>{tr("材质包安装后会自动写入实例的 options.txt 并启用；下次启动生效。")}</p>
              </div>
            ) : isShader ? (
              <div className="notice">
                <Icon name="info" size={16} />
                <p>{tr("光影包会放入实例的 shaderpacks/；需要实例已安装 Iris 或 OptiFine。")}</p>
              </div>
            ) : isMap ? (
              <div className="notice">
                <Icon name="info" size={16} />
                <p>{tr("地图会解压到实例 saves/ 下的新世界文件夹；同名存档会自动使用新名称，不会覆盖。")}</p>
              </div>
            ) : (
              <div className="notice">
                <Icon name="info" size={16} />
                <p>{tr("模组会放入实例的 mods/，下次启动由对应加载器加载。")}</p>
              </div>
            )}
          </>
        )
      ) : (
        <>
          <Field label={tr("下载目录")} hint={tr("只下载文件，不会安装到游戏。目录不存在时会自动创建。")}>
            <input
              className="mono"
              value={dir}
              onChange={(e) => setDir(e.target.value)}
              placeholder="/Users/you/Downloads"
            />
          </Field>
          {isModpack ? (
            <div className="notice">
              <Icon name="info" size={16} />
              <p>{tr("整合包会下载为 .mrpack 文件，可稍后用“导入本地整合包”安装到实例。")}</p>
            </div>
          ) : null}
        </>
      )}

      {error ? <p className="form-error">{error}</p> : null}
      {accepted ? <TaskPanel taskId={accepted.taskId} /> : null}
      {accepted && task?.status === "failed" ? (
        <Button
          size="sm"
          icon="refresh"
          onClick={() => {
            completed.current = false;
            setAccepted(null);
          }}
        >
          {tr("重试")}</Button>
      ) : null}
    </Modal>
  );
}

function ImportModpackDialog({
  onClose,
  onAccepted,
}: {
  onClose: () => void;
  onAccepted: () => Promise<void>;
}) {
  const { bootstrap, tasks, refreshTasks, pushToast } = useApp();
  const instances = bootstrap?.instances ?? [];
  const [instanceId, setInstanceId] = useState(instances[0]?.id ?? "");
  const [path, setPath] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const [accepted, setAccepted] = useState<ResourceTaskAccepted | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [allowIncomplete, setAllowIncomplete] = useState(false);
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const completed = useRef(false);

  useEffect(() => {
    api
      .curseforgeKeyStatus()
      .then((value) => setHasKey(Boolean(value)))
      .catch(() => setHasKey(false));
  }, []);

  const task = accepted ? tasks.find((t) => t.id === accepted.taskId) : undefined;
  useEffect(() => {
    if (task?.status === "completed" && !completed.current) {
      completed.current = true;
      pushToast(tr("整合包安装完成"), "success", task.title);
    }
  }, [task?.status, task?.title, pushToast]);

  async function start() {
    setError(null);
    if (!instanceId) {
      setError(tr("请选择目标实例"));
      return;
    }
    if (!file && !path.trim()) {
      setError(tr("请选择 .mrpack 文件或填写本地路径"));
      return;
    }
    setStarting(true);
    try {
      let result: ResourceTaskAccepted;
      if (file) {
        if (file.size > MAX_UPLOAD_BYTES) {
          throw new Error(tr("文件超过 512MB，请改用拖拽或手动路径导入"));
        }
        const bytes = await readFileBytes(file);
        result = await api.importModpackBytes(instanceId, file.name, bytes, allowIncomplete);
      } else {
        result = await api.importModpack(instanceId, path.trim(), allowIncomplete);
      }
      setAccepted(result);
      await refreshTasks();
      await onAccepted();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setStarting(false);
    }
  }

  return (
    <Modal
      open
      onClose={onClose}
      title={tr("导入本地整合包")}
      subtitle={tr("支持 Modrinth .mrpack、CurseForge manifest.zip 与普通整合 ZIP。")}
      wide
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {tr("关闭")}</Button>
          <Button
            variant="primary"
            icon="download"
            loading={starting}
            disabled={accepted !== null}
            onClick={() => void start()}
          >
            {tr("开始导入")}</Button>
        </>
      }
    >
      <Field label={tr("目标实例")} hint={tr("实例的 Minecraft 版本与加载器必须和整合包一致。")}>
        <select value={instanceId} onChange={(e) => setInstanceId(e.target.value)}>
          <option value="">{tr("选择实例…")}</option>
          {instances.map((instance) => (
            <option key={instance.id} value={instance.id}>
              {instance.name} · {instance.versionId}
              {instance.modLoader ? ` · ${instance.modLoader.kind}` : tr(" · 原版")}
            </option>
          ))}
        </select>
      </Field>

      <DropZone
        accept={[".zip", ".mrpack"]}
        onPaths={(paths) => {
          setPath(paths[0] ?? "");
          setFile(null);
        }}
        title={tr("拖入 .zip / .mrpack 整合包")}
        hint={tr("支持 Modrinth .mrpack、CurseForge manifest.zip 或普通整合 ZIP；只读取本地文件。")}
        disabled={starting}
      />

      <div className="file-toolbar__buttons install-file-actions">
        <Button
          icon="plus"
          onClick={async () => {
            const files = await pickFiles(".zip,.mrpack", false);
            if (files[0]) {
              setFile(files[0]);
              setPath("");
            }
          }}
        >
          {tr("选择 .zip / .mrpack 文件")}</Button>
        <span className="muted small">
          {file ? tr("已选择：{0}", [file.name]) : path ? tr("路径：{0}", [path]) : tr("尚未选择文件")}
        </span>
      </div>

      <Field label={tr("或从本地路径导入")} hint={tr("适合大文件或文件选择器不可用的环境。")}>
        <input
          className="mono"
          value={path}
          onChange={(e) => {
            setPath(e.target.value);
            setFile(null);
          }}
          placeholder="/Users/you/Downloads/pack.mrpack"
        />
      </Field>

      {hasKey === false ? (
        <div className="notice notice--warning">
          <Icon name="alert" size={16} />
          <div>
            <strong>{tr("尚未配置 CurseForge API key")}</strong>
            <p>
              {tr("普通 ZIP 和 Modrinth `.mrpack` 可直接导入；CurseForge `manifest.zip` 中由 manifest 引用的模组需要 API key 才能下载。你可以在设置中配置，或勾选下方选项只导入 ZIP 内已有文件。")}</p>
            <label className="inline-check">
              <input
                type="checkbox"
                checked={allowIncomplete}
                onChange={(e) => setAllowIncomplete(e.target.checked)}
              />
              {tr("仅导入 ZIP 内已有文件（不下载缺失的 CurseForge 模组）")}</label>
          </div>
        </div>
      ) : hasKey === true ? (
        <div className="notice notice--success">
          <Icon name="check" size={16} />
          <p>{tr("CurseForge API key 已配置，manifest 引用的模组会下载并校验 SHA-1。")}</p>
        </div>
      ) : null}

      <div className="notice notice--warning">
        <Icon name="alert" size={16} />
        <p>
          {tr("安装会写入目标实例目录，同名文件可能被覆盖；`options.txt` 会先备份。CurseForge key 只保存在系统凭据库，不写入配置文件或日志。")}</p>
      </div>

      {error ? <p className="form-error">{error}</p> : null}
      {accepted ? <TaskPanel taskId={accepted.taskId} /> : null}
      {instances.length === 0 ? (
        <p className="form-error">{tr("还没有游戏实例，请先创建一个实例。")}</p>
      ) : null}
    </Modal>
  );
}
