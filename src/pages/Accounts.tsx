import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type Account, type DeviceCodeView, type Skin } from "../lib/api";
import { formatDate } from "../lib/format";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, Field, IconButton, Notice } from "../components/ui";
import { Modal } from "../components/Modal";
import { Icon } from "../components/Icon";
import { SkinHead } from "../components/SkinPreview";
import { SkinPicker, type SkinSelection } from "../components/SkinPicker";
import { t } from "../lib/i18n";


const DEFAULT_SELECTION: SkinSelection = { skinId: null, model: "classic" };

export function AccountsPage() {
  const { bootstrap, reload, pushToast, updateBootstrap } = useApp();
  const accounts = bootstrap?.accounts ?? [];
  const microsoft = bootstrap?.features.find((f) => f.id === "microsoft_login");
  const [open, setOpen] = useState(false);
  const [username, setUsername] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [removing, setRemoving] = useState<string | null>(null);
  const [msOpen, setMsOpen] = useState(false);
  const [msDevice, setMsDevice] = useState<DeviceCodeView | null>(null);
  const [msStatus, setMsStatus] = useState("pending");
  const [msError, setMsError] = useState<string | null>(null);
  const [msStarting, setMsStarting] = useState(false);
  const [msInterval, setMsInterval] = useState(5);
  const [clientId, setClientId] = useState(bootstrap?.settings.microsoftClientId ?? "");
  const [savingClient, setSavingClient] = useState(false);
  const [clientError, setClientError] = useState<string | null>(null);
  const [editingClient, setEditingClient] = useState(false);
  const [refreshing, setRefreshing] = useState<string | null>(null);

  const [skins, setSkins] = useState<Skin[]>([]);
  const [skinsLoading, setSkinsLoading] = useState(true);
  const [selection, setSelection] = useState<SkinSelection>(DEFAULT_SELECTION);
  const [editingSkin, setEditingSkin] = useState<Account | null>(null);
  const [editSelection, setEditSelection] = useState<SkinSelection>(DEFAULT_SELECTION);
  const [removingSkin, setRemovingSkin] = useState<string | null>(null);
  const [applyingSkin, setApplyingSkin] = useState<string | null>(null);

  const loadSkins = useCallback(async () => {
    setSkinsLoading(true);
    try {
      const list = await api.listSkins();
      setSkins(list?.skins ?? []);
    } catch (e) {
      pushToast(t("无法读取皮肤库"), "warning", e instanceof Error ? e.message : undefined);
    } finally {
      setSkinsLoading(false);
    }
  }, [pushToast]);

  useEffect(() => {
    void loadSkins();
  }, [loadSkins]);

  const skinsById = useMemo(() => new Map(skins.map((s) => [s.id, s])), [skins]);
  const mySkins = skins.filter((s) => s.source !== "builtin");

  useEffect(() => {
    if (!msOpen || !msDevice) return;
    let cancelled = false;
    let timer = 0;
    const poll = async () => {
      try {
        const result = await api.pollMicrosoftLogin(msDevice.deviceCode);
        if (cancelled) return;
        setMsStatus(result.status);
        if (result.status === "authorized") {
          pushToast(t("Microsoft 登录成功"), "success", result.account?.username ?? "");
          setMsOpen(false);
          setMsDevice(null);
          await reload();
          return;
        }
        if (["expired", "denied", "error"].includes(result.status)) {
          setMsError(result.message ?? t("登录未完成"));
          return;
        }
        const next = result.interval ?? msInterval;
        if (next !== msInterval) setMsInterval(next);
        timer = window.setTimeout(poll, next * 1000);
      } catch (e) {
        if (!cancelled) {
          setMsError(e instanceof Error ? e.message : String(e));
        }
      }
    };
    timer = window.setTimeout(poll, msInterval * 1000);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [msOpen, msDevice, msInterval, pushToast, reload]);

  async function saveClientId() {
    const value = clientId.trim();
    setClientError(null);
    if (!/^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(value)) {
      setClientError(t("client id 应是 Azure 应用注册页显示的 GUID"));
      return;
    }
    setSavingClient(true);
    try {
      const saved = await api.updateSettings({
        ...bootstrap!.settings,
        microsoftClientId: value,
      });
      updateBootstrap((b) => ({
        ...b,
        settings: saved,
        features: b.features.map((f) =>
          f.id === "microsoft_login" ? { ...f, available: true, reason: null } : f,
        ),
      }));
      setEditingClient(false);
      pushToast(t("Microsoft client id 已保存，正版登录已启用"), "success");
    } catch (e) {
      setClientError(e instanceof Error ? e.message : String(e));
    } finally {
      setSavingClient(false);
    }
  }

  async function refreshMicrosoft(account: Account) {
    setRefreshing(account.id);
    try {
      await api.refreshMicrosoftAccount(account.id);
      await reload();
      pushToast(t("Microsoft 授权已刷新"), "success", account.username);
    } catch (e) {
      pushToast(
        t("刷新授权失败"),
        "error",
        e instanceof Error ? e.message : String(e),
      );
    } finally {
      setRefreshing(null);
    }
  }

  async function startMicrosoft() {
    setMsStarting(true);
    setMsError(null);
    try {
      const device = await api.beginMicrosoftLogin();
      setMsDevice(device);
      setMsStatus("pending");
      setMsInterval(device.interval || 5);
      setMsOpen(true);
    } catch (e) {
      setMsError(e instanceof Error ? e.message : String(e));
      pushToast(t("无法开始 Microsoft 登录"), "error", e instanceof Error ? e.message : undefined);
    } finally {
      setMsStarting(false);
    }
  }

  function openAdd() {
    setUsername("");
    setSelection(DEFAULT_SELECTION);
    setError(null);
    setOpen(true);
  }

  async function add() {
    setError(null);
    const name = username.trim();
    if (!/^[A-Za-z0-9_]{3,16}$/.test(name)) {
      setError(t("用户名需为 3-16 位字母、数字或下划线"));
      return;
    }
    setBusy(true);
    try {
      await api.addOfflineAccount(
        name,
        selection.skinId,
        selection.skinId ? selection.model : null,
      );
      await reload();
      setOpen(false);
      pushToast(t("离线档案已创建"), "success", name);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function activate(id: string) {
    setBusy(true);
    try {
      await api.setActiveAccount(id);
      await reload();
      pushToast(t("已切换当前账号"), "success");
    } catch (e) {
      pushToast(t("切换失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: string) {
    setBusy(true);
    try {
      await api.removeAccount(id);
      setRemoving(null);
      await reload();
      pushToast(t("账号档案已移除"), "info");
    } catch (e) {
      pushToast(t("移除失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  function openSkinEditor(account: Account) {
    const knownId = account.skinId && skinsById.has(account.skinId) ? account.skinId : null;
    const skin = knownId ? skinsById.get(knownId) : undefined;
    setEditSelection({
      skinId: knownId,
      model: account.skinModel ?? skin?.model ?? "classic",
    });
    setEditingSkin(account);
  }

  async function saveSkin() {
    if (!editingSkin) return;
    setBusy(true);
    try {
      await api.setAccountSkin(
        editingSkin.id,
        editSelection.skinId,
        editSelection.skinId ? editSelection.model : null,
      );
      setEditingSkin(null);
      await reload();
      pushToast(t("皮肤设置已保存"), "success");
    } catch (e) {
      pushToast(t("保存皮肤失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function applyOfficialSkin(account: Account) {
    setApplyingSkin(account.id);
    try {
      const result = await api.applyAccountSkin(account.id);
      pushToast(t("皮肤已应用到正版账号"), "success", result.message);
    } catch (e) {
      pushToast(t("皮肤上传失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setApplyingSkin(null);
    }
  }

  async function deleteSkin(id: string) {
    setBusy(true);
    try {
      await api.deleteSkin(id);
      setRemovingSkin(null);
      await Promise.all([loadSkins(), reload()]);
      pushToast(t("皮肤已从皮肤库删除"), "info");
    } catch (e) {
      pushToast(t("删除皮肤失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="page accounts-page">
      <div className="toolbar">
        <div>
          <h2>{t("账号管理")}</h2>
        </div>
        <Button variant="primary" icon="plus" onClick={openAdd}>
          {t("添加离线档案")}</Button>
      </div>

      {accounts.length === 0 ? (
        <Card>
          <EmptyState
            icon="user"
            title={t("还没有账号档案")}
            description={t("添加一个离线档案即可开始。")}
            action={
              <Button variant="primary" icon="plus" onClick={openAdd}>
                {t("添加离线档案")}</Button>
            }
          />
        </Card>
      ) : (
        <div className="account-list">
          {accounts.map((a) => {
            const skin = a.skinId ? skinsById.get(a.skinId) : undefined;
            const model = a.skinModel ?? skin?.model;
            return (
              <Card key={a.id} className={`account-card ${a.active ? "account-card--active" : ""}`}>
                <div className="account-card__main">
                  <div className="account-avatar" aria-hidden="true">
                    {skin ? (
                      <SkinHead src={skin.preview} size={44} />
                    ) : (
                      a.username.slice(0, 1).toUpperCase()
                    )}
                  </div>
                  <div className="account-info">
                    <div className="account-info__title">
                      <strong>{a.username}</strong>
                      <Badge tone={a.kind === "offline" ? "neutral" : "info"}>
                        {a.kind === "offline" ? t("离线档案") : "Microsoft"}
                      </Badge>
                      {a.active ? <Badge tone="success">{t("当前")}</Badge> : null}
                      {skin ? (
                        <Badge tone="accent">
                          {skin.name} · {model === "slim" ? t("纤细") : t("标准")}
                        </Badge>
                      ) : null}
                    </div>
                    <code className="mono small">{a.uuid}</code>
                    <p className="muted small">{a.note}</p>
                    <span className="muted small">{t("创建于")}{formatDate(a.createdAt)}</span>
                  </div>
                </div>
                <div className="account-card__actions">
                  <Button size="sm" icon="edit" onClick={() => openSkinEditor(a)} disabled={busy}>
                    {skin ? t("更换皮肤") : t("设置皮肤")}
                  </Button>
                  {a.kind === "microsoft" && skin ? (
                    <Button
                      size="sm"
                      icon="download"
                      loading={applyingSkin === a.id}
                      onClick={() => void applyOfficialSkin(a)}
                    >
                      {t("上传皮肤")}</Button>
                  ) : null}
                  {a.kind === "microsoft" ? (
                    <Button
                      size="sm"
                      icon="refresh"
                      loading={refreshing === a.id}
                      onClick={() => void refreshMicrosoft(a)}
                    >
                      {t("刷新授权")}</Button>
                  ) : null}
                  <Button size="sm" onClick={() => activate(a.id)} disabled={a.active || busy}>
                    {a.active ? t("使用中") : t("设为当前")}
                  </Button>
                  <Button size="sm" variant="ghost" icon="trash" onClick={() => setRemoving(a.id)}>
                    {t("移除")}</Button>
                </div>
              </Card>
            );
          })}
        </div>
      )}

      <Card className="skin-library">
        <div className="card-head">
          <div>
            <h3>{t("我的皮肤")}</h3>
            <p>{t("本地导入与正版玩家皮肤保存在启动器数据目录，不会自动上传。")}</p>
          </div>
          <Badge tone={mySkins.length > 0 ? "info" : "neutral"}>{mySkins.length} {t("个")}</Badge>
        </div>
        {skinsLoading ? (
          <p className="muted small">{t("正在读取皮肤库…")}</p>
        ) : mySkins.length === 0 ? (
          <p className="muted small">
            {t("还没有保存的皮肤。创建账号或点击“设置皮肤”即可导入本地 PNG 或获取正版玩家皮肤。")}</p>
        ) : (
          <div className="skin-library-grid">
            {mySkins.map((skin) => (
              <div key={skin.id} className="skin-library-item">
                <SkinHead src={skin.preview} size={38} />
                <div className="skin-library-item__info">
                  <strong>{skin.name}</strong>
                  <span className="muted small">
                    {skin.source === "mojang" ? t("正版玩家") : t("本地")} ·{" "}
                    {skin.model === "slim" ? t("纤细") : t("标准")} · {skin.width}×{skin.height}
                  </span>
                </div>
                {removingSkin === skin.id ? (
                  <span className="skin-library-item__confirm">
                    <Button
                      size="sm"
                      variant="danger"
                      loading={busy}
                      onClick={() => void deleteSkin(skin.id)}
                    >
                      {t("删除")}</Button>
                    <Button size="sm" variant="ghost" onClick={() => setRemovingSkin(null)}>
                      {t("取消")}</Button>
                  </span>
                ) : (
                  <IconButton
                    icon="trash"
                    label={t("删除皮肤 {0}", [skin.name])}
                    onClick={() => setRemovingSkin(skin.id)}
                  />
                )}
              </div>
            ))}
          </div>
        )}
      </Card>

      <Notice tone="warning" title={t("Microsoft 正版登录暂时需要自行配置")}>
        {t("因条件有限，目前需要自行注册 Azure 应用并获取 client ID 才能登录；后续会修复为开箱即用，敬请期待。")}</Notice>

      <Card className="microsoft-card">
        <div className="card-head">
          <div>
            <h3>{t("Microsoft 正版登录")}</h3>
          </div>
          <Badge tone={microsoft?.available ? "success" : "warning"}>
            {microsoft?.available ? t("已配置") : t("未配置")}
          </Badge>
        </div>
        {!microsoft?.available || editingClient ? (
          <div className="microsoft-setup">
            <Field
              label={t("Azure 应用 client id")}
              hint={t("只填公开的 Application (client) ID，不是密钥。设备码流程无需密码或重定向 URI。")}
              error={clientError}
            >
              <input
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
                placeholder="00000000-0000-0000-0000-000000000000"
                className="mono"
                autoComplete="off"
              />
            </Field>
            <div className="toolbar__actions">
              <Button
                variant="primary"
                loading={savingClient}
                disabled={!clientId.trim()}
                onClick={saveClientId}
              >
                {t("保存并启用")}</Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() =>
                  void api
                    .openTrustedUrl("https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade")
                    .catch((e) =>
                      pushToast(
                        t("无法打开 Azure 门户"),
                        "warning",
                        e instanceof Error ? e.message : undefined,
                      ),
                    )
                }
              >
                {t("打开 Azure 应用注册")}</Button>
            </div>
            <details className="microsoft-guide">
              <summary>{t("如何获取 client id")}</summary>
              <ol>
                <li>{t("打开 Azure 门户 → Microsoft Entra ID → 应用注册 → 新注册。")}</li>
                <li>{t("名称随意；账户类型选“个人 Microsoft 账户”或“任何组织目录中的账户和个人 Microsoft 账户”（必须是后者之一，否则 /consumers 端点不适用）。")}</li>
                <li>{t("注册后复制“应用程序(客户端) ID”。")}</li>
                <li>{t("在“身份验证”中启用“允许公共客户端流”，设备码流程才能使用。")}</li>
                <li>{t("不需要客户端密钥；不要把密钥填到这里。")}</li>
              </ol>
            </details>
          </div>
        ) : (
          <div className="microsoft-card__actions">
            <Button onClick={startMicrosoft} loading={msStarting}>
              {t("使用 Microsoft 登录")}</Button>
            <span className="muted small">{t("设备码流程 · 不收集密码")}</span>
            <Button size="sm" variant="ghost" onClick={() => setEditingClient(true)}>
              {t("更换 client id")}</Button>
          </div>
        )}
      </Card>

      <Modal
        open={msOpen}
        onClose={() => {
          setMsOpen(false);
          setMsDevice(null);
        }}
        title={t("Microsoft 登录")}
        subtitle={t("在浏览器中完成授权后，这里会自动继续。")}
        footer={
          <Button
            variant="ghost"
            onClick={() => {
              setMsOpen(false);
              setMsDevice(null);
            }}
          >
            {t("取消登录")}</Button>
        }
      >
        {msDevice ? (
          <>
            <div className="device-code-box">
              <span>{t("在浏览器打开")}</span>
              <div className="device-code-row">
                <code>{msDevice.verificationUri}</code>
                <Button
                  size="sm"
                  variant="ghost"
                  icon="copy"
                  onClick={() => navigator.clipboard.writeText(msDevice.verificationUri)}
                >
                  {t("复制")}</Button>
                <Button
                  size="sm"
                  variant="ghost"
                  icon="folder"
                  onClick={() =>
                    void api
                      .openTrustedUrl(msDevice.verificationUri)
                      .catch((e) =>
                        pushToast(
                          t("无法打开浏览器"),
                          "warning",
                          e instanceof Error ? e.message : undefined,
                        ),
                      )
                  }
                >
                  {t("打开")}</Button>
              </div>
              <span>{t("并输入代码")}</span>
              <div className="device-code-row">
                <strong className="device-code">{msDevice.userCode}</strong>
                <Button
                  size="sm"
                  variant="ghost"
                  icon="copy"
                  onClick={() => navigator.clipboard.writeText(msDevice.userCode)}
                >
                  {t("复制")}</Button>
              </div>
            </div>
            <p className="muted small">{msDevice.message}</p>
            <p className="device-status">
              {msStatus === "pending"
                ? t("等待授权…")
                : msStatus === "slowDown"
                  ? t("等待授权（已降低轮询频率）…")
                  : msStatus === "expired"
                    ? t("设备码已过期")
                    : msStatus === "denied"
                      ? t("授权被拒绝")
                      : msStatus === "authorized"
                        ? t("授权成功")
                        : t("登录出错")}
            </p>
            {msError ? <p className="form-error">{msError}</p> : null}
          </>
        ) : (
          <p className="muted">{t("正在获取设备码…")}</p>
        )}
      </Modal>

      <Modal
        open={open}
        onClose={() => setOpen(false)}
        title={t("添加离线档案")}
        subtitle={t("仅用于本地游戏，不会连接 Mojang 验证。")}
        wide
        footer={
          <>
            <Button variant="ghost" onClick={() => setOpen(false)}>
              {t("取消")}</Button>
            <Button variant="primary" loading={busy} onClick={add}>
              {t("创建")}</Button>
          </>
        }
      >
        <Field label={t("用户名")} hint={t("3-16 位字母、数字或下划线")} error={error} htmlFor="offline-name">
          <input
            id="offline-name"
            value={username}
            maxLength={16}
            onChange={(e) => setUsername(e.target.value)}
            placeholder="Steve"
            onKeyDown={(e) => {
              if (e.key === "Enter") void add();
            }}
          />
        </Field>
        <SkinPicker value={selection} onChange={setSelection} disabled={busy} />
        <div className="notice notice--warning">
          <Icon name="alert" size={16} />
          <p>{t("该档案的 UUID 由用户名本地生成，与其他使用相同用户名的离线启动器一致，但不代表账号所有权。")}</p>
        </div>
      </Modal>

      <Modal
        open={editingSkin !== null}
        onClose={() => setEditingSkin(null)}
        title={editingSkin ? t("皮肤 · {0}", [editingSkin.username]) : t("皮肤")}
        subtitle={t("正版账号可上传到官方皮肤服务；离线档案仅在本机生效。")}
        wide
        footer={
          <>
            <Button variant="ghost" onClick={() => setEditingSkin(null)}>
              {t("取消")}</Button>
            <Button variant="primary" loading={busy} onClick={() => void saveSkin()}>
              {t("保存")}</Button>
          </>
        }
      >
        <SkinPicker value={editSelection} onChange={setEditSelection} disabled={busy} />
      </Modal>

      <Modal
        open={removing !== null}
        onClose={() => setRemoving(null)}
        title={t("移除账号档案")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setRemoving(null)}>
              {t("取消")}</Button>
            <Button variant="danger" loading={busy} onClick={() => removing && remove(removing)}>
              {t("移除")}</Button>
          </>
        }
      >
        <p>{t("仅移除启动器中的档案记录，不会删除任何游戏文件。")}</p>
      </Modal>
    </div>
  );
}
