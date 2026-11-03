import { useEffect, useRef, useState } from "react";
import {
  api,
  type AssistantConversation,
  type AssistantConversationSummary,
  type AssistantProposal,
  type AssistantReply,
  type AssistantStatus,
  type ModInstallPlan,
  type ModInstallResult,
  type RepairPlan,
  type RepairResult,
} from "../lib/api";
import { formatBytes } from "../lib/format";
import { useApp } from "../state/AppContext";
import { Badge, Button } from "../components/ui";
import { Icon } from "../components/Icon";
import { MarkdownText } from "../components/MarkdownText";
import logo from "../assets/logo.png";

interface ChatItem {
  id: number;
  role: "user" | "assistant";
  content: string;
  traces?: AssistantReply["toolTraces"];
  proposals?: AssistantProposal[];
  error?: boolean;
}

const QUICK_PROMPTS = [
  "分析当前实例最近一次崩溃，直接告诉我最可能的原因和怎么修。",
  "当前实例启动就崩，帮我看看日志和崩溃报告。",
  "搜索适合当前版本的性能优化模组，并告诉我推荐哪一个。",
  "帮我配一个适合当前版本的轻量性能整合包，先给安装计划和依赖，不要直接安装。",
];

export function AssistantPage() {
  const { bootstrap, pushToast, setPage } = useApp();
  const instances = bootstrap?.instances ?? [];
  const [status, setStatus] = useState<AssistantStatus | null>(null);
  const [instanceId, setInstanceId] = useState(instances[0]?.id ?? "");
  const [conversations, setConversations] = useState<AssistantConversationSummary[]>([]);
  const [conversationId, setConversationId] = useState<string | null>(null);
  const [messages, setMessages] = useState<ChatItem[]>([]);
  const [input, setInput] = useState("");
  const [sending, setSending] = useState(false);
  const [loadingHistory, setLoadingHistory] = useState(false);
  const logRef = useRef<HTMLDivElement | null>(null);
  const nextId = useRef(1);
  const bootstrapped = useRef(false);

  useEffect(() => {
    api
      .assistantStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  useEffect(() => {
    if (!instanceId && instances[0]) setInstanceId(instances[0].id);
  }, [instances, instanceId]);

  useEffect(() => {
    if (bootstrapped.current) return;
    bootstrapped.current = true;
    void (async () => {
      try {
        const list = await api.assistantConversations();
        setConversations(list ?? []);
        if (list && list.length > 0) {
          await selectConversation(list[0].id);
        }
      } catch {
        /* history is optional */
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [messages, sending]);

  async function refreshConversations() {
    try {
      setConversations((await api.assistantConversations()) ?? []);
    } catch {
      /* keep the current list */
    }
  }

  async function selectConversation(id: string) {
    setLoadingHistory(true);
    try {
      const conv: AssistantConversation = await api.assistantConversation(id);
      setConversationId(conv.id);
      if (conv.instanceId && instances.some((i) => i.id === conv.instanceId)) {
        setInstanceId(conv.instanceId);
      }
      setMessages(
        conv.messages
          .filter((m) => m.role === "user" || m.role === "assistant")
          .map((m, index) => ({
            id: index + 1,
            role: m.role,
            content: m.content,
            traces: m.traces,
            proposals: m.proposals,
          })),
      );
      nextId.current = conv.messages.length + 1;
    } catch (e) {
      pushToast("读取聊天记录失败", "error", e instanceof Error ? e.message : String(e));
    } finally {
      setLoadingHistory(false);
    }
  }

  function newChat() {
    setConversationId(null);
    setMessages([]);
    setInput("");
  }

  async function deleteConversation(id: string) {
    try {
      await api.assistantDeleteConversation(id);
      if (id === conversationId) newChat();
      setConversations((prev) => prev.filter((item) => item.id !== id));
      pushToast("聊天记录已删除", "success");
    } catch (e) {
      pushToast("删除聊天记录失败", "error", e instanceof Error ? e.message : String(e));
    }
  }

  async function send(text?: string) {
    const content = (text ?? input).trim();
    if (!content || sending) return;
    if (!status?.configured) {
      pushToast("尚未配置 DeepSeek API key", "warning", "请先在设置中配置");
      return;
    }
    setMessages((prev) => [
      ...prev,
      { id: nextId.current++, role: "user", content },
    ]);
    setInput("");
    setSending(true);
    try {
      const reply = await api.assistantChat(
        conversationId,
        instanceId || null,
        content,
      );
      setConversationId(reply.conversationId);
      setMessages((prev) => [
        ...prev,
        {
          id: nextId.current++,
          role: "assistant",
          content: reply.content,
          traces: reply.toolTraces,
          proposals: reply.proposals,
        },
      ]);
      await refreshConversations();
    } catch (e) {
      setMessages((prev) => [
        ...prev,
        {
          id: nextId.current++,
          role: "assistant",
          content: e instanceof Error ? e.message : String(e),
          error: true,
        },
      ]);
    } finally {
      setSending(false);
    }
  }

  const current = instances.find((item) => item.id === instanceId);

  return (
    <div className="page assistant-page assistant-page--chat">
      <div className="assistant-chat-head">
        <div className="assistant-head">
          <img className="assistant-avatar" src={logo} alt="鸦鸭" />
          <div>
            <h2>鸦鸭</h2>
            <p className="muted">
              {current ? `${current.name} · ${current.versionId}` : "不绑定实例"}
            </p>
          </div>
        </div>
        <div className="assistant-chat-head__actions">
          <select
            value={instanceId}
            onChange={(e) => setInstanceId(e.target.value)}
            aria-label="选择实例"
          >
            <option value="">不绑定实例</option>
            {instances.map((instance) => (
              <option key={instance.id} value={instance.id}>
                {instance.name} · {instance.versionId}
              </option>
            ))}
          </select>
          <Badge tone={status?.configured ? "success" : "warning"}>
            {status?.configured ? status.model : "未配置 key"}
          </Badge>
          <Button size="sm" variant="ghost" icon="settings" onClick={() => setPage("settings")}>
            设置
          </Button>
        </div>
      </div>

      {!status?.configured ? (
        <div className="notice notice--warning assistant-config-notice">
          <Icon name="alert" size={16} />
          <div>
            <strong>配置 DeepSeek API key 后即可对话</strong>
            <p>
              鸦鸭会直接读取当前实例的崩溃报告和日志，用自然语言告诉你原因和修复方式。
              本地诊断仍可在诊断中心使用。
            </p>
          </div>
          <Button size="sm" onClick={() => setPage("settings")}>
            去配置
          </Button>
        </div>
      ) : null}

      <div className="assistant-layout">
        <aside className="assistant-history">
          <Button icon="plus" onClick={newChat}>
            新对话
          </Button>
          <div className="assistant-history__list">
            {conversations.length === 0 ? (
              <p className="muted small">还没有历史对话</p>
            ) : (
              conversations.map((conv) => (
                <div
                  key={conv.id}
                  className={`assistant-history__item ${
                    conv.id === conversationId ? "assistant-history__item--active" : ""
                  }`}
                >
                  <button
                    type="button"
                    className="assistant-history__open"
                    onClick={() => void selectConversation(conv.id)}
                    title={conv.title}
                  >
                    <span>{conv.title}</span>
                    <small>{conv.messageCount} 条</small>
                  </button>
                  <button
                    type="button"
                    className="assistant-history__delete"
                    aria-label={`删除对话 ${conv.title}`}
                    title="删除对话"
                    onClick={() => void deleteConversation(conv.id)}
                  >
                    <Icon name="trash" size={14} />
                  </button>
                </div>
              ))
            )}
          </div>
        </aside>

        <div className="assistant-chat-shell">
          <div className="chat-log chat-log--page" ref={logRef}>
            {loadingHistory ? (
              <p className="muted small">正在读取聊天记录…</p>
            ) : messages.length === 0 ? (
              <div className="assistant-welcome">
                <img src={logo} alt="" className="assistant-welcome__logo" />
                <h3>有什么可以帮你？</h3>
                <p className="muted">
                  我不会让你自己看崩溃日志。直接描述现象，或者点下面的问题开始。
                </p>
                <div className="quick-prompts quick-prompts--center">
                  {QUICK_PROMPTS.map((prompt) => (
                    <button
                      key={prompt}
                      type="button"
                      className="quick-prompt"
                      disabled={!status?.configured}
                      onClick={() => void send(prompt)}
                    >
                      {prompt}
                    </button>
                  ))}
                </div>
              </div>
            ) : (
              messages.map((item) => (
                <div
                  key={item.id}
                  className={`chat-msg chat-msg--${item.role} ${item.error ? "chat-msg--error" : ""}`}
                >
                  <div className="chat-msg__role">{item.role === "user" ? "你" : "鸦鸭"}</div>
                  <div className="chat-msg__body">
                    <MarkdownText text={item.content} />
                    {item.traces && item.traces.length > 0 ? (
                      <div className="tool-traces">
                        {item.traces.map((trace, index) => (
                          <span
                            key={`${trace.name}-${index}`}
                            className={`tool-trace ${trace.ok ? "" : "tool-trace--failed"}`}
                            title={trace.summary}
                          >
                            {trace.ok ? "✓" : "!"} {trace.summary}
                          </span>
                        ))}
                      </div>
                    ) : null}
                    {item.proposals?.map((proposal) => (
                      <ProposalCard
                        key={`${proposal.kind}-${proposal.id}`}
                        proposal={proposal}
                        instanceId={instanceId}
                      />
                    ))}
                  </div>
                </div>
              ))
            )}
            {sending ? (
              <div className="chat-msg chat-msg--assistant">
                <div className="chat-msg__role">鸦鸭</div>
                <div className="chat-msg__body">
                  <p className="muted">正在读取你的实例信息并分析…</p>
                </div>
              </div>
            ) : null}
          </div>

          <div className="assistant-compose assistant-compose--page">
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder={
                status?.configured
                  ? "描述问题，或让我分析当前实例…（⌘/Ctrl + Enter 发送）"
                  : "配置 DeepSeek API key 后即可对话"
              }
              rows={2}
              maxLength={20000}
              disabled={sending || !status?.configured}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                  e.preventDefault();
                  void send();
                }
              }}
            />
            <Button
              variant="primary"
              icon="play"
              loading={sending}
              disabled={!input.trim() || !status?.configured}
              onClick={() => void send()}
            >
              发送
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

function ProposalCard({
  proposal,
  instanceId,
}: {
  proposal: AssistantProposal;
  instanceId: string;
}) {
  const { pushToast, reload } = useApp();
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [repairResult, setRepairResult] = useState<RepairResult | null>(null);
  const [modResult, setModResult] = useState<ModInstallResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const repair = proposal.kind === "repair" ? (proposal.plan as RepairPlan) : null;
  const mod = proposal.kind === "mod_install" ? (proposal.plan as ModInstallPlan) : null;
  const done = Boolean(repairResult || modResult);
  const actionCount = repair?.actions.length ?? mod?.entries.length ?? 0;

  async function apply() {
    if (!confirmed || done) return;
    setBusy(true);
    setError(null);
    try {
      if (repair) {
        const result = await api.assistantApplyRepair(repair.id);
        setRepairResult(result);
        pushToast("修复已执行", "success", result.applied.join("；"));
      } else if (mod) {
        const result = await api.assistantApplyModPlan(mod.id);
        setModResult(result);
        pushToast("模组安装完成", "success", result.installed.join("；"));
      }
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function rollback() {
    setBusy(true);
    setError(null);
    try {
      if (repairResult) {
        await api.assistantRollbackRepair(instanceId, repairResult.token);
        setRepairResult(null);
        pushToast("已回滚修复", "success");
      } else if (modResult) {
        await api.assistantRollbackModInstall(instanceId, modResult.token);
        setModResult(null);
        pushToast("已回滚模组安装", "success");
      }
      setConfirmed(false);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  if (!repair && !mod) return null;

  return (
    <div className="proposal-card">
      <div className="proposal-card__head">
        <div>
          <strong>{proposal.title}</strong>
          <span className="muted small">{proposal.summary}</span>
        </div>
        <Badge tone="warning">{actionCount} 个操作</Badge>
      </div>

      {repair ? (
        <ul className="proposal-list">
          {repair.actions.map((action, index) => (
            <li key={`${action.kind}-${index}`}>
              <strong>{action.description}</strong>
              <span className="muted small">{action.risk}</span>
              <span className="muted small mono">{action.evidence.join(" · ")}</span>
            </li>
          ))}
        </ul>
      ) : null}

      {mod ? (
        <ul className="proposal-list">
          {mod.entries.map((entry) => (
            <li key={`${entry.projectId}-${entry.versionId}`}>
              <strong>
                {entry.title} <span className="muted">{entry.versionNumber}</span>
              </strong>
              <span className="muted small mono">{entry.fileName}</span>
              <span className="muted small">
                {formatBytes(entry.size)}
                {entry.requiredBy.length > 0
                  ? ` · 依赖：${entry.requiredBy.join(", ")}`
                  : " · 主模组"}
              </span>
            </li>
          ))}
        </ul>
      ) : null}

      {repair && repair.notes.length > 0 ? (
        <ul className="proposal-notes">
          {repair.notes.map((note) => (
            <li key={note}>{note}</li>
          ))}
        </ul>
      ) : null}
      {mod && mod.warnings.length > 0 ? (
        <ul className="proposal-notes">
          {mod.warnings.map((note) => (
            <li key={note}>{note}</li>
          ))}
        </ul>
      ) : null}

      {error ? <p className="form-error">{error}</p> : null}

      {done ? (
        <div className="proposal-card__done">
          <span className="muted small">
            {repairResult
              ? `已备份到 ${repairResult.backupDir}`
              : `已安装：${modResult?.installed.join("；")}`}
          </span>
          <Button size="sm" icon="refresh" loading={busy} onClick={() => void rollback()}>
            回滚本次操作
          </Button>
        </div>
      ) : actionCount > 0 ? (
        <>
          <label className="checkbox-row">
            <input
              type="checkbox"
              checked={confirmed}
              onChange={(e) => setConfirmed(e.target.checked)}
              disabled={busy}
            />
            <span>
              我已核对上述内容，并了解备份与回滚方式
              <small>
                {repair
                  ? "只修改 instance.json 和 mods/ 文件名，不碰存档。"
                  : "下载先进入临时目录并校验 SHA-1；同名文件会先备份。"}
              </small>
            </span>
          </label>
          <div className="proposal-card__actions">
            <Button
              variant="primary"
              icon={mod ? "download" : "shield"}
              loading={busy}
              disabled={!confirmed}
              onClick={() => void apply()}
            >
              {mod ? "安装模组" : "执行修复"}
            </Button>
          </div>
        </>
      ) : (
        <p className="muted small">没有可执行操作。</p>
      )}
    </div>
  );
}
