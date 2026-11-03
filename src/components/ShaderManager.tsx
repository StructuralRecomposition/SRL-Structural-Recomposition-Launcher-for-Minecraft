import { useEffect, useState } from "react";
import {
  api,
  type ImportOutcome,
  type Instance,
  type ShaderEntry,
  type ShaderSupport,
} from "../lib/api";
import { formatBytes } from "../lib/format";
import { MAX_UPLOAD_BYTES, pickFiles, readFileBytes } from "../lib/files";
import { useApp } from "../state/AppContext";
import { Button, EmptyState, Field, IconButton } from "./ui";
import { Icon } from "./Icon";
import { Modal } from "./Modal";
import { DropZone } from "./DropZone";

const SHADER_ACCEPT = [".zip"];

export function ShaderManager({ instance, onClose }: { instance: Instance; onClose: () => void }) {
  const { pushToast } = useApp();
  const [packs, setPacks] = useState<ShaderEntry[]>([]);
  const [support, setSupport] = useState<ShaderSupport | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [overwrite, setOverwrite] = useState(false);
  const [manualOpen, setManualOpen] = useState(false);
  const [manualPath, setManualPath] = useState("");
  const [deleting, setDeleting] = useState<string | null>(null);

  const importing = busy === "import" || busy?.startsWith("file:") === true;
  const loader = instance.modLoader;
  const loaderKind = (
    loader?.kind ??
    (instance.versionId.includes("-forge-")
      ? "Forge"
      : instance.versionId.startsWith("neoforge-")
        ? "NeoForge"
        : instance.versionId.startsWith("fabric-loader-")
          ? "Fabric"
          : instance.versionId.startsWith("quilt-loader-")
            ? "Quilt"
            : "")
  ).toLowerCase();
  const supported = Boolean(support?.iris || support?.optifine);

  async function load() {
    setLoading(true);
    try {
      const [nextPacks, nextSupport] = await Promise.all([
        api.listShaders(instance.id),
        api.shaderSupport(instance.id),
      ]);
      setPacks(nextPacks ?? []);
      setSupport(nextSupport ?? { iris: null, optifine: null });
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
        ok.length === 1 ? "光影包已导入" : `已导入 ${ok.length} 个光影包`,
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
      const outcomes = await api.importShaders(instance.id, paths, overwrite);
      report(outcomes);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function importPicked() {
    const files = await pickFiles(SHADER_ACCEPT.join(","), true);
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
        const entry = await api.importShaderBytes(instance.id, file.name, bytes, overwrite);
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
      const outcomes = await api.importShaders(instance.id, [manualPath.trim()], overwrite);
      const outcome = outcomes[0];
      if (!outcome || !outcome.ok) {
        throw new Error(outcome?.error ?? "导入失败");
      }
      setManualPath("");
      pushToast("光影包已导入", "success", outcome.fileName);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function toggle(entry: ShaderEntry) {
    setBusy(entry.fileName);
    setError(null);
    try {
      const updated = await api.toggleShader(instance.id, entry.fileName, !entry.enabled);
      setPacks((prev) => prev.map((p) => (p.fileName === entry.fileName ? updated : p)));
      pushToast(updated.enabled ? "光影包已启用" : "光影包已禁用", "info", updated.fileName);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function remove(entry: ShaderEntry) {
    setBusy(entry.fileName);
    try {
      await api.deleteShader(instance.id, entry.fileName);
      setDeleting(null);
      await load();
      pushToast("光影包已删除", "info", entry.fileName);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function installIris() {
    setBusy("iris");
    setError(null);
    try {
      const entry = await api.installIris(instance.id);
      pushToast("Iris 已安装", "success", entry.fileName);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function installOptiFine() {
    setBusy("optifine");
    setError(null);
    try {
      const entry = await api.installOptiFine(instance.id);
      pushToast("OptiFine 已安装到 mods/", "success", entry.fileName);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function openFolder() {
    try {
      await api.openLauncherPath("instanceShaderpacks", instance.id);
    } catch (e) {
      pushToast("无法打开文件夹", "error", e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <Modal
      open
      onClose={onClose}
      title="光影管理"
      subtitle={`${instance.name} · ${instance.versionId}`}
      wide
    >
      {!loader ? (
        <div className="notice notice--warning">
          <Icon name="alert" size={16} />
          <div>
            <strong>原版客户端无法加载光影</strong>
            <p>
              光影包需要 Iris（Fabric/Quilt）或 OptiFine。请先在加载器页安装 Fabric/Quilt 并创建实例。
            </p>
          </div>
        </div>
      ) : supported ? (
        <div className="notice notice--success">
          <Icon name="sun" size={16} />
          <div>
            <strong>已检测到光影支持</strong>
            <p>
              {support?.iris ? `Iris：${support.iris}` : null}
              {support?.iris && support?.optifine ? " · " : null}
              {support?.optifine ? `OptiFine：${support.optifine}` : null}
            </p>
          </div>
        </div>
      ) : (
        <div className="notice notice--warning">
          <Icon name="alert" size={16} />
          <div>
            <strong>当前实例还没有 Iris / OptiFine</strong>
            <p>
              Fabric/Quilt 可一键安装 Iris；Forge 可把 OptiFine 作为模组直接装入 mods/。
              安装后重启游戏即可在视频设置中选择光影。
            </p>
          </div>
          {(loaderKind === "fabric" || loaderKind === "quilt") ? (
            <Button
              size="sm"
              variant="primary"
              loading={busy === "iris"}
              onClick={() => void installIris()}
            >
              安装 Iris
            </Button>
          ) : loaderKind === "forge" ? (
            <div className="shader-install">
              <Button
                size="sm"
                variant="primary"
                loading={busy === "optifine"}
                onClick={() => void installOptiFine()}
              >
                安装 OptiFine
              </Button>
              <span className="muted small">
                OptiFine 与 Forge 版本强绑定，启动器会按 OptiFine 元数据校验；不兼容时拒绝安装。
              </span>
            </div>
          ) : (
            <span className="muted small">NeoForge 光影需 Oculus，暂未接入</span>
          )}
        </div>
      )}

      <div className="file-toolbar">
        <DropZone
          accept={SHADER_ACCEPT}
          onPaths={(paths) => void importPaths(paths)}
          title="拖入 .zip 光影包到这里"
          hint="支持一次拖入多个文件；光影包会放入 shaderpacks/。"
          disabled={importing}
        />
        <div className="file-toolbar__buttons">
          <Button variant="primary" icon="plus" onClick={() => void importPicked()} loading={importing}>
            选择 .zip 光影包
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
              placeholder="/Users/you/Downloads/BSL.zip"
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
          {packs.length} 个光影包 · {packs.filter((p) => p.enabled).length} 个启用
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
        <p className="muted">正在读取 shaderpacks/ 目录…</p>
      ) : packs.length === 0 ? (
        <EmptyState
          icon="sun"
          title="没有光影包"
          description="把 .zip 光影包拖进上面的区域，或点击“选择 .zip 光影包”。"
        />
      ) : (
        <ul className="mod-list">
          {packs.map((entry) => (
            <li key={entry.fileName} className={entry.enabled ? "" : "mod-row--off"}>
              <div className="mod-row__main">
                <strong className="mono">{entry.fileName}</strong>
                <span className="muted small">
                  {formatBytes(entry.size)}
                  {entry.enabled ? " · 已启用" : " · 已禁用"}
                </span>
              </div>
              <label className="inline-check">
                <input
                  type="checkbox"
                  checked={entry.enabled}
                  disabled={busy === entry.fileName}
                  onChange={() => void toggle(entry)}
                />
                启用
              </label>
              {deleting === entry.fileName ? (
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
                  label={`删除 ${entry.fileName}`}
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
