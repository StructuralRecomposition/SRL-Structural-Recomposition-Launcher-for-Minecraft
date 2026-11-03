import type { TaskStatus } from "./api";
import { percent } from "./format";

export const TASK_LABELS: Record<TaskStatus, string> = {
  queued: "等待中",
  running: "下载中",
  verifying: "校验中",
  completed: "已完成",
  cancelled: "已取消",
  retrying: "等待重试",
  failed: "失败",
};

export function taskTone(status: TaskStatus): "neutral" | "success" | "warning" | "danger" | "accent" {
  switch (status) {
    case "completed":
      return "success";
    case "failed":
      return "danger";
    case "cancelled":
      return "neutral";
    case "retrying":
    case "verifying":
      return "warning";
    case "running":
    case "queued":
      return "accent";
  }
}

/**
 * Byte totals can be incomplete when metadata omits file sizes (common for
 * Maven loader libraries). A full byte bar with files still downloading is
 * misleading, so fall back to an indeterminate bar until file count catches up.
 */
export function taskProgress(task: {
  doneBytes: number;
  totalBytes: number;
  doneFiles: number;
  totalFiles: number;
}): number | null {
  if (
    task.totalBytes > 0 &&
    task.doneBytes >= task.totalBytes &&
    task.totalFiles > 0 &&
    task.doneFiles < task.totalFiles
  ) {
    return null;
  }
  return percent(task.doneBytes, task.totalBytes);
}

export function isActive(status: TaskStatus): boolean {
  return status === "queued" || status === "running" || status === "verifying" || status === "retrying";
}
