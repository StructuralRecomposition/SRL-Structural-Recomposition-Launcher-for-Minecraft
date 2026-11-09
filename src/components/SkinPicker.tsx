import { useCallback, useEffect, useState } from "react";
import { api, type Skin, type SkinModel } from "../lib/api";
import { useApp } from "../state/AppContext";
import { pickFiles, readFileBytes } from "../lib/files";
import { Badge, Button, Field } from "../components/ui";
import { DropZone } from "../components/DropZone";
import { Icon } from "../components/Icon";
import { SkinFigure, SkinHead } from "../components/SkinPreview";
import { t } from "../lib/i18n";


export interface SkinSelection {
  skinId: string | null;
  model: SkinModel;
}

type Tab = "builtin" | "mine" | "import" | "mojang";

const SOURCE_LABEL: Record<Skin["source"], string> = {
  builtin: t("官方默认"),
  local: t("本地导入"),
  mojang: t("正版玩家"),
};

function modelLabel(model: SkinModel) {
  return model === "slim" ? t("纤细") : t("标准");
}

export function SkinPicker({
  value,
  onChange,
  disabled,
}: {
  value: SkinSelection;
  onChange: (next: SkinSelection) => void;
  disabled?: boolean;
}) {
  const { pushToast } = useApp();
  const [skins, setSkins] = useState<Skin[]>([]);
  const [builtinHint, setBuiltinHint] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [tab, setTab] = useState<Tab>("builtin");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [playerName, setPlayerName] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const list = await api.listSkins();
      setSkins(list?.skins ?? []);
      setBuiltinHint(list?.builtinHint ?? null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const selected = skins.find((s) => s.id === value.skinId) ?? null;
  const builtins = skins.filter((s) => s.source === "builtin" && s.model === value.model);
  const mine = skins.filter((s) => s.source !== "builtin");

  function selectSkin(skin: Skin) {
    onChange({ skinId: skin.id, model: skin.model });
    setError(null);
  }

  function setModel(model: SkinModel) {
    let nextId = value.skinId;
    if (selected?.source === "builtin") {
      const key = selected.id.replace(/^builtin-(classic|slim)-/, "");
      const sibling = skins.find((s) => s.id === `builtin-${model}-${key}`);
      if (sibling) nextId = sibling.id;
    }
    onChange({ skinId: nextId, model });
  }

  async function importFiles(files: File[]) {
    const file = files[0];
    if (!file) return;
    if (file.size > 2 * 1024 * 1024) {
      setError(t("皮肤文件超过 2 MB"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const bytes = await readFileBytes(file);
      const skin = await api.importSkinBytes(file.name, bytes, value.model);
      await load();
      onChange({ skinId: skin.id, model: skin.model });
      setTab("mine");
      pushToast(t("皮肤已导入"), "success", `${skin.name} · ${skin.width}×${skin.height}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function importPath(path: string) {
    setBusy(true);
    setError(null);
    try {
      const skin = await api.importSkinFile(path, value.model);
      await load();
      onChange({ skinId: skin.id, model: skin.model });
      setTab("mine");
      pushToast(t("皮肤已导入"), "success", `${skin.name} · ${skin.width}×${skin.height}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function fetchByName() {
    const name = playerName.trim();
    if (!name) return;
    setBusy(true);
    setError(null);
    try {
      const skin = await api.fetchSkinByName(name);
      await load();
      onChange({ skinId: skin.id, model: skin.model });
      setTab("mine");
      pushToast(t("已获取正版玩家皮肤"), "success", skin.owner ?? skin.name);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="skin-picker">
      <div className="skin-picker__main">
        <div className="segmented skin-tabs" role="tablist" aria-label={t("皮肤来源")}>
          {(
            [
              ["builtin", t("内置默认")],
              ["mine", t("我的皮肤")],
              ["import", t("导入本地")],
              ["mojang", t("正版玩家")],
            ] as [Tab, string][]
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={tab === id}
              className={`segmented__item ${tab === id ? "segmented__item--active" : ""}`}
              onClick={() => setTab(id)}
              disabled={disabled}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="skin-model-row">
          <span className="muted small">{t("模型")}</span>
          <div className="segmented">
            {(
              [
                ["classic", t("标准")],
                ["slim", t("纤细")],
              ] as [SkinModel, string][]
            ).map(([id, label]) => (
              <button
                key={id}
                type="button"
                className={`segmented__item ${value.model === id ? "segmented__item--active" : ""}`}
                onClick={() => setModel(id)}
                disabled={disabled}
              >
                {label}
              </button>
            ))}
          </div>
          <span className="muted small">
            {value.model === "slim" ? t("3 像素手臂") : t("4 像素手臂")}
          </span>
        </div>

        {tab === "builtin" ? (
          loading ? (
            <p className="muted small">{t("正在读取皮肤库…")}</p>
          ) : builtins.length === 0 ? (
            <div className="skin-empty">
              <Icon name="info" size={18} />
              <p>
                {builtinHint ??
                  t("没有可用的默认皮肤。安装 1.20.2+ 原版后会自动从游戏文件中提取官方默认皮肤。")}
              </p>
            </div>
          ) : (
            <div className="skin-grid">
              {builtins.map((skin) => (
                <SkinCard
                  key={skin.id}
                  skin={skin}
                  active={skin.id === value.skinId}
                  disabled={disabled}
                  onClick={() => selectSkin(skin)}
                />
              ))}
            </div>
          )
        ) : null}

        {tab === "mine" ? (
          loading ? (
            <p className="muted small">{t("正在读取皮肤库…")}</p>
          ) : mine.length === 0 ? (
            <div className="skin-empty">
              <Icon name="info" size={18} />
              <p>{t("还没有导入或获取的皮肤。可从“导入本地”或“正版玩家”添加。")}</p>
            </div>
          ) : (
            <div className="skin-grid">
              {mine.map((skin) => (
                <SkinCard
                  key={skin.id}
                  skin={skin}
                  active={skin.id === value.skinId}
                  disabled={disabled}
                  onClick={() => selectSkin(skin)}
                />
              ))}
            </div>
          )
        ) : null}

        {tab === "import" ? (
          <div className="skin-import">
            <DropZone
              accept={[".png"]}
              onPaths={(paths) => void importPath(paths[0])}
              title={t("拖入 PNG 皮肤到这里")}
              hint={t("支持 64×64、128×128 与旧版 64×32；会保留原图并生成 64×64 兼容副本。")}
              disabled={disabled || busy}
            />
            <div className="skin-import__actions">
              <Button
                variant="primary"
                icon="plus"
                loading={busy}
                disabled={disabled}
                onClick={async () => {
                  const files = await pickFiles("image/png", false);
                  if (files.length > 0) await importFiles(files);
                }}
              >
                {t("选择 PNG 文件")}</Button>
              <span className="muted small">{t("文件不会上传到任何服务器。")}</span>
            </div>
          </div>
        ) : null}

        {tab === "mojang" ? (
          <div className="skin-mojang">
            <Field
              label={t("正版玩家名称")}
              hint={t("通过 Mojang 公开档案读取该玩家当前皮肤，不需要登录。")}
            >
              <input
                value={playerName}
                maxLength={16}
                placeholder="Notch"
                onChange={(e) => setPlayerName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void fetchByName();
                }}
                disabled={disabled || busy}
              />
            </Field>
            <Button
              variant="primary"
              icon="download"
              loading={busy}
              disabled={disabled || playerName.trim().length < 3}
              onClick={() => void fetchByName()}
            >
              {t("获取皮肤")}</Button>
          </div>
        ) : null}

        {error ? <p className="form-error">{error}</p> : null}
      </div>

      <aside className="skin-preview-pane">
        <div className="skin-preview-stage">
          {selected ? (
            <SkinFigure src={selected.preview} model={value.model} height={158} />
          ) : (
            <span className="muted small">{t("未选择皮肤")}</span>
          )}
        </div>
        <div className="skin-preview-meta">
          <strong>{selected ? selected.name : t("使用账号当前皮肤")}</strong>
          <div className="skin-preview-tags">
            {selected ? <Badge tone="neutral">{SOURCE_LABEL[selected.source]}</Badge> : null}
            {selected ? <Badge tone="info">{modelLabel(value.model)}</Badge> : null}
            {selected ? (
              <Badge tone="neutral">
                {selected.width}×{selected.height}
              </Badge>
            ) : null}
          </div>
          {selected?.owner ? <span className="muted small">{t("来源玩家：")}{selected.owner}</span> : null}
        </div>
        {value.skinId ? (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => onChange({ skinId: null, model: value.model })}
            disabled={disabled}
          >
            {t("不设置皮肤")}</Button>
        ) : null}
        <p className="muted skin-note">
          {t("模型只影响手臂宽度。正版账号上传皮肤时会写入官方皮肤服务；离线档案使用本机资源包，手臂模型由客户端决定。")}</p>
      </aside>
    </div>
  );
}

function SkinCard({
  skin,
  active,
  disabled,
  onClick,
}: {
  skin: Skin;
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`skin-card ${active ? "skin-card--active" : ""}`}
      onClick={onClick}
      disabled={disabled}
      aria-pressed={active}
    >
      <SkinHead src={skin.preview} size={40} />
      <span className="skin-card__name">{skin.name}</span>
      <span className="skin-card__meta">{modelLabel(skin.model)}</span>
      {active ? (
        <span className="skin-card__check">
          <Icon name="check" size={13} />
        </span>
      ) : null}
    </button>
  );
}
