import { useEffect, useState } from "react";
import {
  api,
  type CrashAnalysis,
  type CrashFile,
  type DiagnosticsReport,
  type LogFileInfo,
} from "../lib/api";
import { formatBytes, formatDate } from "../lib/format";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, EmptyState, StatusDot } from "../components/ui";
import { Modal } from "../components/Modal";
import { Icon } from "../components/Icon";
import { t } from "../lib/i18n";


export function DiagnosticsPage() {
  const { bootstrap, pushToast } = useApp();
  const [report, setReport] = useState<DiagnosticsReport | null>(null);
  const [logs, setLogs] = useState<LogFileInfo[]>([]);
  const [network, setNetwork] = useState(true);
  const [loading, setLoading] = useState(false);
  const [logContent, setLogContent] = useState<{ name: string; text: string } | null>(null);
  const [logBusy, setLogBusy] = useState(false);
  const [exported, setExported] = useState<string | null>(null);
  const instances = bootstrap?.instances ?? [];
  const [crashInstance, setCrashInstance] = useState(instances[0]?.id ?? "");
  const [crashes, setCrashes] = useState<CrashFile[]>([]);
  const [crashLoading, setCrashLoading] = useState(false);
  const [analysis, setAnalysis] = useState<CrashAnalysis | null>(null);

  async function run() {
    setLoading(true);
    try {
      const [r, l] = await Promise.all([api.diagnostics(network), api.listLogs(20)]);
      setReport(r);
      setLogs(l);
    } catch (e) {
      pushToast(t("诊断失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void run();
  }, []);

  async function loadCrashes(instanceId: string) {
    if (!instanceId) {
      setCrashes([]);
      return;
    }
    setCrashLoading(true);
    try {
      setCrashes(await api.listCrashReports(instanceId));
    } catch (e) {
      pushToast(t("读取崩溃报告失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setCrashLoading(false);
    }
  }

  useEffect(() => {
    if (instances.length > 0 && !instances.some((i) => i.id === crashInstance)) {
      setCrashInstance(instances[0].id);
      return;
    }
    void loadCrashes(crashInstance);
  }, [crashInstance, instances.length]);

  async function deleteCrash(file: CrashFile) {
    try {
      await api.deleteCrashReport(crashInstance, file.fileName);
      setCrashes((prev) => prev.filter((item) => item.fileName !== file.fileName));
      pushToast(t("崩溃报告已删除"), "success", file.fileName);
    } catch (e) {
      pushToast(t("删除崩溃报告失败"), "error", e instanceof Error ? e.message : String(e));
    }
  }

  async function openCrash(file: CrashFile) {
    try {
      setAnalysis(await api.analyzeCrash(crashInstance, file.fileName));
    } catch (e) {
      pushToast(t("无法解析崩溃报告"), "error", e instanceof Error ? e.message : String(e));
    }
  }

  async function openLog(log: LogFileInfo) {
    try {
      const text = await api.readLog(log.path, 400);
      setLogContent({ name: log.name, text });
    } catch (e) {
      pushToast(t("无法读取日志"), "error", e instanceof Error ? e.message : String(e));
    }
  }

  async function refreshLogs() {
    setLogs(await api.listLogs(20));
  }

  async function cleanupLogs() {
    setLogBusy(true);
    try {
      const result = await api.cleanupLogs();
      await refreshLogs();
      pushToast(
        result.removed > 0 ? t("日志已清理") : t("没有需要清理的日志"),
        "success",
        result.removed > 0
          ? t("删除 {0} 个文件，释放 {1}", [result.removed, formatBytes(result.bytes)])
          : t("保留最近 14 天且总量不超过 200MB"),
      );
    } catch (e) {
      pushToast(t("清理日志失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setLogBusy(false);
    }
  }

  async function deleteLog(log: LogFileInfo) {
    setLogBusy(true);
    try {
      await api.deleteLog(log.path);
      setLogs((prev) => prev.filter((item) => item.path !== log.path));
      pushToast(t("日志已删除"), "success", log.name);
    } catch (e) {
      pushToast(t("删除日志失败"), "error", e instanceof Error ? e.message : String(e));
    } finally {
      setLogBusy(false);
    }
  }

  async function exportReport() {
    try {
      setExported(await api.exportDiagnostics(network));
    } catch (e) {
      pushToast(t("导出失败"), "error", e instanceof Error ? e.message : String(e));
    }
  }

  async function copyExport() {
    if (!exported) return;
    try {
      await navigator.clipboard.writeText(exported);
      pushToast(t("诊断报告已复制"), "success");
    } catch {
      pushToast(t("复制失败"), "warning");
    }
  }

  const problems = report?.checks.filter((c) => c.status !== "ok").length ?? 0;

  return (
    <div className="page">
      <div className="toolbar">
        <div>
          <h2>{t("诊断中心")}</h2>
          <p className="muted">{t("检查本机环境、网络与运行日志。所有检查都在本地执行。")}</p>
        </div>
        <div className="toolbar__actions">
          <label className="inline-check inline-check--chip">
            <input
              type="checkbox"
              checked={network}
              onChange={(e) => setNetwork(e.target.checked)}
            />
            {t("网络检查")}</label>
          <Button icon="refresh" onClick={run} loading={loading}>
            {t("重新检查")}</Button>
          <Button icon="shield" onClick={exportReport}>
            {t("导出报告")}</Button>
        </div>
      </div>

      {report ? (
        <>
          <div className="diag-summary">
            <Card>
              <span className="muted">{t("检查结果")}</span>
              <strong className={problems > 0 ? "text-warning" : "text-success"}>
                {problems === 0 ? t("全部通过") : t("{0} 项需要关注", [problems])}
              </strong>
              <span className="muted small">{formatDate(report.generatedAt)}</span>
            </Card>
            <Card>
              <span className="muted">{t("平台")}</span>
              <strong>{report.platform}</strong>
              <span className="muted small mono ellipsis">{report.gameDir}</span>
            </Card>
            <Card>
              <span className="muted">{t("数据规模")}</span>
              <strong>
                {report.installedVersions} {t("版本 ·")}{report.instances} {t("实例")}</strong>
              <span className="muted small">{report.java.length} {t("个 Java")}</span>
            </Card>
          </div>

          <div className="grid-2">
            <Card>
              <div className="card-head">
                <div>
                  <h3>{t("环境检查")}</h3>
                  <p>{t("错误会阻止启动，警告不阻塞但建议处理。")}</p>
                </div>
              </div>
              <ul className="check-list">
                {report.checks.map((c) => (
                  <li key={c.id} className={`check-item check-item--${c.status}`}>
                    <StatusDot status={c.status} />
                    <div>
                      <div className="check-item__head">
                        <strong>{c.label}</strong>
                        <Badge
                          tone={
                            c.status === "ok" ? "success" : c.status === "warning" ? "warning" : "danger"
                          }
                        >
                          {c.status === "ok" ? t("正常") : c.status === "warning" ? t("警告") : t("错误")}
                        </Badge>
                      </div>
                      <p>{c.detail}</p>
                      {c.fix ? <span className="check-item__fix">{t("建议：")}{c.fix}</span> : null}
                    </div>
                  </li>
                ))}
              </ul>
            </Card>

            <Card>
              <div className="card-head">
                <div>
                  <h3>Java</h3>
                  <p>{t("共发现")}{report.java.length} {t("个环境。")}</p>
                </div>
                <Icon name="coffee" size={18} />
              </div>
              {report.java.length === 0 ? (
                <EmptyState
                  icon="coffee"
                  title={t("未发现 Java")}
                  description={t("请在 Java 环境页面手动指定可执行文件。")}
                />
              ) : (
                <ul className="java-mini-list">
                  {report.java.map((j) => (
                    <li key={j.path}>
                      <strong>Java {j.major}</strong>
                      <span>{j.versionString}</span>
                      <code title={j.path}>{j.path}</code>
                    </li>
                  ))}
                </ul>
              )}
            </Card>
          </div>

          <Card>
            <div className="card-head">
              <div>
                <h3>{t("运行日志")}</h3>
                <p>{t("只列出启动器日志目录中的文件，读取时自动脱敏；启动时自动清理 14 天前的日志。")}</p>
              </div>
              <div className="toolbar__actions">
                <span className="muted small mono">{bootstrap?.paths.logsDir}</span>
                <Button
                  size="sm"
                  icon="trash"
                  loading={logBusy}
                  onClick={cleanupLogs}
                >
                  {t("清理旧日志")}</Button>
              </div>
            </div>
            {logs.length === 0 ? (
              <EmptyState
                icon="terminal"
                title={t("还没有日志")}
                description={t("启动游戏或执行安装后，日志会写入这里。")}
              />
            ) : (
              <ul className="log-list">
                {logs.map((log) => (
                  <li key={log.path}>
                    <button className="log-list__open" onClick={() => openLog(log)}>
                      <Icon name="terminal" size={16} />
                      <div>
                        <strong>{log.name}</strong>
                        <span className="muted small">
                          {formatBytes(log.size)} · {formatDate(log.modified)}
                        </span>
                      </div>
                      <span className="log-list__open-label">{t("查看")}</span>
                    </button>
                    <button
                      className="log-list__delete"
                      title={t("删除该日志")}
                      aria-label={t("删除 {0}", [log.name])}
                      disabled={logBusy}
                      onClick={() => void deleteLog(log)}
                    >
                      <Icon name="trash" size={15} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </Card>
        </>
      ) : loading ? (
        <Card>
          <p className="muted">{t("正在执行检查…")}</p>
        </Card>
      ) : (
        <Card>
          <EmptyState
            icon="activity"
            title={t("尚未运行诊断")}
            description={t("点击重新检查以收集环境信息。")}
            action={
              <Button variant="primary" onClick={run}>
                {t("开始检查")}</Button>
            }
          />
        </Card>
      )}

      <Card>
        <div className="card-head">
          <div>
            <h3>{t("崩溃报告")}</h3>
            <p>{t("从实例的 crash-reports/ 读取；只根据报告内容分类，不臆测根因。")}</p>
          </div>
          <div className="card-head__actions">
            {instances.length > 0 ? (
              <select
                value={crashInstance}
                onChange={(e) => setCrashInstance(e.target.value)}
                aria-label={t("选择实例")}
                style={{ minWidth: 180 }}
              >
                {instances.map((i) => (
                  <option key={i.id} value={i.id}>
                    {i.name}
                  </option>
                ))}
              </select>
            ) : null}
            <Button
              size="sm"
              icon="refresh"
              loading={crashLoading}
              onClick={() => void loadCrashes(crashInstance)}
              disabled={!crashInstance}
            >
              {t("刷新")}</Button>
          </div>
        </div>
        {instances.length === 0 ? (
          <EmptyState
            icon="box"
            title={t("没有游戏实例")}
            description={t("创建实例并启动后，崩溃报告会写入实例的 crash-reports/ 目录。")}
          />
        ) : crashes.length === 0 ? (
          <EmptyState
            icon="check"
            title={t("没有崩溃报告")}
            description={
              crashLoading ? t("正在读取…") : t("该实例目前没有 crash-reports/*.txt。")
            }
          />
        ) : (
          <ul className="log-list">
            {crashes.map((c) => (
              <li key={c.fileName}>
                <button className="log-list__open" onClick={() => openCrash(c)}>
                  <Icon name="alert" size={16} />
                  <div>
                    <strong>{c.fileName}</strong>
                    <span className="muted small">
                      {formatBytes(c.size)} · {formatDate(c.modifiedAt)}
                    </span>
                  </div>
                  <span className="log-list__open-label">{t("分析")}</span>
                </button>
                <button
                  className="log-list__delete"
                  title={t("删除该崩溃报告")}
                  aria-label={t("删除崩溃报告 {0}", [c.fileName])}
                  onClick={() => void deleteCrash(c)}
                >
                  <Icon name="trash" size={15} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </Card>

      <Modal
        open={analysis !== null}
        onClose={() => setAnalysis(null)}
        title={analysis ? t("崩溃分析 · {0}", [analysis.fileName]) : ""}
        subtitle={t("基于报告文本的模式匹配；未识别的崩溃会明确说明。")}
        wide
      >
        {analysis ? (
          <>
            <div className="detail-list">
              <div>
                <span>{t("描述")}</span>
                <strong>{analysis.description ?? t("报告未提供")}</strong>
              </div>
              <div>
                <span>Minecraft</span>
                <strong>{analysis.minecraftVersion ?? t("未知")}</strong>
              </div>
              <div>
                <span>Java</span>
                <strong>{analysis.javaVersion ?? t("未知")}</strong>
              </div>
              <div>
                <span>{t("加载器")}</span>
                <strong>{analysis.loader ?? t("未记录 / 原版")}</strong>
              </div>
            </div>

            <div className="crash-causes">
              {analysis.causes.map((c) => (
                <div className="cause-card" key={c.title + c.detail}>
                  <strong>{c.title}</strong>
                  <p>{c.detail}</p>
                  <span>{t("建议：")}{c.advice}</span>
                </div>
              ))}
            </div>

            {analysis.suspectMods.length > 0 ? (
              <div className="suspect-mods">
                <span className="muted small">{t("报告中列出的模组")}</span>
                <div>
                  {analysis.suspectMods.map((m) => (
                    <Badge key={m}>{m}</Badge>
                  ))}
                </div>
              </div>
            ) : null}

            <details className="crash-excerpt">
              <summary>{t("查看报告开头（已脱敏）")}</summary>
              <pre className="console">{analysis.excerpt}</pre>
            </details>
          </>
        ) : null}
      </Modal>

      <div className="notice">
        <Icon name="info" size={17} />
        <div>
          <strong>{t("DDoS 与本地优先")}</strong>
          <p>
            {t("本启动器不依赖自建后端；已安装版本在断网时仍可启动。客户端只使用有限并发与退避重试，不做无限轮询。任何客户端设计都无法彻底防止服务端被攻击。")}</p>
        </div>
      </div>

      <Modal
        open={logContent !== null}
        onClose={() => setLogContent(null)}
        title={logContent?.name ?? t("日志")}
        subtitle={t("已脱敏，最多显示最后 400 行")}
        wide
        footer={
          <Button
            variant="ghost"
            icon="copy"
            onClick={() => logContent && navigator.clipboard.writeText(logContent.text)}
          >
            {t("复制")}</Button>
        }
      >
        <pre className="console console--modal">{logContent?.text || t("（空）")}</pre>
      </Modal>

      <Modal
        open={exported !== null}
        onClose={() => setExported(null)}
        title={t("诊断报告预览")}
        subtitle={t("复制前请确认内容；报告不会自动上传。")}
        wide
        footer={
          <>
            <Button variant="ghost" onClick={() => setExported(null)}>
              {t("关闭")}</Button>
            <Button variant="primary" icon="copy" onClick={copyExport}>
              {t("复制报告")}</Button>
          </>
        }
      >
        <pre className="report-preview">{exported}</pre>
      </Modal>
    </div>
  );
}

