import { useEffect, useMemo, useState } from "react";
import { api, onEvent } from "../lib/api";
import type {
  MultiplayerEdition,
  MultiplayerLocalService,
  MultiplayerNode,
  MultiplayerOrphan,
  MultiplayerProgress,
  MultiplayerRoom,
  MultiplayerStatus,
} from "../lib/api";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, Field, InfoHint, Notice } from "../components/ui";
import { Modal } from "../components/Modal";
import { MultiplayerHelp } from "../components/MultiplayerHelp";
import { Icon } from "../components/Icon";
import { t } from "../lib/i18n";


const CREATE_STEPS: { step: string; label: string }[] = [
  { step: "auth", label: t("检查 52FRP 账户") },
  { step: "local", label: t("检查本地 Minecraft 服务") },
  { step: "nodes", label: t("获取可用节点") },
  { step: "node", label: t("选择合适节点") },
  { step: "quota", label: t("检查隧道额度") },
  { step: "port", label: t("获取远程端口") },
  { step: "tunnel", label: t("创建隧道") },
  { step: "config", label: t("获取隧道配置") },
  { step: "frpc", label: t("下载并启动 FRPC") },
  { step: "verify", label: t("等待 52FRP 云端确认") },
];

function editionLabel(edition: MultiplayerEdition) {
  return edition === "java" ? "Java Edition" : "Bedrock Edition";
}

function address(room: MultiplayerRoom) {
  const host =
    room.publicHost.includes(":") && !room.publicHost.startsWith("[")
      ? `[${room.publicHost}]`
      : room.publicHost;
  return `${host}:${room.publicPort}`;
}

function stateTone(room: MultiplayerRoom): "success" | "warning" | "neutral" {
  if (room.state === "online") return "success";
  if (room.state === "retained" || room.state === "local_stopped") return "warning";
  return "neutral";
}

function roomStateLabel(room: MultiplayerRoom) {
  if (room.state === "online") return t("联机在线");
  if (room.state === "retained") return t("隧道保留");
  if (room.state === "local_stopped") return t("本地游戏已退出");
  return t("FRPC 离线");
}

type ConfirmAction = {
  title: string;
  message: string;
  action: string;
  danger?: boolean;
  run: () => Promise<void>;
};

