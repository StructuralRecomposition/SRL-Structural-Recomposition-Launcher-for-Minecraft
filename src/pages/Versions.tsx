import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  type InstalledVersion,
  type ManifestVersion,
  type VerificationReport,
  type VersionDeletePlan,
  type VersionDetail,
  type VersionList,
} from "../lib/api";
import { formatBytes, formatDate } from "../lib/format";
import { isActive } from "../lib/task";
import {
  manifestTypeLabel,
  versionBucket,
  versionBucketLabel,
  versionBucketTone,
} from "../lib/version";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, Field, IconButton } from "../components/ui";
import { Modal } from "../components/Modal";
import { Icon } from "../components/Icon";
import { InstallVersionModal } from "../components/InstallVersionModal";

type Tab = "download" | "installed";
type DownloadKind = "release" | "snapshot" | "old_beta" | "old_alpha";
type LocalFilter = "all" | "release" | "snapshot" | "legacy" | "loader";

const DOWNLOAD_TABS: [DownloadKind, string][] = [
  ["release", "正式版"],
  ["snapshot", "快照"],
  ["old_beta", "旧版 Beta"],
  ["old_alpha", "旧版 Alpha"],
];

const LOCAL_TABS: [LocalFilter, string][] = [
  ["all", "全部"],
  ["release", "正式版"],
  ["snapshot", "快照"],
  ["legacy", "旧版"],
  ["loader", "加载器"],
];

