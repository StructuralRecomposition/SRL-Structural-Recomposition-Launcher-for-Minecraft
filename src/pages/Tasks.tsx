import { useState } from "react";
import { api, type TaskView } from "../lib/api";
import { formatBytes, formatEta, formatSpeed } from "../lib/format";
import { TASK_LABELS, isActive, taskProgress, taskTone } from "../lib/task";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, ProgressBar } from "../components/ui";
import { Icon } from "../components/Icon";

export function TasksPage() {
  const { tasks, pushToast, refreshTasks } = useApp();
  const [busy, setBusy] = useState<string | null>(null);
  const active = tasks.filter((t) => isActive(t.status));
  const finished = tasks.filter((t) => !isActive(t.status));

  async function cancel(task: TaskView) {
    setBusy(task.id);
    try {
      await api.cancelTask(task.id);
      pushToast("已请求取消任务", "info", task.title);
    } catch (e) {
      pushToast("取消失败", "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function clear() {
    try {
      await api.clearFinishedTasks();
      await refreshTasks();
      pushToast("已清除完成的任务记录", "info");
    } catch (e) {
      pushToast("清除失败", "error", e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <div className="page">
      <div className="toolbar">
        <div>
          <h2>下载任务</h2>
          <p className="muted">
            进度来自实际写入的字节数；无法确定总大小时不会伪造百分比。
          </p>
        </div>
        <div className="toolbar__actions">
          <Badge tone={active.length > 0 ? "accent" : "neutral"}>{active.length} 进行中</Badge>
          <Button icon="trash" onClick={clear} disabled={finished.length === 0}>
            清除已结束
          </Button>
        </div>
      </div>

      {tasks.length === 0 ? (
        <Card>
          <EmptyState
            icon="download"
            title="没有下载任务"
            description="在版本管理中安装游戏，任务会出现在这里。所有进度均为真实数据。"
          />
        </Card>
      ) : (
        <div className="task-sections">
          {active.length > 0 ? (
            <section>
              <h3 className="section-title">进行中</h3>
              <div className="task-list">
                {active.map((t) => (
                  <TaskCard key={t.id} task={t} onCancel={() => cancel(t)} busy={busy === t.id} />
                ))}
              </div>
            </section>
          ) : null}
          {finished.length > 0 ? (
            <section>
              <h3 className="section-title">已结束</h3>
              <div className="task-list">
                {finished.map((t) => (
                  <TaskCard key={t.id} task={t} />
                ))}
              </div>
            </section>
          ) : null}
        </div>
      )}
    </div>
  );
}

function TaskCard({
  task,
  onCancel,
  busy,
}: {
  task: TaskView;
  onCancel?: () => void;
  busy?: boolean;
}) {
  const pct = taskProgress(task);
  const totalUnknown = task.totalBytes <= 0;
  const pendingFiles = pct === null && !totalUnknown;
  return (
    <Card className="task-card">
      <div className="task-card__head">
        <div>
          <div className="task-card__title">
            <strong>{task.title}</strong>
            <Badge tone={taskTone(task.status)}>{TASK_LABELS[task.status]}</Badge>
          </div>
          <span className="muted small">
            {task.currentFile ?? "准备中…"}
            {task.totalFiles > 0 ? ` · ${task.doneFiles}/${task.totalFiles} 个文件` : ""}
          </span>
        </div>
        <div className="task-card__actions">
          <span className="muted small mono">{task.id.slice(0, 8)}</span>
          {onCancel ? (
            <Button size="sm" variant="danger" onClick={onCancel} loading={busy}>
              取消
            </Button>
          ) : null}
        </div>
      </div>

      {isActive(task.status) ? (
        <div className="task-card__progress">
          <ProgressBar
            value={pct}
            tone={task.status === "retrying" ? "warning" : "accent"}
            label={
              totalUnknown
                ? `已下载 ${formatBytes(task.doneBytes)} · 总量未知`
                : pendingFiles
                  ? `下载中 · ${task.doneFiles}/${task.totalFiles} 个文件`
                  : `${formatBytes(task.doneBytes)} / ${formatBytes(task.totalBytes)}`
            }
          />
          <div className="task-stats">
            <span>速度 {formatSpeed(task.speedBps)}</span>
            <span>剩余 {formatEta(task.etaSeconds)}</span>
            <span>{pct === null ? "进度未知" : `${pct.toFixed(1)}%`}</span>
          </div>
        </div>
      ) : (
        <div className="task-stats">
          <span>已处理 {formatBytes(task.doneBytes)}</span>
          {task.totalBytes > 0 ? <span>总计 {formatBytes(task.totalBytes)}</span> : null}
          <span>{task.doneFiles} 个文件</span>
        </div>
      )}

      {task.error ? (
        <details className="task-error">
          <summary>
            <Icon name="alert" size={15} />
            {task.error.message}
          </summary>
          {task.error.detail ? <pre>{task.error.detail}</pre> : null}
          <span className="mono small">{task.error.code}</span>
        </details>
      ) : null}
    </Card>
  );
}
