use crate::error::{AppError, Result};
use crate::java::{JavaCompatibility, JavaInfo};
use crate::paths::{AppPaths, GamePaths};
use crate::util;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckStatus {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    pub id: String,
    pub label: String,
    pub status: CheckStatus,
    pub detail: String,
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsReport {
    pub generated_at: String,
    pub platform: String,
    pub game_dir: PathBuf,
    pub checks: Vec<CheckItem>,
    pub java: Vec<JavaInfo>,
    pub installed_versions: usize,
    pub instances: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFileInfo {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub modified: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogCleanup {
    pub removed: usize,
    pub bytes: u64,
}

/// Retention policy for launcher logs: delete files older than two weeks, then
/// trim the oldest files until the directory is under 200 MB.
pub const LOG_RETENTION_DAYS: u64 = 14;
pub const LOG_MAX_TOTAL_BYTES: u64 = 200 * 1024 * 1024;
const LOG_MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

pub async fn run_checks(
    paths: &AppPaths,
    game: &GamePaths,
    java: &[JavaInfo],
    installed_versions: usize,
    instances: usize,
    network_ok: Option<bool>,
) -> DiagnosticsReport {
    let mut checks = Vec::new();

    checks.push(match tokio::fs::try_exists(game.root()).await {
        Ok(true) => CheckItem {
            id: "game_dir".into(),
            label: "游戏目录".into(),
            status: CheckStatus::Ok,
            detail: game.root().display().to_string(),
            fix: None,
        },
        _ => CheckItem {
            id: "game_dir".into(),
            label: "游戏目录".into(),
            status: CheckStatus::Error,
            detail: "游戏目录不存在或不可访问".into(),
            fix: Some("在设置中选择一个可写的目录".into()),
        },
    });

    // Write probe: catches read-only volumes and permission issues.
    let probe = game.root().join(".sr-write-test");
    let write_ok = async {
        util::ensure_dir(game.root()).await.ok()?;
        tokio::fs::write(&probe, b"ok").await.ok()?;
        tokio::fs::remove_file(&probe).await.ok()?;
        Some(())
    }
    .await
    .is_some();
    checks.push(CheckItem {
        id: "write".into(),
        label: "目录写入权限".into(),
        status: if write_ok {
            CheckStatus::Ok
        } else {
            CheckStatus::Error
        },
        detail: if write_ok {
            "可以创建和删除文件".into()
        } else {
            "无法写入游戏目录".into()
        },
        fix: if write_ok {
            None
        } else {
            Some("检查磁盘空间与目录权限，或更换游戏目录".into())
        },
    });

    checks.push(if java.is_empty() {
        CheckItem {
            id: "java".into(),
            label: "Java 环境".into(),
            status: CheckStatus::Error,
            detail: "未发现任何可用的 Java".into(),
            fix: Some(
                "安装 Java 21（或该版本要求的版本），或在设置中手动指定 java 可执行文件".into(),
            ),
        }
    } else {
        let newest = java.iter().max_by_key(|j| j.major).unwrap();
        CheckItem {
            id: "java".into(),
            label: "Java 环境".into(),
            status: CheckStatus::Ok,
            detail: format!(
                "发现 {} 个 Java，最高版本 {}（{}）",
                java.len(),
                newest.major,
                newest.path.display()
            ),
            fix: None,
        }
    });

    checks.push(CheckItem {
        id: "versions".into(),
        label: "已安装版本".into(),
        status: if installed_versions > 0 {
            CheckStatus::Ok
        } else {
            CheckStatus::Warning
        },
        detail: format!("{installed_versions} 个版本，{instances} 个实例"),
        fix: if installed_versions == 0 {
            Some("在版本管理中安装一个 Minecraft 版本".into())
        } else {
            None
        },
    });

    checks.push(match network_ok {
        Some(true) => CheckItem {
            id: "network".into(),
            label: "元数据网络".into(),
            status: CheckStatus::Ok,
            detail: "可以连接 Mojang 元数据服务器".into(),
            fix: None,
        },
        Some(false) => CheckItem {
            id: "network".into(),
            label: "元数据网络".into(),
            status: CheckStatus::Warning,
            detail: "无法连接 Mojang 元数据服务器；已安装版本仍可离线启动".into(),
            fix: Some("检查网络或代理设置；本地功能不受影响".into()),
        },
        None => CheckItem {
            id: "network".into(),
            label: "元数据网络".into(),
            status: CheckStatus::Warning,
            detail: "未执行网络检查".into(),
            fix: None,
        },
    });

    checks.push(CheckItem {
        id: "logs".into(),
        label: "日志目录".into(),
        status: if tokio::fs::try_exists(paths.logs_dir())
            .await
            .unwrap_or(false)
        {
            CheckStatus::Ok
        } else {
            CheckStatus::Warning
        },
        detail: paths.logs_dir().display().to_string(),
        fix: None,
    });

    DiagnosticsReport {
        generated_at: util::now_rfc3339(),
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        game_dir: game.root().to_path_buf(),
        checks,
        java: java.to_vec(),
        installed_versions,
        instances,
    }
}

pub fn java_compatibility_summary(required: u32, javas: &[JavaInfo]) -> CheckItem {
    let best = javas
        .iter()
        .filter(|j| j.major >= required)
        .min_by_key(|j| j.major);
    match best {
        Some(j) => CheckItem {
            id: "java_compat".into(),
            label: format!("Java 兼容性（需要 {required}+）"),
            status: if crate::java::check_compatibility(required, j.major).compatibility
                == JavaCompatibility::Compatible
            {
                CheckStatus::Ok
            } else {
                CheckStatus::Warning
            },
            detail: format!("将使用 Java {}（{}）", j.major, j.path.display()),
            fix: None,
        },
        None => CheckItem {
            id: "java_compat".into(),
            label: format!("Java 兼容性（需要 {required}+）"),
            status: CheckStatus::Error,
            detail: "没有满足版本要求的 Java".into(),
            fix: Some(format!("安装 Java {required} 或更高版本")),
        },
    }
}

pub async fn list_logs(paths: &AppPaths, limit: usize) -> Result<Vec<LogFileInfo>> {
    let mut out = Vec::new();
    let dirs = [paths.logs_dir(), paths.logs_dir().join("instances")];
    for dir in dirs {
        let mut rd = match tokio::fs::read_dir(&dir).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = rd.next_entry().await? {
            let meta = entry.metadata().await?;
            if !meta.is_file() {
                continue;
            }
            out.push(LogFileInfo {
                path: entry.path(),
                name: entry.file_name().to_string_lossy().into_owned(),
                size: meta.len(),
                modified: meta
                    .modified()
                    .ok()
                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()),
            });
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out.truncate(limit);
    Ok(out)
}

/// Read the tail of a log. The path must resolve under the launcher log dir.
pub async fn read_log_tail(paths: &AppPaths, path: &Path, max_lines: usize) -> Result<String> {
    let canonical = tokio::fs::canonicalize(path).await.map_err(|e| {
        AppError::new(
            crate::error::ErrorCode::Io,
            format!("无法读取日志：{}", path.display()),
        )
        .with_detail(e.to_string())
    })?;
    let root = tokio::fs::canonicalize(paths.logs_dir())
        .await
        .unwrap_or_else(|_| paths.logs_dir());
    if !canonical.starts_with(&root) {
        return Err(AppError::path_unsafe("拒绝读取日志目录之外的文件"));
    }
    // Only read the tail: giant logs must not be loaded into memory.
    let meta = tokio::fs::metadata(&canonical).await?;
    let start_offset = meta.len().saturating_sub(LOG_MAX_READ_BYTES);
    let content = if start_offset == 0 {
        let bytes = tokio::fs::read(&canonical).await?;
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        let mut file = tokio::fs::File::open(&canonical).await?;
        file.seek(std::io::SeekFrom::Start(start_offset)).await?;
        let mut bytes = Vec::with_capacity(LOG_MAX_READ_BYTES as usize);
        file.read_to_end(&mut bytes).await?;
        // Game logs may contain platform-encoded bytes; never fail a log view
        // because of one invalid UTF-8 sequence.
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // Drop the first (likely partial) line of the truncated window.
        text.split_once('\n')
            .map(|(_, rest)| {
                format!(
                    "…（日志过长，仅显示最后 {} MB）\n{rest}",
                    LOG_MAX_READ_BYTES / 1024 / 1024
                )
            })
            .unwrap_or(text)
    };
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    Ok(lines[start..]
        .iter()
        .map(|l| util::redact_secrets(l))
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Delete a single log after confirming it lives under the launcher log dir.
pub async fn delete_log(paths: &AppPaths, path: &Path) -> Result<()> {
    let canonical = tokio::fs::canonicalize(path).await.map_err(|e| {
        AppError::new(
            crate::error::ErrorCode::Io,
            format!("无法删除日志：{}", path.display()),
        )
        .with_detail(e.to_string())
    })?;
    let root = tokio::fs::canonicalize(paths.logs_dir())
        .await
        .unwrap_or_else(|_| paths.logs_dir());
    if !canonical.starts_with(&root) {
        return Err(AppError::path_unsafe("拒绝删除日志目录之外的文件"));
    }
    tokio::fs::remove_file(&canonical).await?;
    Ok(())
}

struct LogEntry {
    path: PathBuf,
    size: u64,
    modified: Option<std::time::SystemTime>,
}

async fn collect_logs(paths: &AppPaths) -> Result<Vec<LogEntry>> {
    let mut out = Vec::new();
    let dirs = [paths.logs_dir(), paths.logs_dir().join("instances")];
    for dir in dirs {
        let mut rd = match tokio::fs::read_dir(&dir).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = rd.next_entry().await? {
            let meta = match entry.metadata().await {
                Ok(m) if m.is_file() => m,
                _ => continue,
            };
            out.push(LogEntry {
                path: entry.path(),
                size: meta.len(),
                modified: meta.modified().ok(),
            });
        }
    }
    Ok(out)
}

/// Remove old logs and trim total size. Safety: only files under the launcher
/// log directory are touched, and files modified within the last hour are kept
/// during size-based trimming so an active session log is never deleted.
pub async fn cleanup_logs(
    paths: &AppPaths,
    max_age_days: u64,
    max_total_bytes: u64,
) -> Result<LogCleanup> {
    let cutoff = std::time::SystemTime::now().checked_sub(std::time::Duration::from_secs(
        max_age_days.saturating_mul(24 * 3600),
    ));
    let mut logs = collect_logs(paths).await?;
    let mut removed = 0usize;
    let mut bytes = 0u64;

    let mut remaining = Vec::new();
    for entry in logs.drain(..) {
        let old = cutoff.map(|c| entry.modified.map(|m| m < c).unwrap_or(false));
        if old == Some(true) {
            if tokio::fs::remove_file(&entry.path).await.is_ok() {
                removed += 1;
                bytes += entry.size;
            }
        } else {
            remaining.push(entry);
        }
    }

    let mut total: u64 = remaining.iter().map(|e| e.size).sum();
    if total > max_total_bytes {
        remaining.sort_by_key(|a| a.modified);
        let protect_after =
            std::time::SystemTime::now().checked_sub(std::time::Duration::from_secs(3600));
        for entry in &remaining {
            if total <= max_total_bytes {
                break;
            }
            let recent = protect_after
                .zip(entry.modified)
                .map(|(limit, m)| m > limit)
                .unwrap_or(false);
            if recent {
                continue;
            }
            if tokio::fs::remove_file(&entry.path).await.is_ok() {
                removed += 1;
                bytes += entry.size;
                total = total.saturating_sub(entry.size);
            }
        }
    }

    Ok(LogCleanup { removed, bytes })
}

/// Build a shareable diagnostics text. Redaction is applied to every line.
pub fn export_report(report: &DiagnosticsReport, extra: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("SR Launcher 诊断报告\n");
    out.push_str(&format!("生成时间: {}\n", report.generated_at));
    out.push_str(&format!("平台: {}\n", report.platform));
    out.push_str(&format!("游戏目录: {}\n", report.game_dir.display()));
    out.push_str(&format!(
        "版本数: {} / 实例数: {}\n\n",
        report.installed_versions, report.instances
    ));
    for check in &report.checks {
        out.push_str(&format!(
            "[{:?}] {}: {}\n",
            check.status, check.label, check.detail
        ));
        if let Some(fix) = &check.fix {
            out.push_str(&format!("  建议: {fix}\n"));
        }
    }
    out.push_str("\nJava:\n");
    for j in &report.java {
        out.push_str(&format!(
            "  {} | {} | {} | {}\n",
            j.major,
            j.version_string,
            j.path.display(),
            j.source
        ));
    }
    if let Some(extra) = extra {
        out.push_str("\n附加信息:\n");
        out.push_str(extra);
        out.push('\n');
    }
    util::redact_secrets(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn checks_report_core_items() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        let game = GamePaths::new(dir.path().join("mc"));
        paths.ensure_layout().await.unwrap();
        game.ensure_layout().await.unwrap();
        let report = run_checks(&paths, &game, &[], 0, 0, Some(false)).await;
        assert!(report
            .checks
            .iter()
            .any(|c| c.id == "game_dir" && c.status == CheckStatus::Ok));
        assert!(report
            .checks
            .iter()
            .any(|c| c.id == "write" && c.status == CheckStatus::Ok));
        assert!(report
            .checks
            .iter()
            .any(|c| c.id == "java" && c.status == CheckStatus::Error));
        assert!(report
            .checks
            .iter()
            .any(|c| c.id == "network" && c.status == CheckStatus::Warning));
        let text = export_report(&report, Some("accessToken=secret123"));
        assert!(text.contains("诊断报告"));
        assert!(!text.contains("secret123"));
    }

    #[test]
    fn compatibility_summary_reports_missing_java() {
        let item = java_compatibility_summary(21, &[]);
        assert_eq!(item.status, CheckStatus::Error);
        assert!(item.fix.unwrap().contains("21"));
    }

    #[tokio::test]
    async fn log_tail_is_redacted_and_confined() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        paths.ensure_layout().await.unwrap();
        let log = paths.logs_dir().join("instances/x.log");
        tokio::fs::create_dir_all(log.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&log, "line1\naccessToken=abc999\nline3\n")
            .await
            .unwrap();
        let tail = read_log_tail(&paths, &log, 2).await.unwrap();
        assert!(!tail.contains("abc999"));
        assert!(tail.contains("line3"));

        let outside = dir.path().join("outside.txt");
        tokio::fs::write(&outside, "secret").await.unwrap();
        assert!(read_log_tail(&paths, &outside, 10).await.is_err());
    }

    fn set_age(path: &Path, days: u64) {
        let stamp = std::time::SystemTime::now() - std::time::Duration::from_secs(days * 24 * 3600);
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(stamp))
            .unwrap();
    }

    #[tokio::test]
    async fn cleanup_removes_expired_logs_and_keeps_recent() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        paths.ensure_layout().await.unwrap();
        let old = paths.logs_dir().join("old.log");
        let recent = paths.logs_dir().join("recent.log");
        tokio::fs::write(&old, b"old").await.unwrap();
        tokio::fs::write(&recent, b"new").await.unwrap();
        set_age(&old, 20);
        let report = cleanup_logs(&paths, 14, 1024 * 1024).await.unwrap();
        assert_eq!(report.removed, 1);
        assert!(!old.exists());
        assert!(recent.exists());
    }

    #[tokio::test]
    async fn cleanup_trims_total_size() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        paths.ensure_layout().await.unwrap();
        for name in ["a.log", "b.log", "c.log"] {
            let path = paths.logs_dir().join(name);
            tokio::fs::write(&path, b"xx").await.unwrap();
            set_age(&path, 2);
        }
        let report = cleanup_logs(&paths, 14, 2).await.unwrap();
        assert_eq!(report.removed, 2);
    }

    #[tokio::test]
    async fn delete_log_is_confined_to_log_dir() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        paths.ensure_layout().await.unwrap();
        let outside = dir.path().join("outside.txt");
        tokio::fs::write(&outside, b"x").await.unwrap();
        assert!(delete_log(&paths, &outside).await.is_err());
    }

    #[tokio::test]
    async fn list_logs_returns_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        paths.ensure_layout().await.unwrap();
        tokio::fs::write(paths.logs_dir().join("a.log"), b"a")
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        tokio::fs::write(paths.logs_dir().join("b.log"), b"bb")
            .await
            .unwrap();
        let logs = list_logs(&paths, 10).await.unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].name, "b.log");
    }
}
