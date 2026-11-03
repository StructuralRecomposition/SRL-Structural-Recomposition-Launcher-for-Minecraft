import { useEffect, useState } from "react";
import {
  api,
  type ImportOutcome,
  type Instance,
  type ResourcePackEntry,
} from "../lib/api";
import { formatBytes, relativeTime } from "../lib/format";
import { MAX_UPLOAD_BYTES, pickFiles, readFileBytes } from "../lib/files";
import { useApp } from "../state/AppContext";
import { Badge, Button, EmptyState, Field, IconButton } from "./ui";
import { Icon } from "./Icon";
import { Modal } from "./Modal";
import { DropZone } from "./DropZone";

const PACK_ACCEPT = [".zip"];

export function ResourcePackManager({
  instance,
  onClose,
}: {
  instance: Instance;
  onClose: () => void;
}) {
  const { pushToast, running } = useApp();
  const [packs, setPacks] = useState<ResourcePackEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [overwrite, setOverwrite] = useState(false);
  const [manualOpen, setManualOpen] = useState(false);
  const [manualPath, setManualPath] = useState("");
  const [deleting, setDeleting] = useState<string | null>(null);

  const importing = busy === "import" || busy?.startsWith("file:") === true;
  const isRunning = running.has(instance.id);

  async function load() {
    setLoading(true);
    try {
      setPacks((await api.listResourcepacks(instance.id)) ?? []);
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
    if (ok.length > 0) {
      pushToast(
        ok.length === 1 ? "材质包已导入" : `已导入 ${ok.length} 个材质包`,
        "success",
        ok.map((o) => o.fileName).join("、"),
      );
    }
    setError(
      bad.length > 0
        ? bad.map((o) => `${o.fileName}：${o.error ?? "导入失败"}`).join("；")
        : null,
    );
  }

  async function importPaths(paths: string[]) {
    if (paths.length === 0) return;
    setBusy("import");
    setError(null);
    try {
      const outcomes = await api.importResourcepacks(instance.id, paths, overwrite);
      report(outcomes);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function importPicked() {
    const files = await pickFiles(PACK_ACCEPT.join(","), true);
    if (files.length === 0) return;
    setError(null);
    const outcomes: ImportOutcome[] = [];
    for (const file of files) {
      setBusy(`file:${file.name}`);
      try {
        if (file.size > MAX_UPLOAD_BYTES) {
          throw new Error("文件超过 512MB，请改用拖拽或手动路径导入");
        }
        const bytes = await readFileBytes(file);
        const entry = await api.importResourcepackBytes(
          instance.id,
          file.name,
          bytes,
          overwrite,
        );
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
      const outcomes = await api.importResourcepacks(
        instance.id,
        [manualPath.trim()],
        overwrite,
      );
      const outcome = outcomes[0];
      if (!outcome || !outcome.ok) {
        throw new Error(outcome?.error ?? "导入失败");
      }
      setManualPath("");
      pushToast("材质包已导入", "success", outcome.fileName);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function toggle(entry: ResourcePackEntry) {
    setBusy(entry.fileName);
    setError(null);
    try {
      const updated = await api.setResourcepackEnabled(
        instance.id,
        entry.fileName,
        !entry.enabled,
      );
      setPacks((prev) =>
        prev.map((p) => (p.fileName === entry.fileName ? updated : p)),
      );
      pushToast(
        updated.enabled ? "材质包已启用" : "材质包已禁用",
        "info",
        updated.fileName,
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function remove(entry: ResourcePackEntry) {
    setBusy(entry.fileName);
    try {
      await api.deleteResourcepack(instance.id, entry.fileName);
      setDeleting(null);
      await load();
      pushToast("材质包已删除", "info", entry.fileName);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function openFolder() {
    try {
      await api.openLauncherPath("instanceResourcepacks", instance.id);
    } catch (e) {
      pushToast("无法打开文件夹", "error", e instanceof Error ? e.message : undefined);
    }
  }

  return (
    <Modal
      open
      onClose={onClose}
      title="材质"
      subtitle={`${instance.name} · resourcepacks/`}
      footer={
        <Button variant="ghost" onClick={onClose}>
          关闭
        </Button>
      }
    >
      <div className="notice">
        <Icon name="palette" size={16} />
        <p>
          材质包会复制到实例的 <code className="mono">resourcepacks/</code>；启用状态写入{" "}
          <code className="mono">options.txt</code>。ZIP 根目录必须包含{" "}
          <code className="mono">pack.mcmeta</code>。
        </p>
      </div>
      {isRunning ? (
        <div className="notice notice--warning">
          <Icon name="alert" size={16} />
          <p>游戏正在运行：修改启用状态可能在退出时被游戏覆盖，建议退出游戏后调整。</p>
        </div>
      ) : null}

      <div className="file-toolbar">
        <DropZone
          accept={PACK_ACCEPT}
          onPaths={(paths) => void importPaths(paths)}
          title="拖入 .zip 材质包到这里"
          hint="支持一次拖入多个文件；只复制，不会修改源文件。"
          disabled={importing}
        />
        <div className="file-toolbar__buttons">
          <Button
            variant="primary"
            icon="plus"
            onClick={() => void importPicked()}
            loading={importing}
          >
            选择 .zip 材质包
          </Button>
          <Button icon="folder" onClick={() => void openFolder()}>
            打开文件夹
          </Button>
          <Button variant="ghost" onClick={() => setManualOpen((v) => !v)}>
            手动路径
          </Button>
        </div>
      </div>

      {manualOpen ? (
        <div className="mod-import">
          <Field label="从本地路径导入" hint="只接受 .zip；文件名会保留。">
            <input
              className="mono"
              value={manualPath}
              onChange={(e) => setManualPath(e.target.value)}
              placeholder="/Users/you/Downloads/MyPack.zip"
            />
          </Field>
          <label className="inline-check">
            <input
              type="checkbox"
              checked={overwrite}
              onChange={(e) => setOverwrite(e.target.checked)}
            />
            覆盖同名文件
          </label>
          <Button
            variant="primary"
            icon="plus"
            onClick={() => void addManual()}
            loading={busy === "import"}
            disabled={!manualPath.trim()}
          >
            导入
          </Button>
        </div>
      ) : null}

      <div className="file-summary">
        <span className="muted small">
          {packs.length} 个材质包 · {packs.filter((p) => p.enabled).length} 个启用
        </span>
        <label className="inline-check">
          <input
            type="checkbox"
            checked={overwrite}
            onChange={(e) => setOverwrite(e.target.checked)}
          />
          覆盖同名文件
        </label>
      </div>

      {error ? <p className="form-error">{error}</p> : null}

      {loading ? (
        <p className="muted">正在读取 resourcepacks/ 目录…</p>
      ) : packs.length === 0 ? (
        <EmptyState
          icon="palette"
          title="没有材质包"
          description="把 .zip 材质包拖进上面的区域，或点击“选择 .zip 材质包”。"
        />
      ) : (
        <ul className="mod-list">
          {packs.map((entry) => (
            <li
              key={entry.fileName}
              className={entry.enabled ? "" : "mod-row--off"}
            >
              <div className="mod-row__main">
                <strong className="mono">
                  {entry.fileName}
                  {entry.managed ? (
                    <span className="rp-badge">
                      <Badge tone="accent">皮肤自动管理</Badge>
                    </span>
                  ) : null}
                </strong>
                <span className="muted small">
                  {entry.isDir ? "文件夹" : formatBytes(entry.size)}
                  {entry.enabled ? " · 已启用" : " · 已禁用"}
                  {entry.modifiedAt ? ` · ${relativeTime(entry.modifiedAt)}` : ""}
                </span>
              </div>
              <label className="inline-check" title={entry.managed ? "由账号皮肤自动管理" : undefined}>
                <input
                  type="checkbox"
                  checked={entry.enabled}
                  disabled={entry.managed || busy === entry.fileName}
                  onChange={() => void toggle(entry)}
                />
                启用
              </label>
              {entry.managed ? (
                <span className="rp-lock" title="由账号皮肤自动管理">
                  <Icon name="shield" size={16} />
                </span>
              ) : deleting === entry.fileName ? (
                <span className="mod-confirm">
                  <Button
                    size="sm"
                    variant="danger"
                    loading={busy === entry.fileName}
                    onClick={() => void remove(entry)}
                  >
                    确认删除
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => setDeleting(null)}>
                    取消
                  </Button>
                </span>
              ) : (
                <IconButton
                  icon="trash"
                  label={`删除材质包 ${entry.fileName}`}
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