export function MultiplayerPage() {
  const { bootstrap, setPage, pushToast } = useApp();
  const instances = bootstrap?.instances ?? [];
  const [status, setStatus] = useState<MultiplayerStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [edition, setEdition] = useState<MultiplayerEdition>("java");
  const [instanceId, setInstanceId] = useState(() => instances[0]?.id ?? "");
  const [localIp, setLocalIp] = useState("127.0.0.1");
  const [localPort, setLocalPort] = useState("");
  const [localService, setLocalService] = useState<MultiplayerLocalService | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [nodes, setNodes] = useState<MultiplayerNode[]>([]);
  const [nodesLoading, setNodesLoading] = useState(false);
  const [nodeId, setNodeId] = useState<number | "auto">("auto");
  const [steps, setSteps] = useState<MultiplayerProgress[]>([]);
  const [creating, setCreating] = useState(false);
  const [orphans, setOrphans] = useState<MultiplayerOrphan[]>([]);
  const [copied, setCopied] = useState(false);
  const [token, setToken] = useState("");
  const [confirmAction, setConfirmAction] = useState<ConfirmAction | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);

  const instance = useMemo(
    () => instances.find((item) => item.id === instanceId) ?? instances[0],
    [instances, instanceId],
  );
  const room = status?.room ?? null;
  const online = room?.state === "online";
  const visibleOrphans = orphans.filter((orphan) => orphan.tunnelId !== room?.tunnelId);

  async function loadStatus() {
    try {
      const next = await api.multiplayerStatus();
      setStatus(next);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  async function loadOrphans() {
    try {
      setOrphans(await api.multiplayerOrphans());
    } catch {
      setOrphans([]);
    }
  }

  useEffect(() => {
    void loadStatus();
    const timer = window.setInterval(() => void loadStatus(), 15000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!instanceId && instances[0]) setInstanceId(instances[0].id);
  }, [instances, instanceId]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onEvent<MultiplayerProgress>("multiplayer://progress", (event) => {
      if (disposed) return;
      setSteps((prev) => {
        const index = prev.findIndex((step) => step.step === event.step);
        if (index < 0) return [...prev, event];
        const next = [...prev];
        next[index] = event;
        return next;
      });
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!instance) return;
    let disposed = false;
    setDetecting(true);
    void api
      .multiplayerDetect(instance.id, edition)
      .then((result) => {
        if (disposed) return;
        setLocalService(result);
        if (result.detectedPort) setLocalPort(String(result.detectedPort));
      })
      .catch(() => {
        if (!disposed) setLocalService(null);
      })
      .finally(() => {
        if (!disposed) setDetecting(false);
      });
    return () => {
      disposed = true;
    };
  }, [instance?.id, edition]);

  useEffect(() => {
    if (!status?.loggedIn) {
      setNodes([]);
      return;
    }
    let disposed = false;
    setNodesLoading(true);
    void api
      .multiplayerNodes(edition)
      .then((result) => {
        if (disposed) return;
        setNodes(result);
        setNodeId((current) => {
          if (current === "auto") return current;
          return result.some((node) => node.id === current && node.available) ? current : "auto";
        });
      })
      .catch(() => {
        if (!disposed) setNodes([]);
      })
      .finally(() => {
        if (!disposed) setNodesLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [status?.loggedIn, edition]);

  useEffect(() => {
    if (status?.loggedIn) void loadOrphans();
    else setOrphans([]);
  }, [status?.loggedIn]);

  async function connect() {
    if (!token.trim()) return;
    setBusy("connect");
    setError(null);
    try {
      await api.multiplayerConnect(token.trim());
      setToken("");
      pushToast(t("已连接 52FRP"), "success");
      await loadStatus();
      await loadOrphans();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function logout() {
    setBusy("logout");
    try {
      await api.multiplayerLogout();
      pushToast(t("已退出 52FRP 账户"), "info");
      await loadStatus();
      setOrphans([]);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function detect() {
    if (!instance) return;
    setDetecting(true);
    try {
      const result = await api.multiplayerDetect(instance.id, edition);
      setLocalService(result);
      if (result.detectedPort) setLocalPort(String(result.detectedPort));
      pushToast(result.message, result.found ? "success" : "warning");
    } catch (e) {
      pushToast(t("检测本地服务失败"), "error", e instanceof Error ? e.message : undefined);
    } finally {
      setDetecting(false);
    }
  }

  async function createRoom() {
    if (!instance) {
      pushToast(t("请先选择一个游戏实例"), "warning");
      setPage("instances");
      return;
    }
    setCreating(true);
    setError(null);
    setSteps(CREATE_STEPS.map((step) => ({ ...step, status: "pending", detail: null })));
    try {
      const room = await api.multiplayerCreateRoom({
        instanceId: instance.id,
        edition,
        localIp: localIp.trim() || "127.0.0.1",
        localPort: localPort.trim() ? Number(localPort) : null,
        nodeId: nodeId === "auto" ? null : nodeId,
        autoNode: nodeId === "auto",
      });
      setSteps((prev) =>
        prev.map((step) =>
          step.step === "verify" ? { ...step, status: "ok", detail: null } : step,
        ),
      );
      pushToast(t("联机房间已创建"), "success", address(room));
      await loadStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setSteps((prev) =>
        prev.map((step) =>
          step.status === "running" ? { ...step, status: "error", detail: null } : step,
        ),
      );
    } finally {
      setCreating(false);
    }
  }

  async function stopRoom(deleteTunnel: boolean) {
    setBusy(deleteTunnel ? "stop-delete" : "stop");
    try {
      const result = await api.multiplayerStopRoom(deleteTunnel);
      pushToast(
        result.tunnelDeleted ? t("联机已停止，隧道已删除") : t("联机已停止"),
        result.tunnelDeleted ? "success" : "info",
        result.message,
      );
      await loadStatus();
      await loadOrphans();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function restartFrpc() {
    setBusy("restart");
    try {
      await api.multiplayerRestartFrpc();
      pushToast(t("FRPC 已重新启动"), "success");
      await loadStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function copyAddress() {
    if (!room) return;
    try {
      await navigator.clipboard.writeText(address(room));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
      pushToast(t("联机地址已复制"), "success", address(room));
    } catch {
      pushToast(t("复制失败"), "warning", t("当前环境不允许访问剪贴板"));
    }
  }

  async function deleteOrphan(tunnelId: number) {
    try {
      await api.multiplayerDeleteOrphan(tunnelId);
      pushToast(t("临时隧道已删除"), "success");
      await loadOrphans();
      await loadStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  function askLogout() {
    setConfirmAction({
      title: t("退出 52FRP 账户"),
      message: t("会清除本机保存的 API 访问令牌。当前房间需要先清理。"),
      action: t("退出账户"),
      danger: true,
      run: logout,
    });
  }

  function askStop(deleteTunnel: boolean) {
    setConfirmAction({
      title: deleteTunnel ? t("停止并删除临时隧道") : t("停止联机"),
      message: deleteTunnel
        ? t("停止 FRPC 并删除这次创建的 SRL 临时隧道。不会删除账户中的其他隧道。")
        : t("停止 FRPC？云端隧道会保留，下次进入联机中心时可以删除。"),
      action: deleteTunnel ? t("停止并删除") : t("停止联机"),
      danger: deleteTunnel,
      run: () => stopRoom(deleteTunnel),
    });
  }

  function askDeleteOrphan(tunnelId: number) {
    setConfirmAction({
      title: t("删除 SRL 临时隧道"),
      message: t("只删除这条由 SRL 创建的临时隧道，其他隧道不会受影响。"),
      action: t("删除"),
      danger: true,
      run: () => deleteOrphan(tunnelId),
    });
  }

  function openFrp() {
    void api
      .openTrustedUrl("https://www.52frp.com/user/#/user-center")
      .catch((e) =>
        pushToast(t("无法打开 52FRP 个人中心"), "warning", e instanceof Error ? e.message : undefined),
      );
  }

  if (loading) {
    return (
      <div className="page">
        <Card>
          <p className="muted">{t("正在读取联机状态…")}</p>
        </Card>
      </div>
    );
  }

  if (!status) {
    return (
      <div className="page">
        <Card>
          <h3>{t("无法读取联机状态")}</h3>
          <p className="muted">{error ?? t("请稍后重试。")}</p>
          <div className="toolbar__actions">
            <Button size="sm" variant="primary" onClick={() => void loadStatus()}>
              {t("重试")}</Button>
            <Button size="sm" variant="ghost" onClick={() => setPage("settings")}>
              {t("打开设置")}</Button>
          </div>
        </Card>
      </div>
    );
  }

  return (
    <div className="page multiplayer-page">
      <div className="mp-toolbar">
        <div className="mp-toolbar__title">
          <h2>{t("Minecraft 联机")}</h2>
        </div>
        <div className="mp-toolbar__status">
          <Badge tone={status.loggedIn ? "success" : "neutral"}>
            {status.providerName ?? "52FRP"}
          </Badge>
          <Badge tone={online ? "success" : "neutral"}>
            {online ? t("联机在线") : status.loggedIn ? t("未建房") : t("未连接")}
          </Badge>
        </div>
      </div>

      {error ? <Notice tone="danger" title={t("操作失败")}>{error}</Notice> : null}
      {status.lastError && status.lastError !== error ? (
        <Notice tone="warning" title={t("最近一次连接状态")}>{status.lastError}</Notice>
      ) : null}
      {!status.tokenConfigured ? (
        <Notice tone="warning" title={t("先连接 52FRP")}>
          {t("还没有连接 52FRP，先在下方粘贴 API 令牌。")}</Notice>
      ) : null}

      <div className="mp-layout">
        <div className="mp-main">
          {status.loggedIn ? (
            <Card className="mp-account">
              <div className="mp-account__main">
                <span className="mp-account__avatar" aria-hidden="true">
                  <Icon name="user" size={18} />
                </span>
                <div>
                  <div className="mp-account__name">
                    <strong>{status.account?.displayName ?? t("52FRP 账户")}</strong>
                  </div>
                  <span className="muted small">
                    {status.account?.tunnelLimit != null
                      ? t("隧道 {0}/{1}", [status.account.tunnelCount ?? 0, status.account.tunnelLimit])
                      : t("已连接")}
                  </span>
                </div>
              </div>
              <div className="mp-account__actions">
                <Badge tone="success">{t("已连接")}</Badge>
                <Button
                  size="sm"
                  variant="ghost"
                  loading={busy === "logout"}
                  onClick={askLogout}
                >
                  {t("退出")}</Button>
              </div>
            </Card>
          ) : null}

          {room ? (
            <RoomCard
              room={room}
              online={online}
              copied={copied}
              busy={busy}
              onCopy={() => void copyAddress()}
              onStop={() => askStop(false)}
              onStopDelete={() => askStop(true)}
              onRestart={() => void restartFrpc()}
              frpcLog={status.frpc.logTail}
            />
          ) : (
            <Card className="mp-create">
              <div className="card-head">
                <div className="mp-card-title">
                  <h3>{t("创建房间")}</h3>
                </div>
                <Badge tone={status.loggedIn ? "success" : "warning"}>
                  {status.loggedIn ? t("可以开始") : t("需要令牌")}
                </Badge>
              </div>

              {!status.loggedIn ? (
                <div className="mp-login-block">
                  <div className="mp-label-row">
                    <span className="field__label">{t("52FRP API 令牌")}</span>
                    <InfoHint text={t("在 52FRP 头像 → API 访问里创建，权限要勾选“创建和修改隧道与客户端”。令牌只存系统凭据库。")} />
                  </div>
                  <div className="mp-token-row">
                    <input
                      type="password"
                      className="mono"
                      value={token}
                      onChange={(event) => setToken(event.target.value)}
                      placeholder="hz_..."
                      autoComplete="off"
                      onKeyDown={(event) => {
                        if (event.key === "Enter") void connect();
                      }}
                    />
                    <Button
                      variant="primary"
                      loading={busy === "connect"}
                      disabled={!token.trim()}
                      onClick={() => void connect()}
                    >
                      {t("连接")}</Button>
                  </div>
                  <div className="mp-login-links">
                    <Button size="sm" variant="ghost" onClick={openFrp}>
                      {t("打开 52FRP")}</Button>
                    <Button size="sm" variant="ghost" onClick={() => setHelpOpen(true)}>
                      {t("怎么获取令牌？")}</Button>
                  </div>
                </div>
              ) : (
                <>
                  <div className="mp-edition">
                    <button
                      type="button"
                      className={`segmented__item ${edition === "java" ? "segmented__item--active" : ""}`}
                      onClick={() => setEdition("java")}
                    >
                      {t("Java 版")}</button>
                    <button
                      type="button"
                      className={`segmented__item ${edition === "bedrock" ? "segmented__item--active" : ""}`}
                      onClick={() => setEdition("bedrock")}
                    >
                      {t("基岩版")}</button>

                  </div>

                  <div className="form-grid">
                    <Field label={t("游戏实例")}>
                      <select
                        value={instance?.id ?? ""}
                        onChange={(event) => setInstanceId(event.target.value)}
                        disabled={instances.length === 0 || creating}
                      >
                        {instances.length === 0 ? <option value="">{t("还没有游戏实例")}</option> : null}
                        {instances.map((item) => (
                          <option key={item.id} value={item.id}>
                            {item.name} · {item.versionId}
                          </option>
                        ))}
                      </select>
                    </Field>
                    <Field
                      label={t("节点")}
                      info={t("节点是 52FRP 的中转服务器。自动选择会挑延迟低、负载低的可用节点。")}
                    >
                      <select
                        value={nodeId}
                        onChange={(event) =>
                          setNodeId(event.target.value === "auto" ? "auto" : Number(event.target.value))
                        }
                        disabled={nodesLoading || creating}
                      >
                        <option value="auto">{t("自动选择")}</option>
                        {nodes.map((node) => (
                          <option key={node.id} value={node.id} disabled={!node.available}>
                            {node.name}
                            {node.latencyMs != null ? ` · ${node.latencyMs} ms` : ""}
                            {!node.available ? ` · ${node.reason ?? t("不可用")}` : ""}
                          </option>
                        ))}
                      </select>
                    </Field>
                  </div>

                  <div className="mp-local-row">
                    <Field label={t("本地 IP")}>
                      <input
                        className="mono"
                        value={localIp}
                        onChange={(event) => setLocalIp(event.target.value)}
                        disabled={creating}
                      />
                    </Field>
                    <Field
                      label={t("本地 {0} 端口", [edition === "java" ? "TCP" : "UDP"])}
                      info={localService?.message ?? t("Minecraft 实际监听的端口；开放局域网后一般能自动检测。")}
                    >
                      <div className="mp-port-row">
                        <input
                          className="mono"
                          inputMode="numeric"
                          value={localPort}
                          onChange={(event) => setLocalPort(event.target.value.replace(/[^0-9]/g, ""))}
                          placeholder={edition === "java" ? t("25565 或局域网动态端口") : "19132"}
                          disabled={creating}
                        />
                        <Button
                          size="sm"
                          variant="ghost"
                          loading={detecting}
                          onClick={() => void detect()}
                        >
                          {t("检测")}</Button>
                      </div>
                    </Field>
                  </div>

                  <div className="mp-create-actions">
                    <Button
                      variant="primary"
                      icon="play"
                      loading={creating}
                      disabled={!status.loggedIn || !instance || creating || !localPort.trim()}
                      onClick={() => void createRoom()}
                    >
                      {t("创建房间")}</Button>
                    <span className="muted small">
                      {instance ? `${instance.name} · ${instance.versionId}` : t("请先创建游戏实例")}
                    </span>
                  </div>
                </>
              )}
            </Card>
          )}

          {steps.length > 0 && (creating || steps.some((step) => step.status === "error")) ? (
            <StepList steps={steps} />
          ) : null}

          {visibleOrphans.length > 0 ? (
            <Card>
              <div className="card-head">
                <div className="mp-card-title">
                  <h3>{t("待清理隧道")}</h3>
                </div>
                <Badge tone="warning">{visibleOrphans.length} {t("条")}</Badge>
              </div>
              <div className="mp-orphans">
                {visibleOrphans.map((orphan) => (
                  <div className="mp-orphan" key={orphan.tunnelId}>
                    <div>
                      <strong>{orphan.name}</strong>
                      <span className="muted small">
                        {orphan.tunnelType} {t("· 本地端口")}{orphan.localPort}
                        {orphan.nodeName ? ` · ${orphan.nodeName}` : ""}
                      </span>
                    </div>
                    <Button size="sm" variant="ghost" onClick={() => askDeleteOrphan(orphan.tunnelId)}>
                      {t("删除")}</Button>
                  </div>
                ))}
              </div>
            </Card>
          ) : null}
        </div>

        <div className="mp-side">
          <Card>
            <div className="card-head">
              <div className="mp-card-title">
                <h3>{t("当前状态")}</h3>
              </div>
              <Button size="sm" variant="ghost" icon="info" onClick={() => setHelpOpen(true)}>
                {t("帮助")}</Button>
            </div>
            <dl className="mp-facts">
              <div>
                <dt>
                  {t("账户")}</dt>
                <dd>
                  {status.account?.displayName ??
                    (status.loggedIn ? t("已连接") : t("未连接"))}
                </dd>
              </div>
              <div>
                <dt>
                  {t("游戏")}</dt>
                <dd>{instance ? instance.versionId : t("未选择")}</dd>
              </div>
              <div>
                <dt>
                  {t("本机端口")}</dt>
                <dd>
                  {detecting
                    ? t("检测中…")
                    : localService?.found
                      ? `${localService.detectedPort ?? "—"}`
                      : t("未检测到")}
                </dd>
              </div>
              <div>
                <dt>
                  FRPC
                  
                </dt>
                <dd>{status.frpc.running ? t("运行中") : t("未运行")}</dd>
              </div>
              <div>
                <dt>
                  {t("联机")}</dt>
                <dd>{online ? t("在线") : room?.state === "retained" ? t("隧道保留") : t("未联机")}</dd>
              </div>
            </dl>
            {status.loggedIn ? (
              <div className="mp-side-actions">
                <Button size="sm" variant="ghost" onClick={() => void loadStatus()}>
                  {t("刷新")}</Button>
                <Button size="sm" variant="ghost" loading={busy === "logout"} onClick={askLogout}>
                  {t("退出账户")}</Button>
              </div>
            ) : null}
          </Card>
        </div>
      </div>

      <Modal
        open={confirmAction !== null}
        onClose={() => setConfirmAction(null)}
        title={confirmAction?.title ?? ""}
        footer={
          confirmAction ? (
            <>
              <Button variant="ghost" onClick={() => setConfirmAction(null)}>
                {t("取消")}</Button>
              <Button
                variant={confirmAction.danger ? "danger" : "primary"}
                onClick={() => {
                  const action = confirmAction;
                  setConfirmAction(null);
                  if (action) void action.run();
                }}
              >
                {confirmAction.action}
              </Button>
            </>
          ) : null
        }
      >
        <p className="muted">{confirmAction?.message}</p>
      </Modal>

      <MultiplayerHelp
        open={helpOpen}
        onClose={() => setHelpOpen(false)}
        onOpenUrl={(url, label) =>
          void api
            .openTrustedUrl(url)
            .catch((e) =>
              pushToast(t("无法打开{0}", [label]), "warning", e instanceof Error ? e.message : undefined),
            )
        }
      />
    </div>
  );
}

function RoomCard({
  room,
  online,
  copied,
  busy,
  onCopy,
  onStop,
  onStopDelete,
  onRestart,
  frpcLog,
}: {
  room: MultiplayerRoom;
  online: boolean;
  copied: boolean;
  busy: string | null;
  onCopy: () => void;
  onStop: () => void;
  onStopDelete: () => void;
  onRestart: () => void;
  frpcLog: string[];
}) {
  return (
    <Card className="mp-room">
      <div className="mp-room__head">
        <div>
          <div className="mp-room__title">
            <h3>{room.instanceName}</h3>
            <Badge tone={stateTone(room)}>{roomStateLabel(room)}</Badge>
          </div>
          <p className="muted small">
            {editionLabel(room.edition)} · {room.versionId} · {room.nodeName}
          </p>
        </div>
      </div>

      <div className={`mp-address ${online ? "mp-address--online" : ""}`}>
        <div>
          <span>
            {t("公网地址")}</span>
          <strong className="mono">{address(room)}</strong>
        </div>
        <Button
          size="sm"
          variant={copied ? "primary" : "secondary"}
          icon={copied ? "check" : "copy"}
          onClick={onCopy}
        >
          {copied ? t("已复制") : t("复制")}
        </Button>
      </div>
      <p className="mp-room__hint">{t("朋友在“多人游戏 → 直接连接”里填这个地址。")}</p>

      <dl className="mp-room__stats">
        <div>
          <dt>
            {t("节点")}</dt>
          <dd>{room.nodeName}</dd>
        </div>
        <div>
          <dt>
            {t("本地端口")}</dt>
          <dd className="mono">{room.localPort}</dd>
        </div>
        <div>
          <dt>
            {t("延迟")}</dt>
          <dd>{room.latencyMs != null ? `${room.latencyMs} ms` : t("未测速")}</dd>
        </div>
      </dl>

      {room.message ? <p className="mp-room__message">{room.message}</p> : null}

      <details className="mp-room__advanced">
        <summary>{t("技术信息")}</summary>
        <dl className="mp-room__advanced-grid">
          <div>
            <dt>{t("公网端口")}</dt>
            <dd className="mono">{room.publicPort}</dd>
          </div>
          <div>
            <dt>FRPC</dt>
            <dd>{room.frpcRunning ? t("运行中") : t("已停止")}</dd>
          </div>
          <div>
            <dt>{t("本地进程")}</dt>
            <dd>{room.localPid ? `PID ${room.localPid}` : t("未记录")}</dd>
          </div>
        </dl>
        {frpcLog.length > 0 ? (
          <details className="mp-log">
            <summary>{t("FRPC 日志")}</summary>
            <pre>{frpcLog.join("\n")}</pre>
          </details>
        ) : null}
      </details>

      <div className="mp-room__actions">
        {room.state !== "retained" ? (
          <Button variant="secondary" icon="refresh" loading={busy === "restart"} onClick={onRestart}>
            {t("重启")}</Button>
        ) : null}
        {room.state !== "retained" ? (
          <Button variant="ghost" loading={busy === "stop"} onClick={onStop}>
            {t("停止")}</Button>
        ) : null}
        <Button variant="danger" icon="trash" loading={busy === "stop-delete"} onClick={onStopDelete}>
          {t("停止并删除隧道")}</Button>
      </div>
    </Card>
  );
}

function StepList({ steps }: { steps: MultiplayerProgress[] }) {
  return (
    <Card>
      <div className="card-head">
        <div>
          <h3>{t("创建过程")}</h3>
          <p>{t("每一步都对应真实的检查、API 调用或进程启动结果。")}</p>
        </div>
      </div>
      <ol className="mp-steps">
        {steps.map((step) => (
          <li key={step.step} className={`mp-step mp-step--${step.status}`}>
            <span className="mp-step__icon" aria-hidden="true">
              {step.status === "ok" ? (
                <Icon name="check" size={14} />
              ) : step.status === "error" ? (
                <Icon name="alert" size={14} />
              ) : step.status === "running" ? (
                <span className="spinner" />
              ) : (
                <span className="mp-step__dot" />
              )}
            </span>
            <div>
              <strong>{step.label}</strong>
              {step.detail ? <span className="muted small">{step.detail}</span> : null}
            </div>
            <span className="mp-step__state">{step.status === "ok" ? t("完成") : step.status === "error" ? t("失败") : step.status === "running" ? t("进行中") : t("等待")}</span>
          </li>
        ))}
      </ol>
    </Card>
  );
}
