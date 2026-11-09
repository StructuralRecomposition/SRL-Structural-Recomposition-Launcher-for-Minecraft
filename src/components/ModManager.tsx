import { useEffect, useMemo, useState } from "react";
import {
  api,
  type ImportOutcome,
  type Instance,
  type ModEntry,
} from "../lib/api";
import { formatBytes } from "../lib/format";
import { MAX_UPLOAD_BYTES, pickFiles, readFileBytes } from "../lib/files";
import { useApp } from "../state/AppContext";
import { Badge, Button, EmptyState, Field, IconButton } from "./ui";
import { Icon } from "./Icon";
import { Modal } from "./Modal";
import { DropZone } from "./DropZone";
import { t } from "../lib/i18n";


const MOD_ACCEPT = [".jar"];

export function ModManager({ instance, onClose }: { instance: Instance; onClose: () => void }) {
  const { pushToast } = useApp();
  const [mods, setMods] = useState<ModEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [failed, setFailed] = useState<ImportOutcome[]>([]);
  const [overwrite, setOverwrite] = useState(false);
  const [manualOpen, setManualOpen] = useState(false);
  const [manualPath, setManualPath] = useState("");
  const [deleting, setDeleting] = useState<string | null>(null);

  const importing = busy === "import" || busy?.startsWith("file:") === true;
  const modLoader = instance.modLoader;

  const summary = useMemo(() => {
    const enabled = mods.filter((m) => m.enabled).length;
    return t("{0} 个文件 · {1} 个启用", [mods.length, enabled]);
  }, [mods]);

  async function load() {
    setLoading(true);
    try {
      setMods((await api.listMods(instance.id)) ?? []);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instance.id]);

  function report(outcomes: ImportOutcome[]) {
    const ok = outcomes.filter((o) => o.ok);
    const bad = outcomes.filter((o) => !o.ok);
    setFailed(bad);
    if (ok.length > 0) {
      pushToast(
        ok.length === 1 ? t("模组已导入") : t("已导入 {0} 个模组", [ok.length]),
        "success",
        ok.map((o) => o.fileName).join("、"),
      );
    }
    if (bad.length > 0) {
      setError(bad.map((o) => `${o.fileName}：${o.error ?? t("导入失败")}`).join("；"));
    } else {
      setError(null);
    }
  }

  async function importPaths(paths: string[]) {
    if (paths.length === 0) return;
    setBusy("import");
    setError(null);
    try {
      const outcomes = await api.importMods(instance.id, paths, overwrite);
      report(outcomes);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function importPicked() {
    const files = await pickFiles(MOD_ACCEPT.join(","), true);
    if (files.length === 0) return;
    setError(null);
    setFailed([]);
    const outcomes: ImportOutcome[] = [];
    for (const file of files) {
      setBusy(`file:${file.name}`);
      try {
        if (file.size > MAX_UPLOAD_BYTES) {
          throw new Error(t("文件超过 512MB，请改用拖拽或手动路径导入"));
        }
        const bytes = await readFileBytes(file);
        const entry = await api.importModBytes(instance.id, file.name, bytes, overwrite);
        outcomes.push({ fileName: entry.fileName, ok: true, error: null });
      } catch (e) {
        outcomes.push({
          fileName: file.name,
          ok: false,
          error: e instanceof Error ? e.message : String(e),
        });
      }
    }
    setBusy(null);
    report(outcomes);
    await load();
  }

  async function addManual() {
    if (!manualPath.trim()) return;
    setBusy("import");
    setError(null);
    try {
      const entry = await api.importMod(instance.id, manualPath.trim(), overwrite);
      setManualPath("");
      pushToast(t("模组已导入"), "success", entry.fileName);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function toggle(entry: ModEntry) {
    setBusy(entry.fileName);
    setError(null);
    try {
      const updated = await api.toggleMod(instance.id, entry.fileName, !entry.enabled);
      setMods((prev) => prev.map((m) => (m.fileName === entry.fileName ? updated : m)));
      pushToast(updated.enabled ? t("模组已启用") : t("模组已禁用"), "info", updated.fileName);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function remove(entry: ModEntry) {
    setBusy(entry.fileName);
    try {
      await api.deleteMod(instance.id, entry.fileName);
      setDeleting(null);
      await load();
      pushToast(t("模组已删除"), "info", entry.fileName);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function openFolder() {
    try {
      await api.openLauncherPath("instanceMods", instance.id);
    } catch (e) {
      pushToast(t("无法打开文件夹"), "error", e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <Modal
      open
      onClose={onClose}
      title={t("模组管理")}
      subtitle={`${instance.name} · ${instance.versionId}`}
      wide
    >
      {!modLoader ? (
        <div className="notice notice--warning">
          <Icon name="alert" size={16} />
          <div>
            <strong>{t("该实例没有记录模组加载器")}</strong>
            <p>{t("原版客户端不会加载 mods/ 中的模组。请使用 Fabric/Quilt 实例，或在加载器页安装。")}</p>
          </div>
        </div>
      ) : (
        <div className="notice">
          <Icon name="plug" size={16} />
          <div>
            <strong>{modLoader.kind} {modLoader.version}</strong>
            <p>{t("模组文件会立即生效于下次启动；切换启用状态只重命名文件，不移动内容。")}</p>
          </div>
        </div>
      )}

      <div className="file-toolbar">
        <DropZone
          accept={MOD_ACCEPT}
          onPaths={(paths) => void importPaths(paths)}
          title={t("拖入 .jar 模组到这里")}
          hint={t("支持一次拖入多个文件；只会复制，不会移动原文件。")}
          disabled={importing}
        />
        <div className="file-toolbar__buttons">
          <Button variant="primary" icon="plus" onClick={() => void importPicked()} loading={importing}>
            {t("选择 .jar 文件")}</Button>
          <Button icon="folder" onClick={() => void openFolder()}>
            {t("打开文件夹")}</Button>
          <Button variant="ghost" onClick={() => setManualOpen((v) => !v)}>
            {t("手动路径")}</Button>
        </div>
      </div>

      {manualOpen ? (
        <div className="mod-import">
          <Field label={t("从本地路径导入")} hint={t("适用于拖拽不可用的环境；文件名会保留。")}>
            <input
              className="mono"
              value={manualPath}
              onChange={(e) => setManualPath(e.target.value)}
              placeholder="/Users/you/Downloads/mod.jar"
            />
          </Field>
          <label className="inline-check">
            <input
              type="checkbox"
              checked={overwrite}
              onChange={(e) => setOverwrite(e.target.checked)}
            />
            {t("覆盖同名文件")}</label>
          <Button
            variant="primary"
            icon="plus"
            onClick={() => void addManual()}
            loading={busy === "import"}
            disabled={!manualPath.trim()}
          >
            {t("导入")}</Button>
        </div>
      ) : null}

      <div className="file-summary">
        <span className="muted small">{summary}</span>
        <label className="inline-check">
          <input
            type="checkbox"
            checked={overwrite}
            onChange={(e) => setOverwrite(e.target.checked)}
          />
          {t("覆盖同名文件")}</label>
      </div>

      {error ? <p className="form-error">{error}</p> : null}
      {failed.length > 0 ? (
        <ul className="issue-list">
          {failed.map((f) => (
            <li key={f.fileName + f.error}>
              <Badge tone="danger">{t("失败")}</Badge>
              <div>
                <strong>{f.fileName}</strong>
                <span className="muted small">{f.error}</span>
              </div>
            </li>
          ))}
        </ul>
      ) : null}

      {loading ? (
        <p className="muted">{t("正在读取 mods/ 目录…")}</p>
      ) : mods.length === 0 ? (
        <EmptyState
          icon="plug"
          title={t("没有模组文件")}
          description={t("把 .jar 拖进上面的区域，或点击“选择 .jar 文件”。")}
        />
      ) : (
        <ul className="mod-list">
          {mods.map((entry) => (
            <li key={entry.fileName} className={entry.enabled ? "" : "mod-row--off"}>
              <div className="mod-row__main">
                <strong className="mono">{entry.fileName}</strong>
                <span className="muted small">
                  {formatBytes(entry.size)}
                  {entry.enabled ? t(" · 已启用") : t(" · 已禁用")}
                </span>
              </div>
              <label className="inline-check">
                <input
                  type="checkbox"
                  checked={entry.enabled}
                  disabled={busy === entry.fileName}
                  onChange={() => void toggle(entry)}
                />
                {t("启用")}</label>
              {deleting === entry.fileName ? (
                <span className="mod-confirm">
                  <Button
                    size="sm"
                    variant="danger"
                    loading={busy === entry.fileName}
                    onClick={() => void remove(entry)}
                  >
                    {t("确认删除")}</Button>
                  <Button size="sm" variant="ghost" onClick={() => setDeleting(null)}>
                    {t("取消")}</Button>
                </span>
              ) : (
                <IconButton
                  icon="trash"
                  label={t("删除 {0}", [entry.fileName])}
                  onClick={() => setDeleting(entry.fileName)}
                />
              )}
            </li>
          ))}
        </ul>
      )}
    </Modal>
  );
}
