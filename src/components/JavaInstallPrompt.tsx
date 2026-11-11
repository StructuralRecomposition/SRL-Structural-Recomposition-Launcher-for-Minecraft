import { useEffect, useMemo, useState } from "react";
import { api, type JavaLaunchCheck } from "../lib/api";
import { useApp } from "../state/AppContext";
import { Badge, Button, ProgressBar } from "./ui";
import { Modal } from "./Modal";
import { t } from "../lib/i18n";
import { isActive, taskProgress } from "../lib/task";
import { formatBytes } from "../lib/format";

interface Props {
  instanceId: string;
  instanceName: string;
  onClose: () => void;
  /** Called only after a runtime was installed and verified. The caller must
   * still start the game explicitly; this dialog never launches twice. */
  onInstalled: () => void;
}

/**
 * Pre-flight dialog shown when `launch_instance` reports JAVA_INVALID or a
 * missing/incompatible runtime. It never installs silently and never changes
 * the launcher-wide default Java.
 */
export function JavaInstallPrompt({ instanceId, instanceName, onClose, onInstalled }: Props) {
  const { tasks, pushToast, reload } = useApp();
  const [check, setCheck] = useState<JavaLaunchCheck | null>(null);
  const [loading, setLoading] = useState(true);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    setLoading(true);
    api
      .javaLaunchCheck(instanceId)
      .then((value) => {
        if (!disposed) setCheck(value);
      })
      .catch((e) => {
        if (!disposed) setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (!disposed) setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [instanceId]);

  const task = useMemo(() => {
    const candidates = tasks.filter((item) => item.kind === "java" && isActive(item.status));
    return candidates[candidates.length - 1];
  }, [tasks]);

  async function install() {
    if (!check || installing) return;
    setInstalling(true);
    setError(null);
    try {
      const info = await api.installJavaForVersion(check.requiredMajor, false);
      pushToast(t("Java {0} 已安装", [info.major]), "success", info.path);
      await reload();
      const next = await api.javaLaunchCheck(instanceId);
      setCheck(next);
      if (next.compatible && next.selected) {
        onInstalled();
      } else {
        setError(next.message);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setInstalling(false);
    }
  }

  const progress = task ? taskProgress(task) : null;

  return (
    <Modal
      open
      onClose={onClose}
      title={t("无法启动 Minecraft")}
      subtitle={instanceName}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={installing}>
            {t("取消")}
          </Button>
          {check?.canInstall ? (
            <Button
              variant="primary"
              icon="download"
              loading={installing}
              disabled={loading || !check.canInstall}
              onClick={() => void install()}
            >
              {t("下载并安装 Java {0}", [check.requiredMajor])}
            </Button>
          ) : null}
        </>
      }
    >
      {loading ? <p className="muted">{t("正在检查 Java 环境…")}</p> : null}
      {check ? (
        <div className="java-prompt">
          <dl className="mp-facts">
            <div>
              <dt>{t("需要 Java")}</dt>
              <dd>
                <Badge tone="accent">Java {check.requiredMajor}</Badge>
              </dd>
            </div>
            <div>
              <dt>{t("兼容范围")}</dt>
              <dd className="mono">
                {check.requiredMajor}–{check.maxSupportedMajor}
              </dd>
            </div>
            <div>
              <dt>{t("当前选择")}</dt>
              <dd>
                {check.selected
                  ? `Java ${check.selected.major} · ${check.selected.versionString}`
                  : t("未找到")}
              </dd>
            </div>
            <div>
              <dt>{t("实例指定 Java")}</dt>
              <dd>{check.instancePinned ? check.pinnedPath ?? t("是") : t("自动选择")}</dd>
            </div>
          </dl>
          <p className={check.compatible ? "muted" : "form-error"}>{check.message}</p>
          {check.needsReset ? (
            <p className="muted small">
              {t("实例显式指定的 Java 不会被自动替换。请到实例设置把 Java 改为自动选择，或手动指定兼容版本。")}
            </p>
          ) : null}
          {task ? (
            <div className="resource-task">
              <div className="resource-task__head">
                <Badge tone="info">{task.status}</Badge>
                <span className="ellipsis">{task.currentFile ?? task.title}</span>
              </div>
              <ProgressBar value={progress} label={t("下载 Java")} />
              <div className="resource-task__meta">
                <span>
                  {formatBytes(task.doneBytes)} / {formatBytes(task.totalBytes)}
                </span>
                {task.speedBps > 0 ? (
                  <span>{formatBytes(task.speedBps)}/s</span>
                ) : null}
              </div>
            </div>
          ) : null}
          {error ? <p className="form-error">{error}</p> : null}
        </div>
      ) : null}
      {!loading && !check && error ? <p className="form-error">{error}</p> : null}
    </Modal>
  );
}
