import { useEffect, useMemo, useRef, useState } from "react";
import { api, type Instance, type VersionDetail } from "../lib/api";
import { formatBytes, formatDate, relativeTime } from "../lib/format";
import { TASK_LABELS, isActive, taskTone } from "../lib/task";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, ProgressBar } from "../components/ui";
import { Icon } from "../components/Icon";
import { formatSpeed } from "../lib/format";

export function HomePage() {
  const {
    bootstrap,
    tasks,
    running,
    markRunning,
    launchEvents,
    clearLaunchEvents,
    setPage,
    pushToast,
    updateBootstrap,
  } = useApp();
  const instances = bootstrap?.instances ?? [];
  const accounts = bootstrap?.accounts ?? [];
  const activeAccount = accounts.find((a) => a.active) ?? null;
  const [selectedId, setSelectedId] = useState<string>(
    () => localStorage.getItem("sr.selectedInstance") ?? instances[0]?.id ?? "",
  );
  const [detail, setDetail] = useState<VersionDetail | null>(null);
  const [launching, setLaunching] = useState(false);
  const [logOpen, setLogOpen] = useState(false);
  const logRef = useRef<HTMLPreElement>(null);

  const instance: Instance | undefined = useMemo(
    () => instances.find((i) => i.id === selectedId) ?? instances[0],
    [instances, selectedId],
  );

  useEffect(() => {
    if (instance) {
      localStorage.setItem("sr.selectedInstance", instance.id);
      setSelectedId(instance.id);
    }
  }, [instance?.id]);

  useEffect(() => {
    let cancelled = false;
    setDetail(null);
    if (!instance) return;
    void api
      .versionDetail(instance.versionId)
      .then((d) => {
        if (!cancelled) setDetail(d);
      })
      .catch(() => {
        if (!cancelled) setDetail(null);
      });
    return () => {
      cancelled = true;
    };
  }, [instance?.versionId, instance?.id]);

  const isRunning = instance ? running.has(instance.id) : false;
  const events = launchEvents.filter((e) => !instance || e.instanceId === instance.id);

  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [events.length, logOpen]);

  const activeTasks = tasks.filter((t) => isActive(t.status));
  const recentTasks = tasks.slice(0, 4);
  const failed = tasks.filter((t) => t.status === "failed").slice(0, 2);

  async function launch() {
    if (!instance) return;
    if (!activeAccount) {
      pushToast("请先添加并选择一个账号", "warning", "离线档案可用于本地游戏。");
      setPage("accounts");
      return;
    }
    setLaunching(true);
    setLogOpen(true);
    try {
      const result = await api.launchInstance(instance.id);
      markRunning(instance.id, true);
      pushToast("游戏进程已启动", "success", `PID ${result.pid}`);
      if (result.warnings.length) {
        pushToast("启动前检查有提示", "warning", result.warnings.join("；"));
      }
      updateBootstrap((b) => ({
        ...b,
        instances: b.instances.map((i) =>
          i.id === instance.id ? { ...i, lastLaunchedAt: new Date().toISOString() } : i,
        ),
      }));
    } catch (e) {
      pushToast("启动失败", "error", e instanceof Error ? e.message : String(e));
    } finally {
      setLaunching(false);
    }
  }

  async function stop() {
    if (!instance) return;
    try {
      await api.stopInstance(instance.id);
      pushToast("正在终止游戏进程", "warning");
    } catch (e) {
      pushToast("无法终止进程", "error", e instanceof Error ? e.message : String(e));
    }
  }

  if (instances.length === 0) {
    return (
      <div className="page">
        <section className="hero">
          <div>
            <span className="eyebrow">结构重组 · Structural Recomposition</span>
            <h2>从第一个独立实例开始</h2>
            <p>
              安装一个官方 Minecraft 版本，然后创建实例。每个实例拥有独立的游戏目录、Java 与内存配置。
            </p>
            <div className="hero__actions">
              <Button variant="primary" icon="layers" onClick={() => setPage("versions")}>
                安装游戏版本
              </Button>
              <Button icon="box" onClick={() => setPage("instances")}>
                查看实例
              </Button>
            </div>
          </div>
          <div className="hero__mark" aria-hidden="true">
            <div className="hero__tile" />
            <div className="hero__tile" />
            <div className="hero__tile" />
            <div className="hero__tile" />
          </div>
        </section>
        <div className="grid-3">
          <Card>
            <div className="stat">
              <span>已安装版本</span>
              <strong>{bootstrap?.installedVersions.length ?? 0}</strong>
            </div>
            <p className="muted">从 Mojang 官方源下载并校验。</p>
          </Card>
          <Card>
            <div className="stat">
              <span>Java 环境</span>
              <strong>{bootstrap?.java.length ?? 0}</strong>
            </div>
            <p className="muted">自动发现本机 Java，不修改系统安装。</p>
          </Card>
          <Card>
            <div className="stat">
              <span>账号档案</span>
              <strong>{accounts.length}</strong>
            </div>
            <p className="muted">离线档案仅用于本地游戏，不冒充正版。</p>
          </Card>
        </div>
      </div>
    );
  }

  return (
    <div className="page">
      <div className="home-top">
        <div className="instance-switcher">
          <label htmlFor="instance-select">当前实例</label>
          <div className="select-wrap">
            <select
              id="instance-select"
              value={instance?.id ?? ""}
              onChange={(e) => setSelectedId(e.target.value)}
            >
              {instances.map((i) => (
                <option key={i.id} value={i.id}>
                  {i.name} · {i.versionId}
                </option>
              ))}
            </select>
            <Icon name="chevron" size={16} />
          </div>
        </div>
        <div className="home-top__meta">
          <Badge tone={isRunning ? "success" : "neutral"}>
            {isRunning ? "运行中" : "未运行"}
          </Badge>
          <span className="muted">上次启动 {relativeTime(instance?.lastLaunchedAt)}</span>
        </div>
      </div>

      <section className="launch-card">
        <div className="launch-card__main">
          <div className="launch-card__version">
            <span className="eyebrow">{detail?.versionType ?? "Minecraft"}</span>
            <h2>{instance?.name}</h2>
            <div className="launch-card__tags">
              <Badge tone="info">Minecraft {instance?.versionId}</Badge>
              {instance?.modLoader ? (
                <Badge tone="accent">
                  {instance.modLoader.kind} {instance.modLoader.version}
                </Badge>
              ) : (
                <Badge>原版</Badge>
              )}
              <Badge>{instance?.memoryMb} MB 内存</Badge>
              {detail?.javaCheck ? (
                <Badge
                  tone={
                    detail.javaCheck.compatibility === "compatible"
                      ? "success"
                      : detail.javaCheck.compatibility === "warning"
                        ? "warning"
                        : "danger"
                  }
                >
                  Java {detail.javaCheck.actualMajor} / 需要 {detail.javaCheck.requiredMajor}+
                </Badge>
              ) : null}
            </div>
          </div>
          <div className="launch-card__actions">
            {isRunning ? (
              <Button variant="danger" icon="stop" onClick={stop}>
                终止游戏
              </Button>
            ) : (
              <Button
                variant="primary"
                icon="play"
                loading={launching}
                onClick={launch}
                disabled={!activeAccount}
                title={!activeAccount ? "请先选择账号" : undefined}
              >
                启动游戏
              </Button>
            )}
            <Button icon="settings" onClick={() => setPage("instances")}>
              实例设置
            </Button>
          </div>
        </div>

        <div className="launch-card__stats">
          <div>
            <span>Java</span>
            <strong title={instance?.javaPath ?? "自动选择"}>
              {instance?.javaPath ? "手动指定" : detail?.javaCheck ? `Java ${detail.javaCheck.actualMajor}` : "自动"}
            </strong>
          </div>
          <div>
            <span>游戏目录</span>
            <strong className="mono ellipsis" title={instance?.gameDir}>
              {instance?.gameDir}
            </strong>
          </div>
          <div>
            <span>文件完整性</span>
            <strong>
              {detail ? (detail.installed ? "完整" : `${detail.issues.length} 项缺失`) : "检查中…"}
            </strong>
          </div>
          <div>
            <span>账号</span>
            <strong>{activeAccount ? activeAccount.username : "未选择"}</strong>
          </div>
        </div>

        {detail && !detail.installed ? (
          <div className="notice notice--warning">
            <Icon name="alert" size={17} />
            <div>
              <strong>版本文件不完整</strong>
              <p>{detail.issues.slice(0, 3).join("；")}</p>
            </div>
            <Button size="sm" onClick={() => setPage("versions")}>
              去修复
            </Button>
          </div>
        ) : null}
        {!activeAccount ? (
          <div className="notice">
            <Icon name="user" size={17} />
            <div>
              <strong>还没有可用账号</strong>
              <p>离线档案可以在本地启动游戏，但不会被当作已验证的正版账号。</p>
            </div>
            <Button size="sm" onClick={() => setPage("accounts")}>
              添加账号
            </Button>
          </div>
        ) : null}
      </section>

      <div className="home-grid">
        <Card className="console-card">
          <div className="card-head">
            <div>
              <h3>启动日志</h3>
              <p>来自游戏进程的真实 stdout / stderr，令牌已脱敏。</p>
            </div>
            <div className="card-head__actions">
              <Button size="sm" variant="ghost" onClick={() => setLogOpen((v) => !v)}>
                {logOpen ? "收起" : "展开"}
              </Button>
              <Button size="sm" variant="ghost" icon="trash" onClick={clearLaunchEvents}>
                清空
              </Button>
            </div>
          </div>
          {logOpen ? (
            events.length > 0 ? (
              <pre className="console" ref={logRef}>
                {events.map((e, i) =>
                  e.kind === "log" ? (
                    <div key={i} className={e.stream === "stderr" ? "console__err" : ""}>
                      {e.line}
                    </div>
                  ) : e.kind === "exit" ? (
                    <div key={i} className="console__exit">
                      [{e.code === null ? "signal" : `exit ${e.code}`}] {e.message}
                    </div>
                  ) : null,
                )}
              </pre>
            ) : (
              <p className="console console--empty">启动游戏后，日志会显示在这里。</p>
            )
          ) : (
            <button className="console-preview" onClick={() => setLogOpen(true)}>
              <Icon name="terminal" size={16} />
              <span>{events.length > 0 ? `查看最近 ${Math.min(events.length, 400)} 行输出` : "打开日志面板"}</span>
            </button>
          )}
        </Card>

        <Card>
          <div className="card-head">
            <div>
              <h3>最近活动</h3>
              <p>下载与安装任务的真实状态。</p>
            </div>
            <Button size="sm" variant="ghost" onClick={() => setPage("tasks")}>
              全部
            </Button>
          </div>
          {recentTasks.length === 0 ? (
            <EmptyState
              icon="download"
              title="还没有任务"
              description="安装版本或校验文件后，这里会显示真实的任务记录。"
            />
          ) : (
            <ul className="activity">
              {recentTasks.map((t) => (
                <li key={t.id}>
                  <div className="activity__row">
                    <strong className="ellipsis">{t.title}</strong>
                    <Badge tone={taskTone(t.status)}>{TASK_LABELS[t.status]}</Badge>
                  </div>
                  {isActive(t.status) ? (
                    <ProgressBar
                      value={t.totalBytes > 0 ? (t.doneBytes / t.totalBytes) * 100 : null}
                      label={`${formatBytes(t.doneBytes)} · ${formatSpeed(t.speedBps)}`}
                    />
                  ) : (
                    <span className="muted">
                      {t.status === "failed" ? t.error?.message : formatDate(t.finishedAt ?? t.startedAt)}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          )}
          {failed.length > 0 ? (
            <div className="notice notice--danger">
              <Icon name="alert" size={16} />
              <div>
                <strong>最近有失败任务</strong>
                <p>{failed[0].error?.message ?? "下载失败"}</p>
              </div>
            </div>
          ) : null}
          {activeTasks.length > 0 ? (
            <div className="card-foot muted">
              {activeTasks.length} 个任务进行中 · 并发上限 {bootstrap?.settings.maxConcurrentDownloads}
            </div>
          ) : null}
        </Card>
      </div>
    </div>
  );
}
