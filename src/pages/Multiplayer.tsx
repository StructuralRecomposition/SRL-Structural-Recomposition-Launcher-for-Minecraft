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
import { Badge, Button, Card, Field, Notice } from "../components/ui";
import { Icon } from "../components/Icon";

const CREATE_STEPS: { step: string; label: string }[] = [
  { step: "auth", label: "检查 52FRP 账户" },
  { step: "local", label: "检查本地 Minecraft 服务" },
  { step: "nodes", label: "获取可用节点" },
  { step: "node", label: "选择合适节点" },
  { step: "quota", label: "检查隧道额度" },
  { step: "port", label: "获取远程端口" },
  { step: "tunnel", label: "创建隧道" },
  { step: "config", label: "获取隧道配置" },
  { step: "frpc", label: "下载并启动 FRPC" },
  { step: "verify", label: "验证公网连接" },
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
  if (room.state === "online") return "联机在线";
  if (room.state === "retained") return "隧道保留";
  if (room.state === "local_stopped") return "本地游戏已退出";
  return "FRPC 离线";
}

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
      pushToast("已连接 52FRP", "success");
      await loadStatus();
      await loadOrphans();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }

  async function logout() {
    if (!window.confirm("退出 52FRP 账户会清除本机保存的 API 访问令牌。继续？")) return;
    setBusy("logout");
    try {
      await api.multiplayerLogout();
      pushToast("已退出 52FRP 账户", "info");
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
      pushToast("检测本地服务失败", "error", e instanceof Error ? e.message : undefined);
    } finally {
      setDetecting(false);
    }
  }

  async function createRoom() {
    if (!instance) {
      pushToast("请先选择一个游戏实例", "warning");
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
      pushToast("联机房间已创建", "success", address(room));
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
    const message = deleteTunnel
      ? "停止 FRPC 并删除这次创建的临时隧道？不会删除账户中的其他隧道。"
      : "停止 FRPC？云端隧道会保留，下次进入联机中心时可以删除。";
    if (!window.confirm(message)) return;
    setBusy(deleteTunnel ? "stop-delete" : "stop");
    try {
      const result = await api.multiplayerStopRoom(deleteTunnel);
      pushToast(
        result.tunnelDeleted ? "联机已停止，隧道已删除" : "联机已停止",
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
      pushToast("FRPC 已重新启动", "success");
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
      pushToast("联机地址已复制", "success", address(room));
    } catch {
      pushToast("复制失败", "warning", "当前环境不允许访问剪贴板");
    }
  }

  async function deleteOrphan(tunnelId: number) {
    if (!window.confirm("只删除这条由 SRL 创建的临时隧道？其他隧道不会受影响。")) return;
    try {
      await api.multiplayerDeleteOrphan(tunnelId);
      pushToast("临时隧道已删除", "success");
      await loadOrphans();
      await loadStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  if (loading) {
    return (
      <div className="page">
        <Card>
          <p className="muted">正在读取联机状态…</p>
        </Card>
      </div>
    );
  }

  if (!status) {
    return (
      <div className="page">
        <Card>
          <h3>无法读取联机状态</h3>
          <p className="muted">{error ?? "请稍后重试。"}</p>
          <div className="toolbar__actions">
            <Button size="sm" variant="primary" onClick={() => void loadStatus()}>
              重试
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setPage("settings")}>
              打开设置
            </Button>
          </div>
        </Card>
      </div>
    );
  }

  return (
    <div className="page multiplayer-page">
      <div className="mp-toolbar">
        <div>
          <span className="eyebrow">SRL Multiplayer</span>
          <h2>Minecraft 联机</h2>
          <p>创建房间、建立隧道、复制地址。公网 IP、端口映射和 FRP 都由 SRL 处理。</p>
        </div>
        <div className="mp-toolbar__status">
          <Badge tone={status?.loggedIn ? "success" : "neutral"}>
            {status?.providerName ?? "52FRP"}
          </Badge>
          <Badge tone={online ? "success" : "neutral"}>{online ? "联机在线" : "未联机"}</Badge>
        </div>
      </div>

      {error ? <Notice tone="danger" title="操作失败">{error}</Notice> : null}
      {status?.lastError && status.lastError !== error ? (
        <Notice tone="warning" title="最近一次连接状态">{status.lastError}</Notice>
      ) : null}

      {!status?.tokenConfigured ? (
        <Notice tone="warning" title="先连接 52FRP">
          在 52FRP 个人中心创建 API 访问令牌，然后粘贴到下方。令牌只保存在系统凭据库，不会写入日志或配置文件。
          <Button
            size="sm"
            variant="primary"
            className="mp-notice-action"
            onClick={() =>
              void api
                .openTrustedUrl("https://www.52frp.com/user/#/user-center")
                .catch((e) =>
                  pushToast("无法打开 52FRP 个人中心", "warning", e instanceof Error ? e.message : undefined),
                )
            }
          >
            打开令牌页面
          </Button>
        </Notice>
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
                  <strong>{status.account?.displayName ?? "52FRP 账户"}</strong>
                  <span className="muted small">
                    {status.account
                      ? `用户 ID ${status.account.userId}${status.account.group ? ` · ${status.account.group}` : ""}`
                      : "已连接，账户信息读取失败"}
                    {status.account?.tunnelLimit != null
                      ? ` · 隧道 ${status.account.tunnelCount ?? 0}/${status.account.tunnelLimit}`
                      : ""}
                  </span>
                </div>
              </div>
              <div className="mp-account__actions">
                <Badge tone="success">账户已授权</Badge>
                <Button
                  size="sm"
                  variant="ghost"
                  loading={busy === "logout"}
                  onClick={() => void logout()}
                >
                  退出账户
                </Button>
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
              onStop={() => void stopRoom(false)}
              onStopDelete={() => void stopRoom(true)}
              onRestart={() => void restartFrpc()}
              frpcLog={status.frpc.logTail}
            />
          ) : (
            <Card className="mp-create">
              <div className="card-head">
                <div>
                  <h3>创建联机房间</h3>
                  <p>先确认游戏已经在运行，并在游戏内开放局域网。</p>
                </div>
                <Badge tone={status?.loggedIn ? "success" : "warning"}>
                  {status?.loggedIn ? "账户已连接" : "需要连接账户"}
                </Badge>
              </div>

              {!status?.loggedIn ? (
                <div className="mp-login-block">
                  <p className="muted">
                    在 52FRP 个人中心创建 API 访问令牌（以 hz_ 开头），粘贴到这里。SRL 只会把令牌保存到系统凭据库。
                  </p>
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
                      连接 52FRP
                    </Button>
                  </div>
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() =>
                      void api
                        .openTrustedUrl("https://www.52frp.com/user/#/user-center")
                        .catch((e) =>
                          pushToast("无法打开 52FRP 个人中心", "warning", e instanceof Error ? e.message : undefined),
                        )
                    }
                  >
                    没有令牌？打开 52FRP 个人中心
                  </Button>
                </div>
              ) : (
                <>
                  <div className="mp-edition">
                    <button
                      type="button"
                      className={`segmented__item ${edition === "java" ? "segmented__item--active" : ""}`}
                      onClick={() => setEdition("java")}
                    >
                      Java Edition · TCP
                    </button>
                    <button
                      type="button"
                      className={`segmented__item ${edition === "bedrock" ? "segmented__item--active" : ""}`}
                      onClick={() => setEdition("bedrock")}
                    >
                      Bedrock Edition · UDP
                    </button>
                  </div>

                  <div className="form-grid">
                    <Field label="游戏实例" hint="使用该实例的版本与日志检测本地服务。">
                      <select
                        value={instance?.id ?? ""}
                        onChange={(event) => setInstanceId(event.target.value)}
                        disabled={instances.length === 0 || creating}
                      >
                        {instances.length === 0 ? <option value="">还没有游戏实例</option> : null}
                        {instances.map((item) => (
                          <option key={item.id} value={item.id}>
                            {item.name} · {item.versionId}
                          </option>
                        ))}
                      </select>
                    </Field>
                    <Field label="节点" hint={nodesLoading ? "正在测速与读取负载…" : "自动选择会综合延迟、客户端数量与节点负载。"}>
                      <select
                        value={nodeId}
                        onChange={(event) =>
                          setNodeId(event.target.value === "auto" ? "auto" : Number(event.target.value))
                        }
                        disabled={nodesLoading || creating}
                      >
                        <option value="auto">自动选择（推荐）</option>
                        {nodes.map((node) => (
                          <option key={node.id} value={node.id} disabled={!node.available}>
                            {node.name}
                            {node.latencyMs != null ? ` · ${node.latencyMs} ms` : ""}
                            {node.clients != null ? ` · ${node.clients} 客户端` : ""}
                            {!node.available ? ` · ${node.reason ?? "不可用"}` : ""}
                          </option>
                        ))}
                      </select>
                    </Field>
                  </div>

                  <div className="mp-local-row">
                    <Field label="本地 IP" hint="通常保持 127.0.0.1。">
                      <input
                        className="mono"
                        value={localIp}
                        onChange={(event) => setLocalIp(event.target.value)}
                        disabled={creating}
                      />
                    </Field>
                    <Field label={`本地 ${edition === "java" ? "TCP" : "UDP"} 端口`} hint={localService?.message ?? "手动填写游戏实际监听端口。"}>
                      <div className="mp-port-row">
                        <input
                          className="mono"
                          inputMode="numeric"
                          value={localPort}
                          onChange={(event) => setLocalPort(event.target.value.replace(/[^0-9]/g, ""))}
                          placeholder={edition === "java" ? "25565 或局域网动态端口" : "19132"}
                          disabled={creating}
                        />
                        <Button
                          size="sm"
                          variant="ghost"
                          loading={detecting}
                          onClick={() => void detect()}
                        >
                          自动检测
                        </Button>
                      </div>
                    </Field>
                  </div>

                  <div className="mp-local-note">
                    <Icon name="info" size={15} />
                    <span>
                      {localService
                        ? localService.message
                        : "还没有检测本地端口。Java 版开放局域网后端口通常是动态的。"}
                    </span>
                  </div>

                  <div className="mp-create-actions">
                    <Button
                      variant="primary"
                      icon="play"
                      loading={creating}
                      disabled={!status?.loggedIn || !instance || creating || !localPort.trim()}
                      onClick={() => void createRoom()}
                    >
                      创建联机房间
                    </Button>
                    <span className="muted small">
                      {instance ? `${instance.name} · ${instance.versionId}` : "请先创建游戏实例"}
                    </span>
                  </div>
                </>
              )}
            </Card>
          )}

          {steps.length > 0 ? <StepList steps={steps} /> : null}

          {visibleOrphans.length > 0 ? (
            <Card>
              <div className="card-head">
                <div>
                  <h3>未清理的 SRL 临时隧道</h3>
                  <p>这些隧道名称以 SRL-MP- 开头，可能是上次异常退出留下的。</p>
                </div>
                <Badge tone="warning">{visibleOrphans.length} 条</Badge>
              </div>
              <div className="mp-orphans">
                {visibleOrphans.map((orphan) => (
                  <div className="mp-orphan" key={orphan.tunnelId}>
                    <div>
                      <strong>{orphan.name}</strong>
                      <span className="muted small">
                        {orphan.tunnelType} · 本地端口 {orphan.localPort}
                        {orphan.nodeName ? ` · ${orphan.nodeName}` : ""}
                      </span>
                    </div>
                    <Button size="sm" variant="ghost" onClick={() => void deleteOrphan(orphan.tunnelId)}>
                      删除
                    </Button>
                  </div>
                ))}
              </div>
            </Card>
          ) : null}
        </div>

        <div className="mp-side">
          <Card>
            <div className="card-head">
              <div>
                <h3>当前状态</h3>
                <p>所有状态都来自本机进程与 52FRP API。</p>
              </div>
            </div>
            <dl className="mp-facts">
              <div>
                <dt>隧道服务</dt>
                <dd>{status?.providerName ?? "52FRP"}</dd>
              </div>
              <div>
                <dt>账户</dt>
                <dd>
                  {status?.account?.displayName ??
                    (status?.loggedIn ? "已连接，信息读取失败" : "未连接")}
                </dd>
              </div>
              <div>
                <dt>Minecraft</dt>
                <dd>{instance ? `${instance.versionId}` : "未选择实例"}</dd>
              </div>
              <div>
                <dt>本地服务</dt>
                <dd>
                  {detecting
                    ? "检测中…"
                    : localService?.found
                      ? `${localService.source === "log" ? "日志" : "进程"} · 端口 ${localService.detectedPort ?? "—"}`
                      : "未检测到"}
                </dd>
              </div>
              <div>
                <dt>FRPC</dt>
                <dd>{status?.frpc.running ? `运行中${status.frpc.version ? ` · ${status.frpc.version}` : ""}` : "未运行"}</dd>
              </div>
              <div>
                <dt>联机状态</dt>
                <dd>{online ? "在线" : room?.state === "retained" ? "隧道保留" : "未联机"}</dd>
              </div>
            </dl>
            {status?.loggedIn ? (
              <div className="mp-side-actions">
                <Button size="sm" variant="ghost" onClick={() => void loadStatus()}>
                  刷新状态
                </Button>
                <Button size="sm" variant="ghost" loading={busy === "logout"} onClick={() => void logout()}>
                  退出账户
                </Button>
              </div>
            ) : null}
          </Card>

          <Card>
            <h3>联机说明</h3>
            <ul className="plain-list">
              <li>Java 版使用 TCP；Bedrock 版使用 UDP。</li>
              <li>实际公网地址以 52FRP 返回的 host 与 port 为准。</li>
              <li>停止联机与删除云端隧道是两个操作，删除只针对本次 SRL 创建的临时隧道。</li>
              <li>FRPC 配置包含临时凭据，停止后会删除本地临时文件。</li>
            </ul>
          </Card>
        </div>
      </div>
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
          <span className="eyebrow">{editionLabel(room.edition)}</span>
          <h3>{room.instanceName}</h3>
          <p className="muted small">
            Minecraft {room.versionId} · {room.nodeName}
          </p>
        </div>
        <Badge tone={stateTone(room)}>
          {roomStateLabel(room)}
        </Badge>
      </div>

      <div className={`mp-address ${online ? "mp-address--online" : ""}`}>
        <div>
          <span>公网连接地址</span>
          <strong className="mono">{address(room)}</strong>
        </div>
        <Button size="sm" variant={copied ? "primary" : "secondary"} icon={copied ? "check" : "copy"} onClick={onCopy}>
          {copied ? "已复制" : "复制地址"}
        </Button>
      </div>

      <dl className="mp-room__stats">
        <div>
          <dt>节点</dt>
          <dd>{room.nodeName}</dd>
        </div>
        <div>
          <dt>公网端口</dt>
          <dd className="mono">{room.publicPort}</dd>
        </div>
        <div>
          <dt>本地端口</dt>
          <dd className="mono">{room.localPort}</dd>
        </div>
        <div>
          <dt>节点延迟</dt>
          <dd>{room.latencyMs != null ? `${room.latencyMs} ms` : "未测速"}</dd>
        </div>
        <div>
          <dt>FRPC</dt>
          <dd>{room.frpcRunning ? `运行中${room.frpcPid ? ` · PID ${room.frpcPid}` : ""}` : "已停止"}</dd>
        </div>
        <div>
          <dt>本地进程</dt>
          <dd>{room.localPid ? `PID ${room.localPid}` : "未记录"}</dd>
        </div>
      </dl>

      {room.message ? <p className="mp-room__message">{room.message}</p> : null}

      {frpcLog.length > 0 ? (
        <details className="mp-log">
          <summary>FRPC 运行日志</summary>
          <pre>{frpcLog.join("\n")}</pre>
        </details>
      ) : null}

      <div className="mp-room__actions">
        {room.state !== "retained" ? (
          <Button variant="secondary" icon="refresh" loading={busy === "restart"} onClick={onRestart}>
            重启 FRPC
          </Button>
        ) : null}
        {room.state !== "retained" ? (
          <Button variant="ghost" loading={busy === "stop"} onClick={onStop}>
            停止联机
          </Button>
        ) : null}
        <Button variant="danger" icon="trash" loading={busy === "stop-delete"} onClick={onStopDelete}>
          停止并删除临时隧道
        </Button>
      </div>
    </Card>
  );
}

function StepList({ steps }: { steps: MultiplayerProgress[] }) {
  return (
    <Card>
      <div className="card-head">
        <div>
          <h3>创建过程</h3>
          <p>每一步都对应真实的检查、API 调用或进程启动结果。</p>
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
            <span className="mp-step__state">{step.status === "ok" ? "完成" : step.status === "error" ? "失败" : step.status === "running" ? "进行中" : "等待"}</span>
          </li>
        ))}
      </ol>
    </Card>
  );
}
