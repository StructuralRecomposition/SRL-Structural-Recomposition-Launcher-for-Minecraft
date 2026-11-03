//! DeepSeek-compatible assistant service and local, evidence-based diagnosis.
//!
//! The model never runs commands or writes files. This module only provides a
//! chat-completions client plus a deterministic report parser that the model can
//! cite. All network calls use a user-provided key stored in the OS keyring.

use crate::account::SecretStore;
use crate::error::{AppError, ErrorCode, Result};
use crate::instance::Instance;
use crate::mods;
use crate::util;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

pub const KEY_ENTRY: &str = "deepseek:api_key";
pub const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
pub const DEFAULT_MODEL: &str = "deepseek-chat";
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_LOG_READ_BYTES: u64 = 256 * 1024;

pub fn load_key(store: &dyn SecretStore) -> Result<Option<String>> {
    Ok(store
        .get(KEY_ENTRY)?
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty()))
}

pub fn save_key(store: &dyn SecretStore, key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() || key.len() > 512 || key.chars().any(|c| c.is_control()) {
        return Err(AppError::invalid_input("DeepSeek API key 格式非法"));
    }
    store.set(KEY_ENTRY, key)
}

pub fn clear_key(store: &dyn SecretStore) -> Result<()> {
    store.delete(KEY_ENTRY)
}

