import { useEffect, useState } from "react";
import { api, type JavaCheck, type JavaRuntimeOptions } from "../lib/api";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, Field, ProgressBar } from "../components/ui";
import { Icon } from "../components/Icon";
import { formatBytes } from "../lib/format";
import { isActive, taskProgress, taskTone, TASK_LABELS } from "../lib/task";
import { t as tr } from "../lib/i18n";


export function JavaPage() {
  const { bootstrap, updateBootstrap, pushToast, reload, tasks } = useApp();
  const javas = bootstrap?.java ?? [];
  const installed = bootstrap?.installedVersions ?? [];
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [checks, setChecks] = useState<Record<string, JavaCheck>>({});
  const [error, setError] = useState<string | null>(null);
  const [runtimeOptions, setRuntimeOptions] = useState<JavaRuntimeOptions | null>(null);
  const [targetMajor, setTargetMajor] = useState(21);
  const [installing, setInstalling] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .javaRuntimeOptions()
      .then((options) => {
        if (cancelled) return;
        setRuntimeOptions(options);
        setTargetMajor(options.recommended || 21);
      })
      .catch(() => {
        /* the install still works with the fallback list */
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function installRuntime() {
    setInstallError(null);
    setInstalling(true);
    try {
      const info = await api.installJavaRuntime(targetMajor);
      updateBootstrap((b) => ({
        ...b,
        java: [...b.java.filter((j) => j.path !== info.path), info].sort(
          (a, b) => b.major - a.major,
        ),
        // Do not replace an existing default just because a new runtime was
        // installed; a newer Java can break older Forge/mod versions.
        settings: b.settings.javaPath
          ? b.settings
          : { ...b.settings, javaPath: info.path },
      }));
      pushToast(
        bootstrap?.settings.javaPath
          ? tr("Java {0} 已安装", [info.major])
          : tr("Java {0} 已安装并设为默认", [info.major]),
        "success",
        `${info.versionString} · ${info.path}`,
      );
      await reload();
    } catch (e) {
      setInstallError(e instanceof Error ? e.message : String(e));
    } finally {
      setInstalling(false);
    }
  }

  async function clearDefault() {
    if (!bootstrap) return;
    setBusy(true);
    try {
      const settings = await api.updateSettings({
        ...bootstrap.settings,
        javaPath: null,
      });
      updateBootstrap((b) => ({ ...b, settings }));
      pushToast(tr("已改为自动选择"), "success", tr("每个版本会使用最接近目标版本的 Java"));
    } catch (e) {
      pushToast(tr("设置失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function refresh() {
    setBusy(true);
    try {
      const list = await api.listJava(true);
      updateBootstrap((b) => ({ ...b, java: list }));
      pushToast(tr("Java 扫描完成"), "success", tr("发现 {0} 个可用环境", [list.length]));
    } catch (e) {
      pushToast(tr("扫描 Java 失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function add() {
    setError(null);
    if (!path.trim()) {
      setError(tr("请输入 java 可执行文件的完整路径"));
      return;
    }
    setBusy(true);
    try {
      const info = await api.addJavaPath(path.trim());
      updateBootstrap((b) => ({
        ...b,
        java: [...b.java.filter((j) => j.path !== info.path), info].sort((a, b) => b.major - a.major),
      }));
      pushToast(tr("Java 已添加并设为默认"), "success", `Java ${info.major} · ${info.path}`);
      setPath("");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function setDefault(javaPath: string) {
    setBusy(true);
    try {
      await api.addJavaPath(javaPath);
      await reload();
      pushToast(tr("默认 Java 已更新"), "success", javaPath);
    } catch (e) {
      pushToast(tr("设置失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    if (installed.length === 0) return;
    let cancelled = false;
    void Promise.all(
      installed.slice(0, 8).map(async (v) => {
        try {
          const check = await api.checkJavaForVersion(v.id);
          return [v.id, check] as const;
        } catch {
          return null;
        }
      }),
    ).then((rows) => {
      if (cancelled) return;
      const next: Record<string, JavaCheck> = {};
      for (const row of rows) if (row) next[row[0]] = row[1];
      setChecks(next);
    });
    return () => {
      cancelled = true;
    };
  }, [installed.map((v) => v.id).join(","), javas.length]);

  const defaultJava = bootstrap?.settings.javaPath ?? null;
  const javaTask = [...tasks]
    .filter((t) => t.kind === "java")
    .sort((a, b) => b.startedAt.localeCompare(a.startedAt))[0];
  const majorOptions = runtimeOptions?.majors ?? [8, 11, 17, 21, 25];
  const lts = runtimeOptions?.lts ?? [8, 11, 17, 21, 25];

  return (
    <div className="page">
      <div className="toolbar">
        <div>
          <h2>{tr("Java 环境")}</h2>
          <p className="muted">
            {tr("自动扫描本机 Java；也可一键下载安装 Temurin，只写入启动器数据目录，不修改系统 Java。")}</p>
        </div>
        <div className="toolbar__actions">
          {defaultJava ? (
            <Button icon="settings" onClick={clearDefault} disabled={busy}>
              {tr("自动选择")}</Button>
          ) : null}
          <Button icon="refresh" onClick={refresh} loading={busy}>
            {tr("重新扫描")}</Button>
        </div>
      </div>

      <Card>
        <div className="card-head">
          <div>
            <h3>{tr("下载并安装 Java")}</h3>
            <p>
              {tr("来源 Eclipse Temurin (Adoptium)。下载后校验 SHA-256，解压安装到启动器数据目录，不影响系统 Java。设置中开启“国内镜像”时优先走清华 TUNA，失败自动回退官方源。")}</p>
          </div>
          <Badge tone="info">{tr("一键安装")}</Badge>
        </div>
        {installError ? <p className="form-error">{installError}</p> : null}
        <div className="inline-form">
          <Field
            label={tr("Java 版本")}
            hint={tr("LTS：8 / 11 / 17 / 21 / 25。新版本通常需要 21 或 25，老整合包可能需要 8/17。")}
          >
            <select
              value={targetMajor}
              onChange={(e) => setTargetMajor(Number(e.target.value))}
              disabled={installing}
            >
              {majorOptions.map((major) => (
                <option key={major} value={major}>
                  Java {major}
                  {lts.includes(major) ? " (LTS)" : ""}
                  {runtimeOptions?.recommended === major ? tr(" · 推荐") : ""}
                </option>
              ))}
            </select>
          </Field>
          <Button variant="primary" icon="download" loading={installing} onClick={installRuntime}>
            {tr("下载并安装")}</Button>
        </div>
        {installing && javaTask && isActive(javaTask.status) ? (
          <div className="java-install-task">
            <div className="resource-task__head">
              <Badge tone={taskTone(javaTask.status)}>{TASK_LABELS[javaTask.status]}</Badge>
              <span className="ellipsis">{javaTask.currentFile ?? javaTask.title}</span>
            </div>
            <ProgressBar value={taskProgress(javaTask)} label={TASK_LABELS[javaTask.status]} />
            <span className="muted small">
              {formatBytes(javaTask.doneBytes)} / {formatBytes(javaTask.totalBytes)}
            </span>
          </div>
        ) : null}
      </Card>

      <Card>
        <div className="card-head">
          <div>
            <h3>{tr("手动指定 Java")}</h3>
            <p>{tr("适用于自动扫描未覆盖的安装路径，例如自编译 JDK。")}</p>
          </div>
        </div>
        <div className="inline-form">
          <Field label={tr("java 可执行文件路径")} error={error} hint={tr("Linux/macOS 通常为 .../bin/java，Windows 为 ...\\\\bin\\\\java.exe")}>
            <input
              value={path}
              onChange={(e) => setPath(e.target.value)}
              placeholder="/Library/Java/JavaVirtualMachines/.../bin/java"
              className="mono"
            />
          </Field>
          <Button variant="primary" onClick={add} loading={busy}>
            {tr("检测并添加")}</Button>
        </div>
      </Card>

      {javas.length === 0 ? (
        <Card>
          <EmptyState
            icon="coffee"
            title={tr("未发现 Java")}
            description={tr("安装 Java 21+（较新版本可能需要 Java 25），然后点击重新扫描，或手动指定路径。")}
            action={
              <Button icon="refresh" onClick={refresh} loading={busy}>
                {tr("重新扫描")}</Button>
            }
          />
        </Card>
      ) : (
        <div className="java-list">
          {javas.map((j) => (
            <Card key={j.path} className="java-card">
              <div className="java-card__main">
                <div className="java-version">
                  <span>Java</span>
                  <strong>{j.major}</strong>
                </div>
                <div className="java-card__info">
                  <div className="java-card__title">
                    <strong>{j.versionString}</strong>
                    {defaultJava === j.path ? <Badge tone="success">{tr("默认")}</Badge> : null}
                    <Badge>{j.source}</Badge>
                  </div>
                  <code className="ellipsis" title={j.path}>
                    {j.path}
                  </code>
                  <span className="muted small">
                    {j.vendor ?? tr("未知供应商")}
                    {j.arch ? ` · ${j.arch}` : ""}
                  </span>
                </div>
              </div>
              <Button
                size="sm"
                onClick={() => setDefault(j.path)}
                disabled={defaultJava === j.path || busy}
              >
                {defaultJava === j.path ? tr("当前默认") : tr("设为默认")}
              </Button>
            </Card>
          ))}
        </div>
      )}

      {installed.length > 0 ? (
        <Card>
          <div className="card-head">
            <div>
              <h3>{tr("版本兼容性")}</h3>
              <p>{tr("基于版本元数据中的 javaVersion.majorVersion 计算。")}</p>
            </div>
          </div>
          <ul className="compat-list">
            {installed.slice(0, 8).map((v) => {
              const check = checks[v.id];
              return (
                <li key={v.id}>
                  <div>
                    <strong>{v.id}</strong>
                    <span className="muted small">{tr("要求 Java")}{check?.requiredMajor ?? "—"}+</span>
                  </div>
                  {check ? (
                    <div className="compat-list__right">
                      <Badge
                        tone={
                          check.compatibility === "compatible"
                            ? "success"
                            : check.compatibility === "warning"
                              ? "warning"
                              : "danger"
                        }
                      >
                        Java {check.actualMajor}
                      </Badge>
                      <span className="muted small">{check.message}</span>
                    </div>
                  ) : (
                    <span className="muted small">{tr("检查中…")}</span>
                  )}
                </li>
              );
            })}
          </ul>
        </Card>
      ) : null}

      <div className="notice">
        <Icon name="info" size={17} />
        <div>
          <strong>{tr("关于自动下载 Java")}</strong>
          <p>
            {tr("安装包取自 Eclipse Adoptium（Temurin）公开 API，按接口提供的 SHA-256 校验后解压；只写入启动器 java/ 目录，不修改 JAVA_HOME。Temurin 使用 GPLv2 + Classpath Exception，可再分发。")}</p>
        </div>
      </div>
    </div>
  );
}