export function VersionsPage() {
  const { bootstrap, tasks, reload, pushToast, setPage, updateBootstrap } = useApp();
  const [data, setData] = useState<VersionList | null>(null);
  const [local, setLocal] = useState<InstalledVersion[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("download");
  const [search, setSearch] = useState("");
  const [kind, setKind] = useState<DownloadKind>("release");
  const [localFilter, setLocalFilter] = useState<LocalFilter>("all");
  const [localSearch, setLocalSearch] = useState("");
  const [limit, setLimit] = useState(120);
  const [installAssets, setInstallAssets] = useState(
    bootstrap?.settings.downloadAssetsOnInstall ?? true,
  );
  const [installTarget, setInstallTarget] = useState<ManifestVersion | null>(null);
  const [detail, setDetail] = useState<VersionDetail | null>(null);
  const [verify, setVerify] = useState<VerificationReport | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [configuring, setConfiguring] = useState<InstalledVersion | null>(null);
  const [configName, setConfigName] = useState("");
  const [configMemory, setConfigMemory] = useState(4096);
  const [configError, setConfigError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<InstalledVersion | null>(null);
  const [deletePlan, setDeletePlan] = useState<VersionDeletePlan | null>(null);
  const [deleteForce, setDeleteForce] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const completedCount = useRef(0);

  const installed = local;

  async function loadLocal() {
    try {
      const list = (await api.listInstalledVersions()) ?? [];
      setLocal(list);
      updateBootstrap((b) => ({ ...b, installedVersions: list }));
    } catch (e) {
      // Keep the bootstrap snapshot on failure; the next refresh will retry.
      if (local.length === 0 && bootstrap?.installedVersions.length) {
        setLocal(bootstrap.installedVersions);
      }
      console.warn("failed to refresh installed versions", e);
    }
  }

  async function load(refresh = false) {
    setLoading(true);
    setError(null);
    try {
      setData(await api.listVersions(refresh));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void load(false);
    void loadLocal();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Refresh installed markers when an install task finishes. Do not call the
  // global reload() here: it unmounts this page while loading, which resets
  // the completed-task counter and can cause an endless reload loop.
  useEffect(() => {
    const done = tasks.filter((t) => t.kind === "install" && t.status === "completed").length;
    if (done > completedCount.current) {
      completedCount.current = done;
      void load(false);
      void loadLocal();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tasks]);

  useEffect(() => {
    if (bootstrap?.installedVersions.length) {
      setLocal((prev) => (prev.length ? prev : bootstrap.installedVersions));
    }
  }, [bootstrap?.installedVersions]);

  const installedIds = useMemo(() => new Set(data?.installed ?? []), [data?.installed]);

  const counts = useMemo(() => {
    const result: Record<DownloadKind, number> = {
      release: 0,
      snapshot: 0,
      old_beta: 0,
      old_alpha: 0,
    };
    for (const version of data?.versions ?? []) {
      if (version.type in result) {
        result[version.type as DownloadKind] += 1;
      }
    }
    return result;
  }, [data?.versions]);

  const visible = useMemo(() => {
    if (!data) return [];
    const q = search.trim().toLowerCase();
    return data.versions
      .filter((v) => v.type === kind)
      .filter((v) => !q || v.id.toLowerCase().includes(q));
  }, [data, kind, search]);

  const localVisible = useMemo(() => {
    const q = localSearch.trim().toLowerCase();
    return installed
      .filter((v) => {
        const bucket = versionBucket(v);
        if (localFilter === "all") return true;
        if (localFilter === "legacy") return bucket === "old_beta" || bucket === "old_alpha";
        return bucket === localFilter;
      })
      .filter((v) => !q || v.id.toLowerCase().includes(q));
  }, [installed, localFilter, localSearch]);

  const installingId = (versionId: string) =>
    tasks.find(
      (t) => t.kind === "install" && isActive(t.status) && t.title.includes(versionId),
    )?.id ?? null;

  async function showDetail(versionId: string) {
    try {
      setDetail(await api.versionDetail(versionId));
    } catch (e) {
      pushToast("读取版本详情失败", "error", e instanceof Error ? e.message : String(e));
    }
  }

  async function runVerify(versionId: string) {
    setBusy(versionId);
    try {
      setVerify(await api.verifyVersion(versionId, false));
    } catch (e) {
      pushToast("校验失败", "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  function openConfig(version: InstalledVersion) {
    setConfiguring(version);
    setConfigName(`${version.id} 实例`);
    setConfigMemory(4096);
    setConfigError(null);
  }

  async function createConfigured() {
    if (!configuring) return;
    setConfigError(null);
    if (!configName.trim()) {
      setConfigError("请输入实例名称");
      return;
    }
    setBusy("create");
    try {
      const inst = await api.createInstance({
        name: configName.trim(),
        versionId: configuring.id,
        memoryMb: configMemory,
      });
      setConfiguring(null);
      pushToast("实例已创建", "success", inst.name);
      await reload();
      setPage("instances");
    } catch (e) {
      setConfigError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function openFolder(versionId?: string) {
    try {
      await api.openLauncherPath(versionId ? "versionDir" : "versionsDir", versionId);
    } catch (e) {
      pushToast("无法打开文件夹", "error", e instanceof Error ? e.message : String(e));
    }
  }

  async function openDelete(version: InstalledVersion) {
    setDeleting(version);
    setDeletePlan(null);
    setDeleteForce(false);
    setDeleteError(null);
    try {
      setDeletePlan(await api.versionDeletePlan(version.id));
    } catch (e) {
      setDeleteError(e instanceof Error ? e.message : String(e));
    }
  }

  async function confirmDelete() {
    if (!deleting) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      const result = await api.deleteVersion(deleting.id, deleteForce);
      pushToast(
        "版本已删除",
        "info",
        result.deleted.length > 1
          ? `同时删除 ${result.deleted.length - 1} 个继承版本`
          : deleting.id,
      );
      setDeleting(null);
      setDeletePlan(null);
      await loadLocal();
      await load(false);
    } catch (e) {
      setDeleteError(e instanceof Error ? e.message : String(e));
    } finally {
      setDeleteBusy(false);
    }
  }

  const latestRelease = data?.latestRelease;
  const latestSnapshot = data?.latestSnapshot;

  return (
    <div className="page">
      <div className="toolbar">
        <div>
          <h2>版本管理</h2>
          <p className="muted">
            下载列表按正式版 / 快照 / 旧版分开；已安装列表直接读取{" "}
            <span className="mono">versions/</span> 目录，可打开文件夹、创建实例或删除。
          </p>
        </div>
        <div className="toolbar__actions">
          <label className="inline-check inline-check--chip" title="正式版资源通常 300–600 MB、数千个文件">
            <input
              type="checkbox"
              checked={installAssets}
              onChange={(e) => setInstallAssets(e.target.checked)}
            />
            下载资源文件（约数百 MB）
          </label>
          <Button icon="folder" onClick={() => void openFolder()}>
            打开 versions
          </Button>
          <Button icon="refresh" onClick={() => void load(true)} loading={loading}>
            刷新列表
          </Button>
        </div>
      </div>

      <div className="version-tabs">
        <button
          className={tab === "download" ? "version-tab version-tab--active" : "version-tab"}
          onClick={() => setTab("download")}
        >
          下载版本
        </button>
        <button
          className={tab === "installed" ? "version-tab version-tab--active" : "version-tab"}
          onClick={() => setTab("installed")}
        >
          已安装 <span className="version-tab__count">{installed.length}</span>
        </button>
      </div>

      {tab === "download" ? (
        <>
          {data?.warning ? (
            <div className="notice notice--warning">
              <Icon name="alert" size={17} />
              <div>
                <strong>网络提示</strong>
                <p>{data.warning}</p>
              </div>
            </div>
          ) : null}
          {error ? (
            <div className="notice notice--danger">
              <Icon name="alert" size={17} />
              <div>
                <strong>无法读取版本列表</strong>
                <p>{error}</p>
              </div>
              <Button size="sm" onClick={() => void load(true)}>
                重试
              </Button>
            </div>
          ) : null}

          {latestRelease && latestSnapshot ? (
            <div className="latest-grid">
              <button
                className="latest-card"
                onClick={() => {
                  setKind("release");
                  setSearch(latestRelease);
                }}
              >
                <span>最新正式版</span>
                <strong>{latestRelease}</strong>
                <small>稳定版本 · 推荐</small>
              </button>
              <button
                className="latest-card latest-card--snapshot"
                onClick={() => {
                  setKind("snapshot");
                  setSearch(latestSnapshot);
                }}
              >
                <span>最新快照</span>
                <strong>{latestSnapshot}</strong>
                <small>测试版本 · 可能不稳定</small>
              </button>
            </div>
          ) : null}

          <Card className="versions-card">
            <div className="versions-toolbar">
              <div className="segmented" role="tablist" aria-label="版本类型">
                {DOWNLOAD_TABS.map(([id, label]) => (
                  <button
                    key={id}
                    role="tab"
                    aria-selected={kind === id}
                    className={
                      kind === id ? "segmented__item segmented__item--active" : "segmented__item"
                    }
                    onClick={() => {
                      setKind(id);
                      setLimit(120);
                    }}
                  >
                    {label}
                    <span className="segmented__count">{counts[id]}</span>
                  </button>
                ))}
              </div>
              <div className="search">
                <Icon name="search" size={16} />
                <input
                  placeholder="搜索版本号…"
                  value={search}
                  onChange={(e) => {
                    setSearch(e.target.value);
                    setLimit(120);
                  }}
                  aria-label="搜索版本号"
                />
              </div>
              <span className="muted small">{visible.length} 个版本</span>
            </div>

            {loading && !data ? (
              <div className="list-skeleton" aria-label="加载中">
                {Array.from({ length: 6 }).map((_, i) => (
                  <div key={i} className="skeleton-row" />
                ))}
              </div>
            ) : visible.length === 0 ? (
              <EmptyState
                icon="layers"
                title="没有匹配的版本"
                description="切换上方类型标签或调整搜索关键字。"
              />
            ) : (
              <ul className="version-list">
                {visible.slice(0, limit).map((v) => {
                  const isInstalled = installedIds.has(v.id);
                  const taskId = installingId(v.id);
                  return (
                    <li key={v.id} className="version-row">
                      <div className="version-row__main">
                        <div className="version-row__id">
                          <strong>{v.id}</strong>
                          {isInstalled ? <Badge tone="success">已安装</Badge> : null}
                          {taskId ? <Badge tone="accent">安装中</Badge> : null}
                        </div>
                        <div className="version-row__meta">
                          <Badge
                            tone={
                              v.type === "release"
                                ? "success"
                                : v.type === "snapshot"
                                  ? "info"
                                  : "warning"
                            }
                          >
                            {manifestTypeLabel(v.type)}
                          </Badge>
                          <span>{formatDate(v.releaseTime ?? v.time)}</span>
                        </div>
                      </div>
                      <div className="version-row__actions">
                        {isInstalled ? (
                          <Button size="sm" onClick={() => setTab("installed")}>
                            管理
                          </Button>
                        ) : (
                          <Button
                            size="sm"
                            variant="primary"
                            icon="download"
                            loading={busy === v.id || Boolean(taskId)}
                            onClick={() => setInstallTarget(v)}
                          >
                            {taskId ? "安装中" : "安装"}
                          </Button>
                        )}
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
            {visible.length > limit ? (
              <div className="load-more">
                <Button variant="ghost" onClick={() => setLimit((n) => n + 200)}>
                  显示更多（剩余 {visible.length - limit}）
                </Button>
              </div>
            ) : null}
          </Card>
        </>
      ) : (
        <Card className="versions-card">
          <div className="versions-toolbar">
            <div className="segmented" role="tablist" aria-label="本地版本类型">
              {LOCAL_TABS.map(([id, label]) => (
                <button
                  key={id}
                  role="tab"
                  aria-selected={localFilter === id}
                  className={
                    localFilter === id
                      ? "segmented__item segmented__item--active"
                      : "segmented__item"
                  }
                  onClick={() => setLocalFilter(id)}
                >
                  {label}
                </button>
              ))}
            </div>
            <div className="search">
              <Icon name="search" size={16} />
              <input
                placeholder="搜索已安装版本…"
                value={localSearch}
                onChange={(e) => setLocalSearch(e.target.value)}
                aria-label="搜索已安装版本"
              />
            </div>
            <span className="muted small">{localVisible.length} 个本地版本</span>
          </div>

          {localVisible.length === 0 ? (
            <EmptyState
              icon="layers"
              title={installed.length === 0 ? "还没有已安装的版本" : "没有匹配的本地版本"}
              description={
                installed.length === 0
                  ? "切换到“下载版本”安装一个正式版或快照。"
                  : "调整筛选条件或搜索关键字。"
              }
              action={
                installed.length === 0 ? (
                  <Button variant="primary" icon="download" onClick={() => setTab("download")}>
                    去下载
                  </Button>
                ) : undefined
              }
            />
          ) : (
            <ul className="version-list">
              {localVisible.map((v) => {
                const usage = (bootstrap?.instances ?? []).filter(
                  (i) => i.versionId === v.id,
                ).length;
                return (
                  <li key={v.id} className="version-row version-row--local">
                    <div className="version-row__main">
                      <div className="version-row__id">
                        <strong>{v.id}</strong>
                        <Badge tone={versionBucketTone(v)}>{versionBucketLabel(v)}</Badge>
                        {!v.hasJar ? <Badge tone="danger">缺少 JAR</Badge> : null}
                        {usage > 0 ? <Badge tone="info">{usage} 个实例</Badge> : null}
                      </div>
                      <div className="version-row__meta">
                        <span>{formatBytes(v.sizeBytes)}</span>
                        <span>修改于 {formatDate(v.modifiedAt)}</span>
                        {v.inheritsFrom ? (
                          <span className="mono">继承自 {v.inheritsFrom}</span>
                        ) : null}
                        <span className="mono ellipsis" title={v.path}>
                          {v.path}
                        </span>
                      </div>
                    </div>
                    <div className="version-row__actions">
                      <Button size="sm" variant="ghost" onClick={() => void showDetail(v.id)}>
                        详情
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        loading={busy === v.id}
                        onClick={() => void runVerify(v.id)}
                      >
                        检查
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => openConfig(v)}>
                        创建实例
                      </Button>
                      <IconButton
                        icon="folder"
                        label={`打开 ${v.id} 文件夹`}
                        onClick={() => void openFolder(v.id)}
                      />
                      <IconButton
                        icon="trash"
                        label={`删除 ${v.id}`}
                        onClick={() => void openDelete(v)}
                      />
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </Card>
      )}

      <Modal
        open={detail !== null}
        onClose={() => setDetail(null)}
        title={detail ? `版本详情 · ${detail.id}` : ""}
        subtitle="来自本地已安装的版本元数据"
      >
        {detail ? (
          <div className="detail-list">
            <div>
              <span>主类</span>
              <code>{detail.mainClass}</code>
            </div>
            <div>
              <span>类型</span>
              <strong>{detail.versionType}</strong>
            </div>
            <div>
              <span>要求 Java</span>
              <strong>{detail.javaMajor}+</strong>
            </div>
            <div>
              <span>库 / natives</span>
              <strong>
                {detail.libraryCount} / {detail.nativeCount}
              </strong>
            </div>
            <div>
              <span>参数格式</span>
              <strong>{detail.legacyArguments ? "旧版 minecraftArguments" : "现代 arguments"}</strong>
            </div>
            <div>
              <span>文件状态</span>
              <strong>{detail.installed ? "完整" : `${detail.issues.length} 项问题`}</strong>
            </div>
            {detail.javaCheck ? (
              <div>
                <span>Java 检查</span>
                <strong>{detail.javaCheck.message}</strong>
              </div>
            ) : null}
            {detail.issues.length > 0 ? (
              <div className="detail-issues">
                <span>缺失文件</span>
                <ul>
                  {detail.issues.slice(0, 10).map((i) => (
                    <li key={i} className="mono">
                      {i}
                    </li>
                  ))}
                </ul>
              </div>
            ) : null}
          </div>
        ) : null}
      </Modal>

      <Modal
        open={verify !== null}
        onClose={() => setVerify(null)}
        title="文件完整性检查"
        subtitle={verify?.ok ? "所有被检查的文件均通过" : "发现缺失或校验失败的文件"}
      >
        {verify ? (
          <>
            <div className="verify-summary">
              <Badge tone={verify.ok ? "success" : "danger"}>{verify.ok ? "通过" : "存在问题"}</Badge>
              <span>已检查 {verify.checked} 个核心文件</span>
              <span>资源文件 {verify.checkedAssets} 个</span>
            </div>
            {verify.issues.length === 0 ? (
              <p className="muted">客户端 JAR、库文件与资源索引均存在且哈希匹配。</p>
            ) : (
              <ul className="issue-list">
                {verify.issues.slice(0, 50).map((issue) => (
                  <li key={issue.path + issue.reason}>
                    <Badge tone="danger">{issue.kind}</Badge>
                    <div>
                      <strong>{issue.reason}</strong>
                      <code>{issue.path}</code>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </>
        ) : null}
      </Modal>

      <Modal
        open={configuring !== null}
        onClose={() => setConfiguring(null)}
        title="从版本创建实例"
        subtitle={configuring ? `${configuring.id} · ${formatBytes(configuring.sizeBytes)}` : ""}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfiguring(null)}>
              取消
            </Button>
            <Button variant="primary" loading={busy === "create"} onClick={() => void createConfigured()}>
              创建并前往实例
            </Button>
          </>
        }
      >
        <div className="form-grid">
          <Field label="实例名称">
            <input
              value={configName}
              maxLength={64}
              onChange={(e) => setConfigName(e.target.value)}
            />
          </Field>
          <Field label="最大内存 (MB)" hint="之后仍可在实例设置中修改">
            <input
              type="number"
              min={512}
              max={65536}
              step={256}
              value={configMemory}
              onChange={(e) => setConfigMemory(Number(e.target.value))}
            />
          </Field>
        </div>
        <p className="muted small">
          版本目录：<span className="mono">{configuring?.path}</span>
        </p>
        {configError ? <p className="form-error">{configError}</p> : null}
      </Modal>

      <Modal
        open={deleting !== null}
        onClose={() => setDeleting(null)}
        title="删除已安装版本"
        subtitle={deleting?.id}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleting(null)}>
              取消
            </Button>
            <Button
              variant="danger"
              loading={deleteBusy}
              disabled={!deletePlan || deletePlan.instances.length > 0}
              onClick={() => void confirmDelete()}
            >
              确认删除
            </Button>
          </>
        }
      >
        {!deletePlan ? (
          <p className="muted">{deleteError ?? "正在检查该版本的使用情况…"}</p>
        ) : (
          <>
            <div className="detail-list">
              <div>
                <span>版本</span>
                <strong className="mono">{deletePlan.id}</strong>
              </div>
              <div>
                <span>占用空间</span>
                <strong>{formatBytes(deletePlan.sizeBytes)}</strong>
              </div>
              <div>
                <span>继承此版本</span>
                <strong>
                  {deletePlan.dependents.length > 0 ? deletePlan.dependents.join("、") : "无"}
                </strong>
              </div>
              <div>
                <span>使用中的实例</span>
                <strong>
                  {deletePlan.instances.length > 0 ? deletePlan.instances.join("、") : "无"}
                </strong>
              </div>
            </div>
            {deletePlan.instances.length > 0 ? (
              <div className="notice notice--danger">
                <Icon name="alert" size={16} />
                <div>
                  <strong>版本正在被实例使用</strong>
                  <p>请先删除或迁移这些实例，再删除版本。启动器不会自动改动实例。</p>
                </div>
              </div>
            ) : null}
            {deletePlan.dependents.length > 0 ? (
              <label className="checkbox-row">
                <input
                  type="checkbox"
                  checked={deleteForce}
                  onChange={(e) => setDeleteForce(e.target.checked)}
                />
                <span>
                  同时删除继承此版本的加载器版本
                  <small className="mono">{deletePlan.dependents.join(", ")}</small>
                </span>
              </label>
            ) : null}
            <p className="muted small">
              删除会移除 <span className="mono">versions/{deletePlan.id}</span>{" "}
              及其文件，不会进入回收站。
            </p>
          </>
        )}
        {deleteError && deletePlan ? <p className="form-error">{deleteError}</p> : null}
      </Modal>

      {installTarget ? (
        <InstallVersionModal
          versionId={installTarget.id}
          versionType={manifestTypeLabel(installTarget.type)}
          defaultAssets={installAssets}
          onClose={() => setInstallTarget(null)}
        />
      ) : null}
    </div>
  );
}