fn normalize_base_url(raw: &str) -> Result<String> {
    let base = raw.trim().trim_end_matches('/');
    let parsed =
        reqwest::Url::parse(base).map_err(|_| AppError::invalid_input("AI API 地址格式无效"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(AppError::invalid_input(
            "AI API 地址必须是 https 且不包含凭据",
        ));
    }
    Ok(base.to_string())
}

pub struct DeepSeekClient {
    http: reqwest::Client,
    key: String,
    base_url: String,
    model: String,
}

impl DeepSeekClient {
    pub fn new(http: reqwest::Client, key: String, base_url: &str, model: &str) -> Result<Self> {
        Ok(Self {
            http,
            key,
            base_url: normalize_base_url(base_url)?,
            model: model.trim().to_string(),
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    async fn get_models(&self) -> Result<()> {
        let url = format!("{}/models", self.base_url);
        let resp = self
            .http
            .get(url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.key),
            )
            .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else if resp.status() == reqwest::StatusCode::UNAUTHORIZED
            || resp.status() == reqwest::StatusCode::FORBIDDEN
        {
            Err(AppError::new(
                ErrorCode::AuthenticationRequired,
                "DeepSeek API key 无效或无权限",
            ))
        } else {
            Err(AppError::new(
                ErrorCode::Http,
                format!("DeepSeek 返回 HTTP {}", resp.status().as_u16()),
            ))
        }
    }

    pub async fn test(&self) -> Result<()> {
        self.get_models().await
    }

    /// One chat-completions round. `messages` and `tools` use the OpenAI
    /// compatible JSON shape; the assistant message value is returned as-is.
    pub async fn complete(&self, messages: &[Value], tools: &Value) -> Result<Value> {
        let body = json!({
            "model": self.model,
            "messages": messages,
            "tools": tools,
            "tool_choice": "auto",
            "temperature": 0.2,
            "max_tokens": 2048,
            "stream": false
        });
        let url = format!("{}/chat/completions", self.base_url);
        let mut last_error: Option<AppError> = None;
        for attempt in 0..2u32 {
            let resp = self
                .http
                .post(&url)
                .header(
                    reqwest::header::AUTHORIZATION,
                    format!("Bearer {}", self.key),
                )
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
                .timeout(Duration::from_secs(90))
                .json(&body)
                .send()
                .await;
            match resp {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let bytes = resp.bytes().await?;
                        if bytes.len() > MAX_RESPONSE_BYTES {
                            return Err(AppError::new(
                                ErrorCode::VersionInvalid,
                                "AI 返回数据异常过大",
                            ));
                        }
                        let raw: Value = serde_json::from_slice(&bytes).map_err(|e| {
                            AppError::new(ErrorCode::InvalidInput, "AI 返回数据格式无效")
                                .with_detail(e.to_string())
                        })?;
                        return raw.pointer("/choices/0/message").cloned().ok_or_else(|| {
                            AppError::new(ErrorCode::InvalidInput, "AI 返回缺少消息内容")
                        });
                    }
                    if status == reqwest::StatusCode::UNAUTHORIZED
                        || status == reqwest::StatusCode::FORBIDDEN
                    {
                        return Err(AppError::new(
                            ErrorCode::AuthenticationRequired,
                            "DeepSeek API key 无效或无权限",
                        ));
                    }
                    last_error = Some(AppError::new(
                        ErrorCode::Http,
                        format!("DeepSeek 返回 HTTP {}", status.as_u16()),
                    ));
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
                    {
                        last_error = last_error.map(|e| e.retryable(true));
                    } else {
                        break;
                    }
                }
                Err(error) => {
                    last_error = Some(AppError::from(error).with_detail("AI 请求失败"));
                }
            }
            if attempt == 0 {
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
        }
        Err(last_error.unwrap_or_else(|| AppError::network("无法连接 AI 服务")))
    }
}

// ---------------------------------------------------------------------------
// Local evidence collection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineEvidence {
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionEvidence {
    pub kind: String,
    pub message: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuspectMod {
    pub name: String,
    pub reason: String,
    pub evidence_line: Option<usize>,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashEvidence {
    pub file_name: String,
    pub size: u64,
    pub line_count: usize,
    pub description: Option<String>,
    pub minecraft_version: Option<String>,
    pub java_version: Option<String>,
    pub loader: Option<String>,
    pub crash_time: Option<String>,
    pub signals: Vec<String>,
    pub exceptions: Vec<ExceptionEvidence>,
    pub caused_by: Vec<LineEvidence>,
    pub frames: Vec<LineEvidence>,
    pub suspected_mods: Vec<SuspectMod>,
    pub mod_list: Vec<String>,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEvidence {
    pub file_name: String,
    pub line_count: usize,
    pub error_lines: Vec<LineEvidence>,
    pub error_context: Vec<LineEvidence>,
    pub mod_list: Vec<String>,
    pub signals: Vec<String>,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalReport {
    pub instance_id: String,
    pub instance_name: String,
    pub version_id: String,
    pub loader: Option<String>,
    pub memory_mb: u32,
    pub java_path: Option<String>,
    pub mods: Vec<String>,
    pub crash: Option<CrashEvidence>,
    pub log: Option<LogEvidence>,
    pub signals: Vec<String>,
    pub facts: Vec<String>,
    pub limits: Vec<String>,
}

fn field_value(lines: &[&str], key: &str) -> Option<String> {
    for line in lines {
        if let Some(rest) = line.trim().strip_prefix(key) {
            let value = rest.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn signals_in(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut add = |needle: &str, label: &str| {
        if lower.contains(needle) && !out.iter().any(|s: &String| s == label) {
            out.push(label.to_string());
        }
    };
    add("outofmemoryerror", "Java 内存不足（OutOfMemoryError）");
    add(
        "unsupportedclassversionerror",
        "Java 版本不匹配（UnsupportedClassVersionError）",
    );
    add(
        "class file version",
        "Java 版本不匹配（class file version）",
    );
    add("mixin apply failed", "Mixin 注入失败");
    add("mixinapplyerror", "Mixin 注入失败");
    add("mixintransformererror", "Mixin 注入失败");
    add("duplicate mod", "重复模组");
    add("duplicate mods", "重复模组");
    add("missing or unsupported mandatory dependencies", "缺失依赖");
    add("missing dependency", "缺失依赖");
    add("nosuchmethoderror", "API 不兼容（NoSuchMethodError）");
    add("noclassdeffounderror", "缺少类（NoClassDefFoundError）");
    add("classnotfoundexception", "缺少类（ClassNotFoundException）");
    add("exception_access_violation", "JVM 崩溃（ACCESS_VIOLATION）");
    add("glfw error", "图形 / GLFW 错误");
    add("opengl", "图形 / OpenGL 错误");
    add("exception in server tick loop", "服务端 tick 异常");
    add("corrupt", "存档 / 区块可能损坏");
    add("level.dat", "世界数据（level.dat）问题");
    add("registry", "注册表 / 数据包错误");
    add("failed to load", "加载失败");
    out
}

fn is_exception_line(line: &str) -> bool {
    let text = line.trim();
    !text.starts_with("Caused by:")
        && (text.contains("Exception:")
            || text.contains("Error:")
            || text.contains("Exception ")
            || text.ends_with("Error"))
        && text
            .split([' ', ':'])
            .next()
            .map(|name| name.contains('.'))
            .unwrap_or(false)
}

fn is_frame_line(line: &str) -> bool {
    let text = line.trim();
    text.starts_with("at ") && text.contains('(') && text.contains(')')
}

pub fn parse_crash_text(file_name: &str, size: u64, text: &str) -> CrashEvidence {
    let lines: Vec<&str> = text.lines().collect();
    let mut exceptions = Vec::new();
    let mut caused_by = Vec::new();
    let mut frames = Vec::new();
    let mut mod_list = Vec::new();
    let mut in_mod_list = false;

    for (index, raw) in lines.iter().enumerate() {
        let line_no = index + 1;
        let line = raw.trim();
        if is_exception_line(line) {
            let (kind, message) = line
                .split_once(':')
                .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
                .unwrap_or_else(|| (line.to_string(), String::new()));
            exceptions.push(ExceptionEvidence {
                kind,
                message,
                line: line_no,
            });
        } else if line.starts_with("Caused by:") {
            caused_by.push(LineEvidence {
                line: line_no,
                text: line.to_string(),
            });
        } else if is_frame_line(line) && frames.len() < 60 {
            frames.push(LineEvidence {
                line: line_no,
                text: line.to_string(),
            });
        }

        let lower = line.to_ascii_lowercase();
        if lower.starts_with("mod list:") || lower.starts_with("-- mod list --") {
            in_mod_list = true;
            continue;
        }
        if in_mod_list {
            if line.is_empty() || line.starts_with("--") {
                in_mod_list = false;
            } else if mod_list.len() < 400
                && (line.contains(".jar)") || line.starts_with("- ") || raw.starts_with('\t'))
            {
                let name = line.strip_prefix("- ").unwrap_or(line).trim();
                if !name.is_empty() {
                    mod_list.push(name.to_string());
                }
            }
        }
    }

    let crash_time = field_value(&lines, "Time:").or_else(|| field_value(&lines, "Crash time:"));
    let signals = signals_in(text);

    let description = field_value(&lines, "Description:");
    let minecraft_version = field_value(&lines, "Minecraft Version:")
        .or_else(|| field_value(&lines, "Minecraft version:"));
    let java_version =
        field_value(&lines, "Java Version:").or_else(|| field_value(&lines, "Java version:"));
    let loader = field_value(&lines, "ModLauncher:")
        .or_else(|| field_value(&lines, "FML:"))
        .or_else(|| field_value(&lines, "Fabric Loader:"))
        .or_else(|| field_value(&lines, "Quilt Loader:"));

    // A mod is only a suspect when its file/id appears in an exception or frame
    // line; mere presence in the mod list is not evidence.
    let mut suspected = Vec::new();
    for (index, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        if !(is_exception_line(line) || is_frame_line(line) || line.starts_with("Caused by:")) {
            continue;
        }
        let haystack = line.to_ascii_lowercase().replace('.', "");
        for name in &mod_list {
            let needle = name
                .split_whitespace()
                .next()
                .unwrap_or(name)
                .to_ascii_lowercase()
                .replace('.', "");
            if needle.len() < 4 || !haystack.contains(&needle) {
                continue;
            }
            if suspected
                .iter()
                .any(|s: &SuspectMod| s.name.eq_ignore_ascii_case(name))
            {
                continue;
            }
            suspected.push(SuspectMod {
                name: name.clone(),
                reason: "模组名称出现在异常或堆栈行".into(),
                evidence_line: Some(index + 1),
                evidence: line.to_string(),
            });
            if suspected.len() >= 12 {
                break;
            }
        }
        if suspected.len() >= 12 {
            break;
        }
    }

    let excerpt = lines
        .iter()
        .take(80)
        .map(|l| util::redact_secrets(l))
        .collect::<Vec<_>>()
        .join("\n");

    CrashEvidence {
        file_name: file_name.to_string(),
        size,
        line_count: lines.len(),
        description,
        minecraft_version,
        java_version,
        loader,
        crash_time,
        signals,
        exceptions,
        caused_by,
        frames,
        suspected_mods: suspected,
        mod_list,
        excerpt,
    }
}

async fn read_crash(instance: &Instance, file_name: &str) -> Result<CrashEvidence> {
    if file_name.is_empty()
        || file_name.contains('/')
        || file_name.contains('\\')
        || !file_name.ends_with(".txt")
    {
        return Err(AppError::path_unsafe("崩溃报告文件名非法"));
    }
    let dir = instance.game_dir.join("crash-reports");
    let path = dir.join(file_name);
    let canonical = tokio::fs::canonicalize(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("崩溃报告不存在")
        } else {
            e.into()
        }
    })?;
    let root = tokio::fs::canonicalize(&dir)
        .await
        .unwrap_or_else(|_| dir.clone());
    if !canonical.starts_with(&root) {
        return Err(AppError::path_unsafe("拒绝读取崩溃目录之外的文件"));
    }
    let meta = tokio::fs::metadata(&canonical).await?;
    if !meta.is_file() || meta.len() > 8 * 1024 * 1024 {
        return Err(AppError::invalid_input("崩溃报告不是可解析的普通文件"));
    }
    let text = tokio::fs::read_to_string(&canonical).await?;
    Ok(parse_crash_text(file_name, meta.len(), &text))
}

pub async fn read_log(instance: &Instance, file_name: &str) -> Result<Option<LogEvidence>> {
    if !matches!(file_name, "latest.log" | "debug.log") {
        return Err(AppError::invalid_input(
            "只允许读取 latest.log 或 debug.log",
        ));
    }
    let path = instance.game_dir.join("logs").join(file_name);
    let meta = match tokio::fs::metadata(&path).await {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !meta.is_file() {
        return Ok(None);
    }
    let start = meta.len().saturating_sub(MAX_LOG_READ_BYTES);
    let bytes = {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        let mut file = tokio::fs::File::open(&path).await?;
        if start > 0 {
            file.seek(std::io::SeekFrom::Start(start)).await?;
        }
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await?;
        buf
    };
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let mut error_indices = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let lower = line.to_ascii_lowercase();
        if line.contains("ERROR")
            || line.contains("Exception")
            || lower.contains("failed")
            || lower.contains("caused by")
        {
            error_indices.push(index);
            if error_indices.len() >= 80 {
                break;
            }
        }
    }
    let error_lines = error_indices
        .iter()
        .map(|index| LineEvidence {
            line: index + 1,
            text: util::redact_secrets(lines[*index]),
        })
        .collect::<Vec<_>>();
    let mut context_indices = std::collections::BTreeSet::new();
    for index in &error_indices {
        let start = index.saturating_sub(2);
        let end = (*index + 2).min(lines.len().saturating_sub(1));
        for i in start..=end {
            context_indices.insert(i);
            if context_indices.len() >= 160 {
                break;
            }
        }
    }
    let error_context = context_indices
        .into_iter()
        .map(|index| LineEvidence {
            line: index + 1,
            text: util::redact_secrets(lines[index]),
        })
        .collect::<Vec<_>>();
    let mut mod_list = Vec::new();
    let mut in_mods = false;
    for line in lines.iter() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("loading") && lower.contains("mods") {
            in_mods = true;
            continue;
        }
        if !in_mods {
            continue;
        }
        let content = line
            .split_once("]: ")
            .map(|(_, b)| b)
            .unwrap_or(line)
            .trim();
        if let Some(rest) = content.strip_prefix("- ") {
            if rest.len() < 200 && mod_list.len() < 400 {
                mod_list.push(rest.to_string());
            }
        } else if !content.is_empty() && !content.starts_with('-') && !content.starts_with('\t') {
            in_mods = false;
        }
    }
    let signals = signals_in(&text);
    let excerpt = lines
        .iter()
        .rev()
        .take(120)
        .rev()
        .map(|l| util::redact_secrets(l))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some(LogEvidence {
        file_name: file_name.to_string(),
        line_count: lines.len(),
        error_lines,
        error_context,
        mod_list,
        signals,
        excerpt,
    }))
}

pub async fn local_report(
    instance: &Instance,
    crash_file: Option<&str>,
    include_log: bool,
) -> Result<LocalReport> {
    let mut facts = Vec::new();
    let mut limits = Vec::new();
    let crash = match crash_file {
        Some(name) => Some(read_crash(instance, name).await?),
        None => {
            let reports = crate::crash::list(instance).await?;
            match reports.first() {
                Some(report) => Some(read_crash(instance, &report.file_name).await?),
                None => {
                    limits.push("该实例没有 crash-reports/*.txt".into());
                    None
                }
            }
        }
    };
    let log = if include_log {
        match read_log(instance, "latest.log").await? {
            Some(log) => Some(log),
            None => read_log(instance, "debug.log").await?,
        }
    } else {
        None
    };
    let mod_entries = mods::list(instance).await.unwrap_or_default();
    let mods: Vec<String> = mod_entries.iter().map(|m| m.file_name.clone()).collect();

    facts.push(format!(
        "实例 {} 使用版本 {}",
        instance.name, instance.version_id
    ));
    facts.push(format!("mods/ 目录中有 {} 个文件", mods.len()));
    let mut signals: Vec<String> = Vec::new();
    if let Some(crash) = &crash {
        facts.push(format!(
            "崩溃报告 {} 共 {} 行，提取到 {} 个异常、{} 条 Caused by、{} 条栈帧",
            crash.file_name,
            crash.line_count,
            crash.exceptions.len(),
            crash.caused_by.len(),
            crash.frames.len()
        ));
        if let Some(desc) = &crash.description {
            facts.push(format!("崩溃描述：{desc}"));
        }
        if let Some(mc) = &crash.minecraft_version {
            facts.push(format!("崩溃报告记录的 Minecraft 版本：{mc}"));
        }
        if let Some(java) = &crash.java_version {
            facts.push(format!("崩溃报告记录的 Java：{java}"));
        }
        if let Some(time) = &crash.crash_time {
            facts.push(format!("崩溃时间：{time}"));
        }
        if !crash.mod_list.is_empty() {
            facts.push(format!("崩溃报告列出 {} 个模组", crash.mod_list.len()));
        }
        signals.extend(crash.signals.clone());
        if crash.exceptions.is_empty() && crash.caused_by.is_empty() {
            limits.push("崩溃报告中没有识别到标准 Java 异常行".into());
        }
    }
    if let Some(log) = &log {
        facts.push(format!(
            "{} 最后窗口 {} 行，提取到 {} 条错误/异常行、{} 行上下文",
            log.file_name,
            log.line_count,
            log.error_lines.len(),
            log.error_context.len()
        ));
        if !log.mod_list.is_empty() {
            facts.push(format!("日志中识别到 {} 个已加载模组", log.mod_list.len()));
        }
        signals.extend(log.signals.clone());
    }
    signals.sort();
    signals.dedup();
    if !signals.is_empty() {
        facts.push(format!("检测到的故障信号：{}", signals.join("；")));
    }
    limits.push("本地解析只引用原始行；不推断未出现在日志中的原因".into());
    limits.push("日志只读取最后 256KB；启动器控制台的完整历史不在实例日志中".into());
    if crash
        .as_ref()
        .map(|c| c.suspected_mods.is_empty())
        .unwrap_or(false)
    {
        limits.push("没有模组名称出现在异常/堆栈行中，不能据此确定模组根因".into());
    }

    Ok(LocalReport {
        instance_id: instance.id.clone(),
        instance_name: instance.name.clone(),
        version_id: instance.version_id.clone(),
        loader: instance
            .mod_loader
            .as_ref()
            .map(|m| format!("{} {}", m.kind, m.version)),
        memory_mb: instance.memory_mb,
        java_path: instance.java_path.as_ref().map(|p| p.display().to_string()),
        mods,
        crash,
        log,
        signals,
        facts,
        limits,
    })
}

pub async fn append_audit(path: &Path, entry: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        util::ensure_dir(parent).await?;
    }
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    let line = serde_json::to_string(entry)?;
    file.write_all(line.as_bytes()).await?;
    file.write_all(b"\n").await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"---- Minecraft Crash Report ----
// Shall we play a game?

Time: 2026-10-26 12:00:00
Description: Rendering overlay

java.lang.NoSuchMethodError: net.minecraft.client.renderer.GameRenderer.m_12345()V
	at com.example.mod.Renderer.render(Renderer.java:42)
	at net.minecraft.client.Minecraft.run(Minecraft.java:1000)
Caused by: java.lang.NullPointerException: Cannot invoke "Object.toString()" because "x" is null
	at com.example.mod.Renderer.render(Renderer.java:45)

A detailed walkthrough of the error, its code path and all known details is as follows:

-- System Details --
Minecraft Version: 1.21.1
Java Version: 21.0.9, Eclipse Adoptium
Mod List:
	- examplemod 1.0 (examplemod.jar)
	- othermod 2.0 (othermod.jar)
"#;

    #[test]
    fn parses_exceptions_with_line_numbers() {
        let parsed = parse_crash_text("crash.txt", SAMPLE.len() as u64, SAMPLE);
        assert_eq!(parsed.description.as_deref(), Some("Rendering overlay"));
        assert_eq!(parsed.minecraft_version.as_deref(), Some("1.21.1"));
        assert!(parsed
            .exceptions
            .iter()
            .any(|e| e.kind.contains("NoSuchMethodError") && e.line > 0));
        assert!(parsed
            .caused_by
            .iter()
            .any(|e| e.text.contains("NullPointerException")));
        assert!(parsed
            .frames
            .iter()
            .any(|f| f.line == 8 && f.text.contains("Renderer")));
        assert!(parsed.mod_list.iter().any(|m| m.starts_with("examplemod")));
        assert!(!parsed.suspected_mods.is_empty());
        assert!(parsed
            .suspected_mods
            .iter()
            .all(|s| s.evidence_line.is_some()));
    }

    #[test]
    fn unknown_crash_has_no_fabricated_cause() {
        let parsed = parse_crash_text("plain.txt", 10, "hello\nworld\n");
        assert!(parsed.exceptions.is_empty());
        assert!(parsed.suspected_mods.is_empty());
    }

    #[tokio::test]
    async fn audit_appends_json_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");
        append_audit(&path, &serde_json::json!({"tool":"x"}))
            .await
            .unwrap();
        append_audit(&path, &serde_json::json!({"tool":"y"}))
            .await
            .unwrap();
        let text = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(text.lines().count(), 2);
    }
}
