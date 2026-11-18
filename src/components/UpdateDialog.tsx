import { useEffect, useMemo, useState } from "react";
import { api, onEvent, type UpdateAsset, type UpdateProgress } from "../lib/api";
import { formatBytes } from "../lib/format";
import { useApp } from "../state/AppContext";
import { Button, ProgressBar } from "./ui";
import { Modal } from "./Modal";
import { MarkdownText } from "./MarkdownText";
import { t } from "../lib/i18n";


function pickAsset(platform: string, assets: UpdateAsset[]): UpdateAsset | null {
  const lower = platform.toLowerCase();
  const find = (test: (name: string) => boolean) =>
    assets.find((asset) => test(asset.name.toLowerCase())) ?? null;

  if (lower.includes("dmg") || lower.includes("mac") || lower.includes("darwin")) {
    return find((name) => name.endsWith(".dmg") && name.includes("universal"))
      ?? find((name) => name.endsWith(".dmg"));
  }
  if (lower.includes("zip") || lower.includes("windows")) {
    return find((name) => name.includes("windows") && name.endsWith(".zip"))
      ?? find((name) => name.endsWith(".zip"));
  }
  if (lower.includes("deb")) {
    return find((name) => name.endsWith(".deb"));
  }
  return find((name) => name.endsWith(".appimage"))
    ?? find((name) => name.endsWith(".deb"));
}

export function UpdateDialog() {
  const { bootstrap, updateInfo, updateOpen, setUpdateOpen, pushToast } = useApp();
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [busy, setBusy] = useState<"download" | "apply" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const asset = useMemo(
    () =>
      updateInfo
        ? pickAsset(
            updateInfo.installKind === "unknown"
              ? (bootstrap?.platform ?? "")
              : updateInfo.installKind,
            updateInfo.assets,
          )
        : null,
    [updateInfo, bootstrap?.platform],
  );

  useEffect(() => {
    if (!updateOpen) {
      setProgress(null);
      setError(null);
      setBusy(null);
      return;
    }
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void onEvent<UpdateProgress>("update-progress", (value) => setProgress(value)).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [updateOpen]);

  if (!updateInfo) return null;

  async function install() {
    if (!asset) return;
    setError(null);
    setBusy("download");
    try {
      const path = await api.downloadUpdate(asset.url, asset.name, asset.sha256);
      setBusy("apply");
      await api.applyUpdate(path, asset.name);
    } catch (e) {
      setBusy(null);
      setError(e instanceof Error ? e.message : String(e));
      pushToast(t("更新失败"), "error", e instanceof Error ? e.message : String(e));
    }
  }

  const percent = progress && progress.total > 0 ? progress.percent : null;

  return (
    <Modal
      open={updateOpen}
      onClose={() => {
        if (!busy) setUpdateOpen(false);
      }}
      title={t("发现新版本")}
      subtitle={`v${updateInfo.currentVersion} → v${updateInfo.latestVersion}`}
      wide
      footer={
        <>
          <Button variant="ghost" onClick={() => setUpdateOpen(false)} disabled={Boolean(busy)}>
            {t("稍后")}</Button>
          <Button variant="primary" onClick={() => void install()} loading={Boolean(busy)} disabled={!asset}>
            {busy === "apply" ? t("正在安装…") : t("下载并安装")}</Button>
        </>
      }
    >
      <div className="update-meta">
        <span>{t("当前版本")} <strong>v{updateInfo.currentVersion}</strong></span>
        <span>{t("最新版本")} <strong>v{updateInfo.latestVersion}</strong></span>
        {asset ? <span>{t("安装包")} {formatBytes(asset.size)}</span> : null}
      </div>

      {updateInfo.notes ? (
        <div className="update-notes">
          <MarkdownText text={updateInfo.notes} />
        </div>
      ) : null}

      {!asset ? (
        <div className="update-fallback">
          <p className="form-error">{t("当前平台没有可用的安装包，请前往 Release 页面手动下载。")}</p>
          <Button size="sm" onClick={() => void api.openTrustedUrl(updateInfo.releaseUrl)}>
            {t("打开 Release 页面")}</Button>
        </div>
      ) : null}

      {progress && busy ? (
        <div className="update-progress">
          <ProgressBar
            value={percent}
            label={
              progress.total > 0
                ? `${formatBytes(progress.downloaded)} / ${formatBytes(progress.total)}`
                : t("正在下载…")
            }
          />
        </div>
      ) : null}

      {busy === "apply" ? (
        <p className="update-status">{t("正在安装更新，启动器即将重启…")}</p>
      ) : null}
      {error ? <p className="form-error">{error}</p> : null}
    </Modal>
  );
}
