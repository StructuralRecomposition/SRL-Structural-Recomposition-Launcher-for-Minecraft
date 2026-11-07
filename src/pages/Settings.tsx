import { useEffect, useState } from "react";
import { api, type Settings } from "../lib/api";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, Field, Toggle } from "../components/ui";
import { Icon } from "../components/Icon";

export function SettingsPage() {
  const { bootstrap, updateBootstrap, pushToast, refreshTasks, theme, setTheme } = useApp();
  const [form, setForm] = useState<Settings | null>(bootstrap?.settings ?? null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cfKey, setCfKey] = useState("");
  const [cfSource, setCfSource] = useState<"builtin" | "custom">("builtin");
  const [cfBusy, setCfBusy] = useState(false);
  const [cfMessage, setCfMessage] = useState<string | null>(null);
  const [dsKey, setDsKey] = useState("");
  const [dsConfigured, setDsConfigured] = useState(false);
  const [dsBusy, setDsBusy] = useState(false);
  const [dsMessage, setDsMessage] = useState<string | null>(null);
  const [frpToken, setFrpToken] = useState("");
  const [frpConfigured, setFrpConfigured] = useState(false);
  const [frpBusy, setFrpBusy] = useState(false);
  const [frpMessage, setFrpMessage] = useState<string | null>(null);

  useEffect(() => {
    setForm(bootstrap?.settings ?? null);
  }, [bootstrap?.settings]);

  useEffect(() => {
    api
      .assistantStatus()
      .then((status) => setDsConfigured(status.configured))
      .catch(() => setDsConfigured(false));
  }, []);

  useEffect(() => {
    api
      .multiplayerStatus()
      .then((status) => setFrpConfigured(status.tokenConfigured))
      .catch(() => setFrpConfigured(false));
  }, []);

  useEffect(() => {
    api
      .curseforgeKeyStatus()
      .then((value) => setCfSource(value === "custom" ? "custom" : "builtin"))
      .catch(() => setCfSource("builtin"));
  }, []);

  async function saveCurseForgeKey() {
    if (!cfKey.trim()) return;
    setCfBusy(true);
    setCfMessage(null);
    try {
      await api.setCurseforgeApiKey(cfKey.trim());
      setCfSource("custom");
      setCfKey("");
      setCfMessage("个人 API key 已保存到系统凭据库，将优先于内置公共 key 使用");
      pushToast("CurseForge API key 已保存", "success");
    } catch (e) {
      setCfMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setCfBusy(false);
    }
  }

  async function testCurseForgeKey() {
    setCfBusy(true);
    setCfMessage(null);
    try {
      await api.testCurseforgeApiKey();
      setCfMessage("连接成功，key 可用");
      pushToast("CurseForge 连接成功", "success");
    } catch (e) {
      setCfMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setCfBusy(false);
    }
  }

  async function saveDeepSeekKey() {
    if (!dsKey.trim()) return;
    setDsBusy(true);
    setDsMessage(null);
    try {
      await api.setDeepseekApiKey(dsKey.trim());
      setDsConfigured(true);
      setDsKey("");
      setDsMessage("DeepSeek key 已保存到系统凭据库");
      pushToast("DeepSeek key 已保存", "success");
    } catch (e) {
      setDsMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setDsBusy(false);
    }
  }

  async function testDeepSeekKey() {
    setDsBusy(true);
    setDsMessage(null);
    try {
      await api.testDeepseekApiKey();
      setDsMessage("连接成功，key 可用");
      pushToast("DeepSeek 连接成功", "success");
    } catch (e) {
      setDsMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setDsBusy(false);
    }
  }

  async function clearDeepSeekKey() {
    setDsBusy(true);
    setDsMessage(null);
    try {
      await api.clearDeepseekApiKey();
      setDsConfigured(false);
      setDsMessage("已清除个人 DeepSeek key");
      pushToast("已清除 DeepSeek key", "info");
    } catch (e) {
      setDsMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setDsBusy(false);
    }
  }

  async function clearCurseForgeKey() {
    setCfBusy(true);
    setCfMessage(null);
    try {
      await api.clearCurseforgeApiKey();
      setCfSource("builtin");
      setCfMessage("已清除个人 key，恢复使用内置公共 key");
      pushToast("已清除 CurseForge API key", "info");
    } catch (e) {
      setCfMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setCfBusy(false);
    }
  }

  if (!bootstrap || !form) {
    return (
      <div className="page">
        <p className="muted">正在加载设置…</p>
      </div>
    );
  }

  const dirty = JSON.stringify(form) !== JSON.stringify(bootstrap.settings);

  function patch(p: Partial<Settings>) {
    setForm((prev) => (prev ? { ...prev, ...p } : prev));
  }

  async function save() {
    if (!form) return;
    setError(null);
    setSaving(true);
    try {
      const saved = await api.updateSettings(form);
      updateBootstrap((b) => ({ ...b, settings: saved }));
      setForm(saved);
      pushToast("设置已保存", "success");
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }

  async function saveFrpToken() {
    if (!frpToken.trim()) return;
    setFrpBusy(true);
    setFrpMessage(null);
    try {
      await api.multiplayerConnect(frpToken.trim());
      setFrpConfigured(true);
      setFrpToken("");
      setFrpMessage("52FRP API 令牌已验证并保存到系统凭据库");
      pushToast("52FRP 已连接", "success");
    } catch (e) {
      setFrpMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setFrpBusy(false);
    }
  }

  async function clearFrpToken() {
    setFrpBusy(true);
    setFrpMessage(null);
    try {
      await api.multiplayerLogout();
      setFrpConfigured(false);
      setFrpMessage("已清除 52FRP API 令牌");
      pushToast("已退出 52FRP", "info");
    } catch (e) {
      setFrpMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setFrpBusy(false);
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      pushToast("已复制到剪贴板", "success");
    } catch {
      pushToast("复制失败", "warning", "当前环境不允许访问剪贴板");
    }
  }

  async function clearTasks() {
    try {
      await api.clearFinishedTasks();
      await refreshTasks();
      pushToast("已清除结束的任务", "info");
    } catch (e) {
      pushToast("清除失败", "error", e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <div className="page">
      <div className="toolbar">
        <div>
          <h2>设置</h2>
          <p className="muted">修改会立即写入本地配置文件，不经过任何服务器。</p>
        </div>
        <div className="toolbar__actions">
          <Button variant="ghost" onClick={() => setForm(bootstrap.settings)} disabled={!dirty || saving}>
            还原
          </Button>
          <Button variant="primary" onClick={save} loading={saving} disabled={!dirty}>
            保存设置
          </Button>
        </div>
      </div>

      {error ? <p className="form-error">{error}</p> : null}

      <div className="settings-grid">
        <div className="settings-main">
          <Card>
            <div className="card-head">
              <div>
                <h3>游戏与下载</h3>
                <p>版本、库、资源与实例都存放在游戏目录下。</p>
              </div>
            </div>
            <Field
              label="游戏目录"
              hint="更改后，新的安装与实例使用新目录；已有实例保留其原始路径。"
            >
              <input
                className="mono"
                value={form.gameDir}
                onChange={(e) => patch({ gameDir: e.target.value })}
              />
            </Field>
            <div className="form-grid">
              <Field label="并发下载数" hint={`1 - 16，当前建议 ${form.maxConcurrentDownloads} 个连接`}>
                <input
                  type="number"
                  min={1}
                  max={16}
                  value={form.maxConcurrentDownloads}
                  onChange={(e) => patch({ maxConcurrentDownloads: Number(e.target.value) })}
                />
              </Field>
              <Field label="界面主题" hint="主题立即预览，点击保存后持久化。">
                <select
                  value={theme}
                  onChange={(e) => {
                    const next = e.target.value as "dark" | "light";
                    setTheme(next);
                    patch({ theme: next });
                  }}
                >
                  <option value="dark">深色</option>
                  <option value="light">浅色</option>
                </select>
              </Field>
            </div>
            <Toggle
              id="assets-default"
              checked={form.downloadAssetsOnInstall}
              onChange={(v) => patch({ downloadAssetsOnInstall: v })}
              label="安装版本时默认下载资源文件"
              description="资源文件约数百 MB，但缺少资源会影响声音与贴图。安装时可逐次覆盖此选项。"
            />
            <Toggle
              id="offline-skin"
              checked={form.applyOfflineSkin}
              onChange={(v) => patch({ applyOfflineSkin: v })}
              label="为离线账号应用本地皮肤"
              description="启动时把所选皮肤写入实例的 resourcepacks 并启用本地资源包。仅本机可见，会覆盖默认皮肤贴图，无法改变手臂模型。"
            />
          </Card>

          <Card>
            <div className="card-head">
              <div>
                <h3>网络与来源</h3>
                <p>默认只信任 Mojang 官方 HTTPS 域名。</p>
              </div>
              <Badge tone="success">官方源 + 国内镜像</Badge>
            </div>
            <Toggle
              id="use-mirror"
              checked={form.useMirror}
              onChange={(v) => patch({ useMirror: v })}
              label="优先使用国内镜像（BMCLAPI）"
              description="Fabric 元数据与 Maven 构件优先走 BMCLAPI，Java 一键安装优先走清华 TUNA Adoptium 镜像，失败时自动回退官方源。Quilt 仍使用官方 meta；不会绕过 SHA-1 校验（镜像缺失校验值时如实标注）。"
            />
            <Toggle
              id="custom-hosts"
              checked={false}
              onChange={() => {
                pushToast("自定义镜像尚未实现", "warning", "阶段 1 固定使用官方域名，避免未验证的下载源。");
              }}
              label="允许自定义下载镜像"
              description="未实现。启用前需要镜像签名、校验与可信度评估。"
            />
            <Field
              label="Microsoft 登录 client id"
              hint="Azure 应用的公开客户端 ID（GUID）。留空则保持登录禁用；这不是密钥。"
            >
              <input
                className="mono"
                value={form.microsoftClientId ?? ""}
                onChange={(e) =>
                  patch({ microsoftClientId: e.target.value.trim() || null })
                }
                placeholder="00000000-0000-0000-0000-000000000000"
              />
            </Field>
            <Field label="语言" hint="当前版本仅提供简体中文界面。">
              <input value="简体中文 (zh-CN)" readOnly disabled />
            </Field>
          </Card>

          <Card>
            <div className="card-head">
              <div>
                <h3>Minecraft 联机（52FRP）</h3>
                <p>使用 52FRP API 访问令牌连接账户；令牌只保存在系统凭据库。</p>
              </div>
              <Badge tone={frpConfigured ? "success" : "warning"}>
                {frpConfigured ? "已连接" : "未连接"}
              </Badge>
            </div>
            <Field
              label="52FRP API 访问令牌"
              hint="登录 52FRP 个人中心，在「API 访问令牌」创建（以 hz_ 开头，只显示一次）。不要填账号密码或 FRPC token。"
            >
              <input
                type="password"
                value={frpToken}
                onChange={(e) => setFrpToken(e.target.value)}
                placeholder={frpConfigured ? "已保存令牌，输入新令牌可替换" : "hz_..."}
                autoComplete="off"
              />
            </Field>
            <div className="toolbar__actions">
              <Button size="sm" variant="primary" loading={frpBusy} disabled={!frpToken.trim()} onClick={saveFrpToken}>
                验证并保存
              </Button>
              <Button
                size="sm"
                variant="ghost"
                disabled={!frpConfigured || frpBusy}
                onClick={clearFrpToken}
              >
                退出账户
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() =>
                  void api
                    .openTrustedUrl("https://www.52frp.com/user/#/user-center")
                    .catch((e) =>
                      pushToast(
                        "无法打开 52FRP 个人中心",
                        "warning",
                        e instanceof Error ? e.message : undefined,
                      ),
                    )
                }
              >
                打开令牌页面
              </Button>
            </div>
            {frpMessage ? <p className="muted small">{frpMessage}</p> : null}
          </Card>

          <Card>
            <div className="card-head">
              <div>
                <h3>CurseForge API</h3>
                <p>
                  搜索 CurseForge 模组 / 整合包 / 光影 / 材质 / 地图，以及解析 CurseForge
                  `manifest.zip` 整合包。默认使用随启动器发布的内置公共 key，开箱即用。
                </p>
              </div>
              <Badge tone="success">
                {cfSource === "custom" ? "个人 key" : "内置公共 key"}
              </Badge>
            </div>
            <Field
              label="个人 API key（可选）"
              hint="默认使用内置公共 key，搜索时不会访问系统钥匙串。只有保存个人 key 后才会读写钥匙串（macOS 可能会询问一次权限）。个人 key 只保存在系统凭据库，不写入 settings.json，也不会出现在日志中。"
            >
              <input
                type="password"
                value={cfKey}
                onChange={(e) => setCfKey(e.target.value)}
                placeholder={
                  cfSource === "custom"
                    ? "已配置个人 key，输入新 key 可替换"
                    : "可选：粘贴自己的 CurseForge API key"
                }
                autoComplete="off"
              />
            </Field>
            <div className="toolbar__actions">
              <Button size="sm" loading={cfBusy} disabled={!cfKey.trim()} onClick={saveCurseForgeKey}>
                保存 key
              </Button>
              <Button size="sm" variant="ghost" loading={cfBusy} onClick={testCurseForgeKey}>
                测试连接
              </Button>
              <Button
                size="sm"
                variant="ghost"
                disabled={cfSource !== "custom" || cfBusy}
                onClick={clearCurseForgeKey}
              >
                清除个人 key
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() =>
                  void api
                    .openTrustedUrl("https://console.curseforge.com/")
                    .catch((e) =>
                      pushToast(
                        "无法打开申请页",
                        "warning",
                        e instanceof Error ? e.message : undefined,
                      ),
                    )
                }
              >
                申请地址
              </Button>
            </div>
            {cfMessage ? <p className="muted small">{cfMessage}</p> : null}
          </Card>

          <Card>
            <div className="card-head">
              <div>
                <h3>鸦鸭 AI 助手（DeepSeek）</h3>
                <p>
                  API key 只保存在系统凭据库；模型与地址写入本地设置。未配置时助手仍可做本地日志分析。
                </p>
              </div>
              <Badge tone={dsConfigured ? "success" : "warning"}>
                {dsConfigured ? "已配置" : "未配置"}
              </Badge>
            </div>
            <div className="form-grid">
              <Field label="模型">
                <input
                  value={form.assistantModel}
                  onChange={(e) => patch({ assistantModel: e.target.value })}
                  placeholder="deepseek-chat"
                />
              </Field>
              <Field label="API 地址" hint="必须是 https；默认 https://api.deepseek.com">
                <input
                  value={form.assistantBaseUrl}
                  onChange={(e) => patch({ assistantBaseUrl: e.target.value })}
                  placeholder="https://api.deepseek.com"
                  className="mono"
                />
              </Field>
            </div>
            <Field
              label="DeepSeek API key"
              hint="只保存在系统凭据库，不写入 settings.json，也不会出现在日志中。macOS 可能询问一次钥匙串权限。"
            >
              <input
                type="password"
                value={dsKey}
                onChange={(e) => setDsKey(e.target.value)}
                placeholder={dsConfigured ? "已配置，输入新 key 可替换" : "粘贴 DeepSeek API key"}
                autoComplete="off"
              />
            </Field>
            <div className="toolbar__actions">
              <Button size="sm" loading={dsBusy} disabled={!dsKey.trim()} onClick={saveDeepSeekKey}>
                保存 key
              </Button>
              <Button size="sm" variant="ghost" loading={dsBusy} onClick={testDeepSeekKey}>
                测试连接
              </Button>
              <Button
                size="sm"
                variant="ghost"
                disabled={!dsConfigured || dsBusy}
                onClick={clearDeepSeekKey}
              >
                清除
              </Button>
              <Button size="sm" variant="ghost" onClick={save} disabled={!dirty || saving}>
                保存模型配置
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() =>
                  void api
                    .openTrustedUrl("https://platform.deepseek.com/api_keys")
                    .catch((e) =>
                      pushToast(
                        "无法打开 DeepSeek 控制台",
                        "warning",
                        e instanceof Error ? e.message : undefined,
                      ),
                    )
                }
              >
                申请 key
              </Button>
            </div>
            {dsMessage ? <p className="muted small">{dsMessage}</p> : null}
          </Card>

          <Card>
            <div className="card-head">
              <div>
                <h3>数据与隐私</h3>
                <p>本地优先：不上传账号令牌、游戏日志或诊断报告。</p>
              </div>
              <Icon name="shield" size={20} />
            </div>
            <ul className="plain-list">
              <li>账号档案只保存用户名与本地 UUID，令牌仅存于系统凭据库（当前版本不写入令牌）。</li>
              <li>诊断报告在导出前会进行预览与脱敏。</li>
              <li>没有内置遥测；不会自动上传任何数据。</li>
            </ul>
            <Button size="sm" variant="ghost" icon="trash" onClick={clearTasks}>
              清除已完成的任务记录
            </Button>
          </Card>
        </div>

        <div className="settings-side">
          <Card>
            <h3>路径</h3>
            <div className="path-row">
              <span>应用数据</span>
              <code title={bootstrap.paths.dataDir}>{bootstrap.paths.dataDir}</code>
              <Button size="sm" variant="ghost" icon="copy" onClick={() => copy(bootstrap.paths.dataDir)}>
                复制
              </Button>
            </div>
            <div className="path-row">
              <span>游戏目录</span>
              <code title={bootstrap.paths.gameDir}>{bootstrap.paths.gameDir}</code>
              <Button size="sm" variant="ghost" icon="copy" onClick={() => copy(bootstrap.paths.gameDir)}>
                复制
              </Button>
            </div>
            <div className="path-row">
              <span>日志</span>
              <code title={bootstrap.paths.logsDir}>{bootstrap.paths.logsDir}</code>
              <Button size="sm" variant="ghost" icon="copy" onClick={() => copy(bootstrap.paths.logsDir)}>
                复制
              </Button>
            </div>
          </Card>


          <Card>
            <p className="about-signature">
              SRL (Structural Recomposition Launcher) V{bootstrap.appVersion} 结构重组
            </p>
          </Card>
        </div>
      </div>
    </div>
  );
}

