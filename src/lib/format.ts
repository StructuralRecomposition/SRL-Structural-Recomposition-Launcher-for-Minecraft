
import { t as tr } from "./i18n";
export function formatBytes(bytes: number, signed = false): string {
  if (!Number.isFinite(bytes) || bytes < 0) return tr("未知");
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let i = 0;
  while (value >= 1024 && i < units.length - 1) {
    value /= 1024;
    i += 1;
  }
  const digits = value >= 100 || i === 0 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits)} ${units[i]}${signed ? "/s" : ""}`;
}

export function formatSpeed(bps: number): string {
  return bps > 0 ? formatBytes(bps, true) : "—";
}

export function formatEta(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds) || seconds <= 0) return "—";
  if (seconds < 60) return tr("{0} 秒", [Math.ceil(seconds)]);
  if (seconds < 3600) return tr("{0} 分 {1} 秒", [Math.floor(seconds / 60), Math.floor(seconds % 60)]);
  return tr("{0} 小时 {1} 分", [Math.floor(seconds / 3600), Math.floor((seconds % 3600) / 60)]);
}

export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(d);
}

export function relativeTime(iso: string | null | undefined): string {
  if (!iso) return tr("从未");
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return "—";
  const diff = Date.now() - t;
  const min = Math.floor(diff / 60000);
  if (min < 1) return tr("刚刚");
  if (min < 60) return tr("{0} 分钟前", [min]);
  const h = Math.floor(min / 60);
  if (h < 24) return tr("{0} 小时前", [h]);
  const d = Math.floor(h / 24);
  if (d < 30) return tr("{0} 天前", [d]);
  return formatDate(iso);
}

export function percent(done: number, total: number): number | null {
  if (!total || total <= 0) return null;
  return Math.min(100, Math.max(0, (done / total) * 100));
}

export function splitLines(text: string): string[] {
  return text
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
}

export function joinLines(lines: string[]): string {
  return lines.join("\n");
}

export function formatCount(value: number): string {
  if (!Number.isFinite(value) || value < 0) return "0";
  if (value >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(1)}B`;
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(value);
}
