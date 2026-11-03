import { useEffect, useState } from "react";
import { api, type Instance, type WorldEntry } from "../lib/api";
import { formatBytes, relativeTime } from "../lib/format";
import { useApp } from "../state/AppContext";
import { Button, EmptyState } from "../components/ui";
import { Icon } from "../components/Icon";
import { Modal } from "../components/Modal";

export function MapManager({
  instance,
  onClose,
}: {
  instance: Instance;
  onClose: () => void;
}) {
  const { pushToast } = useApp();
  const [worlds, setWorlds] = useState<WorldEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    try {
      setWorlds((await api.listWorlds(instance.id)) ?? []);
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

  async function openSaves() {
    try {
      await api.openLauncherPath("instanceSaves", instance.id);
    } catch (e) {
      pushToast("无法打开存档目录", "error", e instanceof Error ? e.message : undefined);
    }
  }

  async function openWorld(world: WorldEntry) {
    setBusy(world.folder);
    try {
      await api.openWorldFolder(instance.id, world.folder);
    } catch (e) {
      pushToast("无法打开存档目录", "error", e instanceof Error ? e.message : undefined);
    } finally {
      setBusy(null);
    }
  }

  return (
    <Modal
      open
      onClose={onClose}
      title="地图"
      subtitle={`${instance.name} · saves/`}
      footer={
        <Button variant="ghost" onClick={onClose}>
          关闭
        </Button>
      }
    >
      <div className="world-toolbar">
        <span className="muted small">
          {loading ? "正在读取存档…" : `${worlds.length} 个地图存档`}
        </span>
        <div className="world-toolbar__actions">
          <Button size="sm" variant="ghost" icon="refresh" onClick={() => void load()}>
            刷新
          </Button>
          <Button size="sm" icon="folder" onClick={() => void openSaves()}>
            打开存档文件夹
          </Button>
        </div>
      </div>

      <div className="notice">
        <Icon name="info" size={16} />
        <p>
          这里只读取和打开存档，不会删除或修改世界。地图列表来自实例目录的{" "}
          <code className="mono">saves/</code>，每个文件夹对应一个单人世界。
        </p>
      </div>

      {error ? <p className="form-error">{error}</p> : null}

      {loading ? (
        <p className="muted">正在读取 saves/ 目录…</p>
      ) : worlds.length === 0 ? (
        <EmptyState
          icon="map"
          title="没有地图存档"
          description="进入单人游戏并创建世界后，存档会出现在这里。也可以打开存档文件夹手动查看。"
          action={
            <Button icon="folder" onClick={() => void openSaves()}>
              打开存档文件夹
            </Button>
          }
        />
      ) : (
        <ul className="world-list">
          {worlds.map((world) => (
            <li key={world.folder} className="world-row">
              <div className="world-row__icon">
                <Icon name="map" size={20} />
              </div>
              <div className="world-row__main">
                <strong>{world.name}</strong>
                <span className="muted small">
                  {world.regionFiles} 个区域文件 · {formatBytes(world.sizeBytes)}
                  {world.modifiedAt ? ` · 最后保存 ${relativeTime(world.modifiedAt)}` : ""}
                </span>
                <code className="mono small">{world.folder}</code>
              </div>
              <Button
                size="sm"
                icon="folder"
                loading={busy === world.folder}
                onClick={() => void openWorld(world)}
              >
                打开文件夹
              </Button>
            </li>
          ))}
        </ul>
      )}
    </Modal>
  );
}
