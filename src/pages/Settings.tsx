import { useEffect, useState } from "react";
import { api, type Settings } from "../lib/api";
import { useApp } from "../state/AppContext";
import { Badge, Button, Card, Field, Toggle } from "../components/ui";
import { normalizeLanguage, setLanguage } from "../lib/i18n";
import { t } from "../lib/i18n";


export function SettingsPage() {
  const {
    bootstrap,
    updateBootstrap,
    pushToast,
    refreshTasks,
    theme,
    setTheme,
    updateInfo,
    updateChecking,
    checkForUpdates,
  } = useApp();
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
      setCfMessage(t("个人 API key 已保存到系统凭据库，将优先于内置公共 key 使用"));
      pushToast(t("CurseForge API key 已保存"), "success");
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
      setCfMessage(t("连接成功，key 可用"));
      pushToast(t("CurseForge 连接成功"), "success");
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
      setDsMessage(t("DeepSeek key 已保存到系统凭据库"));
      pushToast(t("DeepSeek key 已保存"), "success");
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
      setDsMessage(t("连接成功，key 可用"));
      pushToast(t("DeepSeek 连接成功"), "success");
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
      setDsMessage(t("已清除个人 DeepSeek key"));
      pushToast(t("已清除 DeepSeek key"), "info");
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
      setCfMessage(t("已清除个人 key，恢复使用内置公共 key"));
      pushToast(t("已清除 CurseForge API key"), "info");
    } catch (e) {
      setCfMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setCfBusy(false);
    }
  }

  if (!bootstrap || !form) {
    return (
      <div className="page">
        <p className="muted">{t("正在加载设置…")}</p>
      </div>
    );
  }

  const dirty = JSON.stringify(form) !== JSON.stringify(bootstrap.settings);

  function patch(p: Partial<Settings>) {
    setForm((prev) => (prev ? { ...prev, ...p } : prev));
  }

  async function save() {
    if (!form || !bootstrap) return;
    setError(null);
    setSaving(true);
    try {
      const languageChanged =
        normalizeLanguage(form.language) !== normalizeLanguage(bootstrap.settings.language);
      const saved = await api.updateSettings(form);
      updateBootstrap((b) => ({ ...b, settings: saved }));
      setForm(saved);
      if (languageChanged) {
        setLanguage(normalizeLanguage(saved.language));
        window.location.reload();
        return;
      }
      pushToast(t("设置已保存"), "success");
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
      setFrpMessage(t("52FRP API 令牌已验证并保存到系统凭据库"));
      pushToast(t("52FRP 已连接"), "success");
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
      setFrpMessage(t("已清除 52FRP API 令牌"));
      pushToast(t("已退出 52FRP"), "info");
    } catch (e) {
      setFrpMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setFrpBusy(false);
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      pushToast(t("已复制到剪贴板"), "success");
    } catch {
      pushToast(t("复制失败"), "warning", t("当前环境不允许访问剪贴板"));
    }
  }

  async function clearTasks() {
    try {
      await api.clearFinishedTasks();
      await refreshTasks();
      pushToast(t("已清除结束的任务"), "info");
    } catch (e) {
      pushToast(t("清除失败"), "error", e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <div className="page settings-page">
      <div className="toolbar">
        <div>
          <h2>{t("设置")}</h2>
        </div>
        <div className="toolbar__actions">
          <Button variant="ghost" onClick={() => setForm(bootstrap.settings)} disabled={!dirty || saving}>
            {t("还原")}</Button>
          <Button variant="primary" onClick={save} loading={saving} disabled={!dirty}>
            {t("保存设置")}</Button>
        </div>
      </div>

      {error ? <p className="form-error">{error}</p> : null}

      <div className="settings-single">
        <Card>
          <div className="card-head">
            <div>
              <h3>{t("游戏与下载")}</h3>
              <p>{t("版本、库、资源与实例都存放在游戏目录下。")}</p>
            </div>
          </div>
          <Field
            label={t("游戏目录")}
            hint={t("更改后，新的安装与实例使用新目录；已有实例保留其原始路径。")}
          >
            <input
              className="mono"
              value={form.gameDir}
              onChange={(e) => patch({ gameDir: e.target.value })}
            />
          </Field>
          <div className="form-grid">
            <Field label={t("并发下载数")} hint={t("1 - 16，当前建议 {0} 个连接", [form.maxConcurrentDownloads])}>
              <input
                type="number"
                min={1}
                max={16}
                value={form.maxConcurrentDownloads}
                onChange={(e) => patch({ maxConcurrentDownloads: Number(e.target.value) })}
              />
            </Field>
            <Field label={t("界面主题")} hint={t("主题立即预览，点击保存后持久化。")}>
              <select
                value={theme}
                onChange={(e) => {
                  const next = e.target.value as "dark" | "light";
                  setTheme(next);
                  patch({ theme: next });
                }}
              >
                <option value="dark">{t("深色")}</option>
                <option value="light">{t("浅色")}</option>
              </select>
            </Field>
          </div>
          <Field label={t("界面语言")} hint={t("默认简体中文；切换后界面会重新加载。")}>
            <select
              value={normalizeLanguage(form.language)}
              onChange={(e) => patch({ language: e.target.value })}
            >
              <option value="zh-CN">简体中文</option>
              <option value="en">English</option>
            </select>
          </Field>
          <details className="settings-disclosure settings-disclosure--inline">
            <summary>
              <span className="settings-disclosure__title">{t("下载与皮肤")}</span>
              <span className="settings-disclosure__hint">{t("资源文件 · 离线皮肤")}</span>
            </summary>
            <div className="settings-disclosure__body">
              <Toggle
                id="assets-default"
                checked={form.downloadAssetsOnInstall}
                onChange={(v) => patch({ downloadAssetsOnInstall: v })}
                label={t("安装版本时默认下载资源文件")}
                description={t("资源文件约数百 MB，但缺少资源会影响声音与贴图。安装时可逐次覆盖此选项。")}
              />
              <Toggle
                id="offline-skin"
                checked={form.applyOfflineSkin}
                onChange={(v) => patch({ applyOfflineSkin: v })}
                label={t("为离线账号应用本地皮肤")}
                description={t("启动时把所选皮肤写入实例的 resourcepacks 并启用本地资源包。仅本机可见，会覆盖默认皮肤贴图，无法改变手臂模型。")}
              />
            </div>
          </details>
          <div className="update-row">
            <div>
              <strong>{t("启动器更新")}</strong>
              <p>
                {t("当前版本")} v{bootstrap.appVersion}
                {updateInfo?.available
                  ? ` · ${t("发现新版本")} v${updateInfo.latestVersion}`
                  : updateInfo
                    ? ` · ${t("已是最新版本")}`
                    : ` · ${t("每次启动自动检查 GitHub Release")}`}
              </p>
            </div>
            {updateInfo?.available ? <Badge tone="accent">{t("可更新")}</Badge> : null}
            <Button
              size="sm"
              icon="refresh"
              loading={updateChecking}
              onClick={() => void checkForUpdates(true)}
            >
              {t("检查更新")}</Button>
          </div>
        </Card>

        <Card className="settings-collapsible">
          <details className="settings-disclosure">
            <summary>
              <span className="settings-disclosure__title">{t("网络与来源")}</span>
              <Badge tone="success">{t("官方源 + 国内镜像")}</Badge>
            </summary>
            <div className="settings-disclosure__body">
              <p className="settings-disclosure__desc">
                {t("默认只信任官方 HTTPS 域名；镜像仅改变下载地址，不绕过校验。")}</p>
              <Toggle
                id="use-mirror"
                checked={form.useMirror}
                onChange={(v) => patch({ useMirror: v })}
                label={t("优先使用国内镜像（BMCLAPI / TUNA）")}
                description={t("Fabric、Maven 与 Temurin 下载优先走国内镜像，失败时自动回退官方源；SHA-1 / SHA-256 校验不变。")}
              />
              <Field
                label={t("Microsoft 登录 client id")}
                hint={t("Azure 应用的公开客户端 ID（GUID）。留空则保持登录禁用；这不是密钥。")}
              >
                <input
                  className="mono"
                  value={form.microsoftClientId ?? ""}
                  onChange={(e) => patch({ microsoftClientId: e.target.value.trim() || null })}
                  placeholder="00000000-0000-0000-0000-000000000000"
                />
              </Field>
            </div>
          </details>
        </Card>

        <Card className="settings-integrations">
          <div className="card-head">
            <div>
              <h3>{t("API 与集成")}</h3>
              <p>{t("按需展开配置。未配置时，启动器仍可正常使用本地功能。")}</p>
            </div>
          </div>

          <details className="settings-disclosure">
            <summary>
              <span className="settings-disclosure__title">
                {t("Minecraft 联机")} <small>52FRP</small>
              </span>
              <Badge tone={frpConfigured ? "success" : "neutral"}>
                {frpConfigured ? t("已连接") : t("未连接")}
              </Badge>
            </summary>
            <div className="settings-disclosure__body">
              <p className="settings-disclosure__desc">
                {t("使用 52FRP API 访问令牌连接账户；令牌只保存在系统凭据库。")}</p>
              <Field
                label={t("52FRP API 访问令牌")}
                hint={t("在 52FRP 个人中心创建（以 hz_ 开头，只显示一次）。不要填账号密码或 FRPC token。")}
              >
                <input
                  type="password"
                  value={frpToken}
                  onChange={(e) => setFrpToken(e.target.value)}
                  placeholder={frpConfigured ? t("已保存令牌，输入新令牌可替换") : "hz_..."}
                  autoComplete="off"
                />
              </Field>
              <div className="toolbar__actions">
                <Button size="sm" variant="primary" loading={frpBusy} disabled={!frpToken.trim()} onClick={saveFrpToken}>
                  {t("验证并保存")}</Button>
                <Button size="sm" variant="ghost" disabled={!frpConfigured || frpBusy} onClick={clearFrpToken}>
                  {t("退出账户")}</Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() =>
                    void api
                      .openTrustedUrl("https://www.52frp.com/user/#/user-center")
                      .catch((e) =>
                        pushToast(
                          t("无法打开 52FRP 个人中心"),
                          "warning",
                          e instanceof Error ? e.message : undefined,
                        ),
                      )
                  }
                >
                  {t("打开令牌页面")}</Button>
              </div>
              {frpMessage ? <p className="muted small">{frpMessage}</p> : null}
            </div>
          </details>

          <details className="settings-disclosure">
            <summary>
              <span className="settings-disclosure__title">
                CurseForge <small>{t("模组与整合包")}</small>
              </span>
              <Badge tone={cfSource === "custom" ? "success" : "neutral"}>
                {cfSource === "custom" ? t("个人 key") : t("内置公共 key")}
              </Badge>
            </summary>
            <div className="settings-disclosure__body">
              <p className="settings-disclosure__desc">
                {t("默认使用内置公共 key，开箱即用。个人 key 只保存在系统凭据库。")}</p>
              <Field
                label={t("个人 API key（可选）")}
                hint={t("只有保存个人 key 后才会读写钥匙串（macOS 可能询问一次权限）。")}
              >
                <input
                  type="password"
                  value={cfKey}
                  onChange={(e) => setCfKey(e.target.value)}
                  placeholder={
                    cfSource === "custom"
                      ? t("已配置个人 key，输入新 key 可替换")
                      : t("可选：粘贴自己的 CurseForge API key")
                  }
                  autoComplete="off"
                />
              </Field>
              <div className="toolbar__actions">
                <Button size="sm" loading={cfBusy} disabled={!cfKey.trim()} onClick={saveCurseForgeKey}>
                  {t("保存 key")}</Button>
                <Button size="sm" variant="ghost" loading={cfBusy} onClick={testCurseForgeKey}>
                  {t("测试连接")}</Button>
                <Button size="sm" variant="ghost" disabled={cfSource !== "custom" || cfBusy} onClick={clearCurseForgeKey}>
                  {t("清除个人 key")}</Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() =>
                    void api
                      .openTrustedUrl("https://console.curseforge.com/")
                      .catch((e) =>
                        pushToast(
                          t("无法打开申请页"),
                          "warning",
                          e instanceof Error ? e.message : undefined,
                        ),
                      )
                  }
                >
                  {t("申请地址")}</Button>
              </div>
              {cfMessage ? <p className="muted small">{cfMessage}</p> : null}
            </div>
          </details>

          <details className="settings-disclosure">
            <summary>
              <span className="settings-disclosure__title">
                {t("鸦鸭 AI 助手")} <small>DeepSeek</small>
              </span>
              <Badge tone={dsConfigured ? "success" : "neutral"}>
                {dsConfigured ? t("已配置") : t("未配置")}
              </Badge>
            </summary>
            <div className="settings-disclosure__body">
              <p className="settings-disclosure__desc">
                {t("API key 只保存在系统凭据库；模型与地址写入本地设置。")}</p>
              <div className="form-grid">
                <Field label={t("模型")}>
                  <input
                    value={form.assistantModel}
                    onChange={(e) => patch({ assistantModel: e.target.value })}
                    placeholder="deepseek-chat"
                  />
                </Field>
                <Field label={t("API 地址")} hint={t("必须是 https；默认 https://api.deepseek.com")}>
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
                hint={t("只保存在系统凭据库，不写入 settings.json，也不会出现在日志中。")}
              >
                <input
                  type="password"
                  value={dsKey}
                  onChange={(e) => setDsKey(e.target.value)}
                  placeholder={dsConfigured ? t("已配置，输入新 key 可替换") : t("粘贴 DeepSeek API key")}
                  autoComplete="off"
                />
              </Field>
              <div className="toolbar__actions">
                <Button size="sm" loading={dsBusy} disabled={!dsKey.trim()} onClick={saveDeepSeekKey}>
                  {t("保存 key")}</Button>
                <Button size="sm" variant="ghost" loading={dsBusy} onClick={testDeepSeekKey}>
                  {t("测试连接")}</Button>
                <Button size="sm" variant="ghost" disabled={!dsConfigured || dsBusy} onClick={clearDeepSeekKey}>
                  {t("清除")}</Button>
                <Button size="sm" variant="ghost" onClick={save} disabled={!dirty || saving}>
                  {t("保存模型配置")}</Button>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() =>
                    void api
                      .openTrustedUrl("https://platform.deepseek.com/api_keys")
                      .catch((e) =>
                        pushToast(
                          t("无法打开 DeepSeek 控制台"),
                          "warning",
                          e instanceof Error ? e.message : undefined,
                        ),
                      )
                  }
                >
                  {t("申请 key")}</Button>
              </div>
              {dsMessage ? <p className="muted small">{dsMessage}</p> : null}
            </div>
          </details>
        </Card>

        <Card className="settings-advanced">
          <details className="settings-disclosure">
            <summary>
              <span className="settings-disclosure__title">
                {t("数据与存储")}
              </span>
              <span className="settings-disclosure__hint">{t("本地优先 · 无遥测")}</span>
            </summary>
            <div className="settings-disclosure__body">
              <h4>{t("存储路径")}</h4>
              <div className="path-row">
                <span>{t("应用数据")}</span>
                <code title={bootstrap.paths.dataDir}>{bootstrap.paths.dataDir}</code>
                <Button size="sm" variant="ghost" icon="copy" onClick={() => copy(bootstrap.paths.dataDir)}>
                  {t("复制")}</Button>
              </div>
              <div className="path-row">
                <span>{t("游戏目录")}</span>
                <code title={bootstrap.paths.gameDir}>{bootstrap.paths.gameDir}</code>
                <Button size="sm" variant="ghost" icon="copy" onClick={() => copy(bootstrap.paths.gameDir)}>
                  {t("复制")}</Button>
              </div>
              <div className="path-row">
                <span>{t("日志")}</span>
                <code title={bootstrap.paths.logsDir}>{bootstrap.paths.logsDir}</code>
                <Button size="sm" variant="ghost" icon="copy" onClick={() => copy(bootstrap.paths.logsDir)}>
                  {t("复制")}</Button>
              </div>
              <h4>{t("隐私")}</h4>
              <ul className="plain-list">
                <li>{t("账号档案只保存用户名与本地 UUID，令牌仅存于系统凭据库。")}</li>
                <li>{t("诊断报告在导出前会进行预览与脱敏。")}</li>
                <li>{t("没有内置遥测；不会自动上传任何数据。")}</li>
              </ul>
              <div className="settings-advanced__foot">
                <Button size="sm" variant="ghost" icon="trash" onClick={clearTasks}>
                  {t("清除已完成的任务记录")}</Button>
                <span className="about-signature">
                  SRL {bootstrap.appVersion} · {t("结构重组")}</span>
              </div>
            </div>
          </details>
        </Card>
      </div>
    </div>
  );
}

