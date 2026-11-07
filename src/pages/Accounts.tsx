import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type Account, type DeviceCodeView, type Skin } from "../lib/api";
import { formatDate } from "../lib/format";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, Field, IconButton, Notice } from "../components/ui";
import { Modal } from "../components/Modal";
import { Icon } from "../components/Icon";
import { SkinHead } from "../components/SkinPreview";
import { SkinPicker, type SkinSelection } from "../components/SkinPicker";

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
      pushToast("无法读取皮肤库", "warning", e instanceof Error ? e.message : undefined);
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
          pushToast("Microsoft 登录成功", "success", result.account?.username ?? "");
          setMsOpen(false);
          setMsDevice(null);
          await reload();
          return;
        }
        if (["expired", "denied", "error"].includes(result.status)) {
          setMsError(result.message ?? "登录未完成");
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
      setClientError("client id 应是 Azure 应用注册页显示的 GUID");
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
      pushToast("Microsoft client id 已保存，正版登录已启用", "success");
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
      pushToast("Microsoft 授权已刷新", "success", account.username);
    } catch (e) {
      pushToast(
        "刷新授权失败",
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
      pushToast("无法开始 Microsoft 登录", "error", e instanceof Error ? e.message : undefined);
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
      setError("用户名需为 3-16 位字母、数字或下划线");
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
      pushToast("离线档案已创建", "success", name);
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
      pushToast("已切换当前账号", "success");
    } catch (e) {
      pushToast("切换失败", "error", e instanceof Error ? e.message : String(e));
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
      pushToast("账号档案已移除", "info");
    } catch (e) {
      pushToast("移除失败", "error", e instanceof Error ? e.message : String(e));
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
      pushToast("皮肤设置已保存", "success");
    } catch (e) {
      pushToast("保存皮肤失败", "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function applyOfficialSkin(account: Account) {
    setApplyingSkin(account.id);
    try {
      const result = await api.applyAccountSkin(account.id);
      pushToast("皮肤已应用到正版账号", "success", result.message);
    } catch (e) {
      pushToast("皮肤上传失败", "error", e instanceof Error ? e.message : String(e));
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
      pushToast("皮肤已从皮肤库删除", "info");
    } catch (e) {
      pushToast("删除皮肤失败", "error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="page accounts-page">
      <div className="toolbar">
        <div>
          <h2>账号管理</h2>
        </div>
        <Button variant="primary" icon="plus" onClick={openAdd}>
          添加离线档案
        </Button>
      </div>

      {accounts.length === 0 ? (
        <Card>
          <EmptyState
            icon="user"
            title="还没有账号档案"
            description="添加一个离线档案即可开始。"
            action={
              <Button variant="primary" icon="plus" onClick={openAdd}>
                添加离线档案
              </Button>
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
                        {a.kind === "offline" ? "离线档案" : "Microsoft"}
                      </Badge>
                      {a.active ? <Badge tone="success">当前</Badge> : null}
                      {skin ? (
                        <Badge tone="accent">
                          {skin.name} · {model === "slim" ? "纤细" : "标准"}
                        </Badge>
                      ) : null}
                    </div>
                    <code className="mono small">{a.uuid}</code>
                    <p className="muted small">{a.note}</p>
                    <span className="muted small">创建于 {formatDate(a.createdAt)}</span>
                  </div>
                </div>
                <div className="account-card__actions">
                  <Button size="sm" icon="edit" onClick={() => openSkinEditor(a)} disabled={busy}>
                    {skin ? "更换皮肤" : "设置皮肤"}
                  </Button>
                  {a.kind === "microsoft" && skin ? (
                    <Button
                      size="sm"
                      icon="download"
                      loading={applyingSkin === a.id}
                      onClick={() => void applyOfficialSkin(a)}
                    >
                      上传皮肤
                    </Button>
                  ) : null}
                  {a.kind === "microsoft" ? (
                    <Button
                      size="sm"
                      icon="refresh"
                      loading={refreshing === a.id}
                      onClick={() => void refreshMicrosoft(a)}
                    >
                      刷新授权
                    </Button>
                  ) : null}
                  <Button size="sm" onClick={() => activate(a.id)} disabled={a.active || busy}>
                    {a.active ? "使用中" : "设为当前"}
                  </Button>
                  <Button size="sm" variant="ghost" icon="trash" onClick={() => setRemoving(a.id)}>
                    移除
                  </Button>
                </div>
              </Card>
            );
          })}
        </div>
      )}

      <Card className="skin-library">
        <div className="card-head">
          <div>
            <h3>我的皮肤</h3>
            <p>本地导入与正版玩家皮肤保存在启动器数据目录，不会自动上传。</p>
          </div>
          <Badge tone={mySkins.length > 0 ? "info" : "neutral"}>{mySkins.length} 个</Badge>
        </div>
        {skinsLoading ? (
          <p className="muted small">正在读取皮肤库…</p>
        ) : mySkins.length === 0 ? (
          <p className="muted small">
            还没有保存的皮肤。创建账号或点击“设置皮肤”即可导入本地 PNG 或获取正版玩家皮肤。
          </p>
        ) : (
          <div className="skin-library-grid">
            {mySkins.map((skin) => (
              <div key={skin.id} className="skin-library-item">
                <SkinHead src={skin.preview} size={38} />
                <div className="skin-library-item__info">
                  <strong>{skin.name}</strong>
                  <span className="muted small">
                    {skin.source === "mojang" ? "正版玩家" : "本地"} ·{" "}
                    {skin.model === "slim" ? "纤细" : "标准"} · {skin.width}×{skin.height}
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
                      删除
                    </Button>
                    <Button size="sm" variant="ghost" onClick={() => setRemovingSkin(null)}>
                      取消
                    </Button>
                  </span>
                ) : (
                  <IconButton
                    icon="trash"
                    label={`删除皮肤 ${skin.name}`}
                    onClick={() => setRemovingSkin(skin.id)}
                  />
                )}
              </div>
            ))}
          </div>
        )}
      </Card>

      <Notice tone="warning" title="Microsoft 正版登录暂时需要自行配置">
        因条件有限，目前需要自行注册 Azure 应用并获取 client ID 才能登录；后续会修复为开箱即用，敬请期待。
      </Notice>

      <Card className="microsoft-card">
        <div className="card-head">
          <div>
            <h3>Microsoft 正版登录</h3>
          </div>
          <Badge tone={microsoft?.available ? "success" : "warning"}>
            {microsoft?.available ? "已配置" : "未配置"}
          </Badge>
        </div>
        {!microsoft?.available || editingClient ? (
          <div className="microsoft-setup">
            <Field
              label="Azure 应用 client id"
              hint="只填公开的 Application (client) ID，不是密钥。设备码流程无需密码或重定向 URI。"
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
                保存并启用
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() =>
                  void api
                    .openTrustedUrl("https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade")
                    .catch((e) =>
                      pushToast(
                        "无法打开 Azure 门户",
                        "warning",
                        e instanceof Error ? e.message : undefined,
                      ),
                    )
                }
              >
                打开 Azure 应用注册
              </Button>
            </div>
            <details className="microsoft-guide">
              <summary>如何获取 client id</summary>
              <ol>
                <li>打开 Azure 门户 → Microsoft Entra ID → 应用注册 → 新注册。</li>
                <li>名称随意；账户类型选“个人 Microsoft 账户”或“任何组织目录中的账户和个人 Microsoft 账户”（必须是后者之一，否则 /consumers 端点不适用）。</li>
                <li>注册后复制“应用程序(客户端) ID”。</li>
                <li>在“身份验证”中启用“允许公共客户端流”，设备码流程才能使用。</li>
                <li>不需要客户端密钥；不要把密钥填到这里。</li>
              </ol>
            </details>
          </div>
        ) : (
          <div className="microsoft-card__actions">
            <Button onClick={startMicrosoft} loading={msStarting}>
              使用 Microsoft 登录
            </Button>
            <span className="muted small">设备码流程 · 不收集密码</span>
            <Button size="sm" variant="ghost" onClick={() => setEditingClient(true)}>
              更换 client id
            </Button>
          </div>
        )}
      </Card>

      <Modal
        open={msOpen}
        onClose={() => {
          setMsOpen(false);
          setMsDevice(null);
        }}
        title="Microsoft 登录"
        subtitle="在浏览器中完成授权后，这里会自动继续。"
        footer={
          <Button
            variant="ghost"
            onClick={() => {
              setMsOpen(false);
              setMsDevice(null);
            }}
          >
            取消登录
          </Button>
        }
      >
        {msDevice ? (
          <>
            <div className="device-code-box">
              <span>在浏览器打开</span>
              <div className="device-code-row">
                <code>{msDevice.verificationUri}</code>
                <Button
                  size="sm"
                  variant="ghost"
                  icon="copy"
                  onClick={() => navigator.clipboard.writeText(msDevice.verificationUri)}
                >
                  复制
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  icon="folder"
                  onClick={() =>
                    void api
                      .openTrustedUrl(msDevice.verificationUri)
                      .catch((e) =>
                        pushToast(
                          "无法打开浏览器",
                          "warning",
                          e instanceof Error ? e.message : undefined,
                        ),
                      )
                  }
                >
                  打开
                </Button>
              </div>
              <span>并输入代码</span>
              <div className="device-code-row">
                <strong className="device-code">{msDevice.userCode}</strong>
                <Button
                  size="sm"
                  variant="ghost"
                  icon="copy"
                  onClick={() => navigator.clipboard.writeText(msDevice.userCode)}
                >
                  复制
                </Button>
              </div>
            </div>
            <p className="muted small">{msDevice.message}</p>
            <p className="device-status">
              {msStatus === "pending"
                ? "等待授权…"
                : msStatus === "slowDown"
                  ? "等待授权（已降低轮询频率）…"
                  : msStatus === "expired"
                    ? "设备码已过期"
                    : msStatus === "denied"
                      ? "授权被拒绝"
                      : msStatus === "authorized"
                        ? "授权成功"
                        : "登录出错"}
            </p>
            {msError ? <p className="form-error">{msError}</p> : null}
          </>
        ) : (
          <p className="muted">正在获取设备码…</p>
        )}
      </Modal>

      <Modal
        open={open}
        onClose={() => setOpen(false)}
        title="添加离线档案"
        subtitle="仅用于本地游戏，不会连接 Mojang 验证。"
        wide
        footer={
          <>
            <Button variant="ghost" onClick={() => setOpen(false)}>
              取消
            </Button>
            <Button variant="primary" loading={busy} onClick={add}>
              创建
            </Button>
          </>
        }
      >
        <Field label="用户名" hint="3-16 位字母、数字或下划线" error={error} htmlFor="offline-name">
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
          <p>该档案的 UUID 由用户名本地生成，与其他使用相同用户名的离线启动器一致，但不代表账号所有权。</p>
        </div>
      </Modal>

      <Modal
        open={editingSkin !== null}
        onClose={() => setEditingSkin(null)}
        title={editingSkin ? `皮肤 · ${editingSkin.username}` : "皮肤"}
        subtitle="正版账号可上传到官方皮肤服务；离线档案仅在本机生效。"
        wide
        footer={
          <>
            <Button variant="ghost" onClick={() => setEditingSkin(null)}>
              取消
            </Button>
            <Button variant="primary" loading={busy} onClick={() => void saveSkin()}>
              保存
            </Button>
          </>
        }
      >
        <SkinPicker value={editSelection} onChange={setEditSelection} disabled={busy} />
      </Modal>

      <Modal
        open={removing !== null}
        onClose={() => setRemoving(null)}
        title="移除账号档案"
        footer={
          <>
            <Button variant="ghost" onClick={() => setRemoving(null)}>
              取消
            </Button>
            <Button variant="danger" loading={busy} onClick={() => removing && remove(removing)}>
              移除
            </Button>
          </>
        }
      >
        <p>仅移除启动器中的档案记录，不会删除任何游戏文件。</p>
      </Modal>
    </div>
  );
}
