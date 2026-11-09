import type { InstalledVersion } from "./api";
import { t } from "./i18n";


export type VersionBucket =
  | "release"
  | "snapshot"
  | "old_beta"
  | "old_alpha"
  | "loader"
  | "unknown";

export function versionBucket(v: InstalledVersion): VersionBucket {
  if (v.loader) return "loader";
  if (v.versionType === "release") return "release";
  if (v.versionType === "snapshot") return "snapshot";
  if (v.versionType === "old_beta") return "old_beta";
  if (v.versionType === "old_alpha") return "old_alpha";
  return "unknown";
}

export function versionBucketLabel(v: InstalledVersion): string {
  const bucket = versionBucket(v);
  if (bucket === "loader") return v.loader ?? t("加载器");
  if (bucket === "release") return t("正式版");
  if (bucket === "snapshot") return t("快照");
  if (bucket === "old_beta") return t("旧版 Beta");
  if (bucket === "old_alpha") return t("旧版 Alpha");
  return t("未知类型");
}

export function versionBucketTone(
  v: InstalledVersion,
): "success" | "info" | "warning" | "neutral" | "accent" {
  const bucket = versionBucket(v);
  if (bucket === "release") return "success";
  if (bucket === "snapshot") return "info";
  if (bucket === "old_beta" || bucket === "old_alpha") return "warning";
  if (bucket === "loader") return "accent";
  return "neutral";
}

export function manifestTypeLabel(type: string): string {
  switch (type) {
    case "release":
      return t("正式版");
    case "snapshot":
      return t("快照");
    case "old_beta":
      return t("旧版 Beta");
    case "old_alpha":
      return t("旧版 Alpha");
    default:
      return type;
  }
}
