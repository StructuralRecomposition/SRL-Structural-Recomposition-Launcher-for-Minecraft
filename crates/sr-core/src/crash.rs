//! Crash report discovery and conservative analysis.
//!
//! Analysis is pattern-based and intentionally humble: it reports what it can
//! prove from the file and gives general next steps. Unknown crashes are not
//! assigned a fabricated root cause.

use crate::error::{AppError, Result};
use crate::instance::Instance;
use crate::util;
use serde::Serialize;
use std::path::PathBuf;

const MAX_REPORT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_REPORTS: usize = 20;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashFile {
    pub file_name: String,
    pub size: u64,
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashCause {
    pub title: String,
    pub detail: String,
    pub advice: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashAnalysis {
    pub file_name: String,
    pub description: Option<String>,
    pub minecraft_version: Option<String>,
    pub java_version: Option<String>,
    pub loader: Option<String>,
    pub causes: Vec<CrashCause>,
    pub suspect_mods: Vec<String>,
    pub excerpt: String,
}

pub fn reports_dir(instance: &Instance) -> PathBuf {
    instance.game_dir.join("crash-reports")
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.contains(':')
        || name == ".."
        || !name.ends_with(".txt")
    {
        return Err(AppError::path_unsafe("崩溃报告文件名非法"));
    }
    Ok(())
}

pub async fn list(instance: &Instance) -> Result<Vec<CrashFile>> {
    let dir = reports_dir(instance);
    let mut out = Vec::new();
    let mut rd = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    while let Some(entry) = rd.next_entry().await? {
        let path = entry.path();
        let meta = match tokio::fs::symlink_metadata(&path).await {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".txt") || validate_name(&name).is_err() {
            continue;
        }
        out.push(CrashFile {
            file_name: name,
            size: meta.len(),
            modified_at: meta
                .modified()
                .ok()
                .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()),
        });
    }
    out.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    out.truncate(MAX_REPORTS);
    Ok(out)
}

/// Delete one crash report after confirming it lives under the instance's
/// crash-reports directory.
pub async fn delete(instance: &Instance, file_name: &str) -> Result<()> {
    validate_name(file_name)?;
    let path = reports_dir(instance).join(file_name);
    let canonical = tokio::fs::canonicalize(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("崩溃报告不存在")
        } else {
            e.into()
        }
    })?;
    let root = tokio::fs::canonicalize(reports_dir(instance))
        .await
        .unwrap_or_else(|_| reports_dir(instance));
    if !canonical.starts_with(&root) {
        return Err(AppError::path_unsafe("拒绝删除崩溃目录之外的文件"));
    }
    let meta = tokio::fs::symlink_metadata(&canonical).await?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(AppError::path_unsafe("崩溃报告必须是普通文件"));
    }
    tokio::fs::remove_file(&canonical).await?;
    Ok(())
}

pub async fn analyze(instance: &Instance, file_name: &str) -> Result<CrashAnalysis> {
    validate_name(file_name)?;
    let path = reports_dir(instance).join(file_name);
    let canonical = tokio::fs::canonicalize(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("崩溃报告不存在")
        } else {
            e.into()
        }
    })?;
    let root = tokio::fs::canonicalize(reports_dir(instance))
        .await
        .unwrap_or_else(|_| reports_dir(instance));
    if !canonical.starts_with(&root) {
        return Err(AppError::path_unsafe("拒绝读取崩溃目录之外的文件"));
    }
    let meta = tokio::fs::metadata(&canonical).await?;
    if !meta.is_file() {
        return Err(AppError::path_unsafe("崩溃报告必须是普通文件"));
    }
    if meta.len() > MAX_REPORT_BYTES {
        return Err(AppError::invalid_input("崩溃报告超过 8MB，已拒绝解析"));
    }
    let text = tokio::fs::read_to_string(&canonical).await?;
    Ok(analyze_text(file_name, &text))
}

fn field<'a>(lines: &'a [&'a str], name: &str) -> Option<String> {
    for line in lines {
        if let Some(rest) = line.trim().strip_prefix(name) {
            let value = rest.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn cause(title: &str, detail: impl Into<String>, advice: impl Into<String>) -> CrashCause {
    CrashCause {
        title: title.into(),
        detail: detail.into(),
        advice: advice.into(),
    }
}

fn classify(text: &str) -> Vec<CrashCause> {
    let mut causes = Vec::new();
    if text.contains("java.lang.OutOfMemoryError") {
        let metaspace = text.contains("Metaspace");
        causes.push(cause(
            if metaspace {
                "Metaspace 内存不足"
            } else {
                "Java 堆内存不足"
            },
            if metaspace {
                "JVM 的 Metaspace 区域耗尽，通常与大量模组或类加载器泄漏有关。"
            } else {
                "游戏进程耗尽了分配的 Java 堆内存。"
            },
            if metaspace {
                "减少模组数量，或在实例 JVM 参数中增加 -XX:MaxMetaspaceSize=512m。"
            } else {
                "在实例设置中提高最大内存（如 4096–8192 MB），并关闭占用内存的其他程序。"
            },
        ));
    }
    if text.contains("UnsupportedClassVersionError") || text.contains("class file version") {
        causes.push(cause(
            "Java 版本不匹配",
            "某个类文件的版本高于当前 Java 运行时，或模组要求更高的 Java。",
            "在 Java 环境页面选择符合该 Minecraft/加载器要求的 Java 版本。",
        ));
    }
    if text.contains("Mixin apply failed")
        || text.contains("MixinApplyError")
        || text.contains("MixinTransformerError")
    {
        causes.push(cause(
            "Mixin 注入失败",
            "模组 Mixin 与当前游戏/其他模组不兼容。",
            "检查最近新增或更新的模组，逐一禁用定位冲突；确认模组支持当前 Minecraft 版本。",
        ));
    }
    if text.to_ascii_lowercase().contains("duplicate mod") || text.contains("Duplicate mods") {
        causes.push(cause(
            "重复模组",
            "mods/ 中存在同一模组的多个版本。",
            "保留一个版本，移除其余重复 jar。",
        ));
    }
    if text.contains("NoSuchMethodError") || text.contains("NoClassDefFoundError") {
        causes.push(cause(
            "模组 API 不兼容",
            "模组调用了当前版本中不存在的方法或类，常见于模组与加载器/游戏版本不匹配。",
            "核对模组支持的 Minecraft 与加载器版本，移除不兼容模组。",
        ));
    }
    if text.contains("EXCEPTION_ACCESS_VIOLATION")
        || text.contains("GLFW error")
        || text.to_ascii_lowercase().contains("opengl")
    {
        causes.push(cause(
            "图形驱动或 OpenGL 问题",
            "崩溃涉及图形驱动、OpenGL 上下文或 GLFW。",
            "更新显卡驱动；尝试关闭光影/资源包；确认没有强制使用不支持的 OpenGL 版本。",
        ));
    }
    if text.contains("ModResolutionException") || text.contains("Incompatible mod set") {
        causes.push(cause(
            "模组依赖冲突",
            "加载器无法解析模组依赖关系。",
            "查看报告中缺失/冲突的模组名，安装匹配版本或移除冲突项。",
        ));
    }
    causes
}

fn suspect_mods(lines: &[&str]) -> Vec<String> {
    let mut mods = Vec::new();
    let mut in_mods = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("Fabric Mods:") || trimmed.starts_with("Loaded mods:") {
            in_mods = true;
            continue;
        }
        if in_mods {
            if trimmed.is_empty() {
                continue;
            }
            if !line.starts_with(' ') && !line.starts_with('\t') && !trimmed.contains(':') {
                in_mods = false;
                continue;
            }
            if let Some((id, _)) = trimmed.split_once(':') {
                let id = id.trim();
                if !id.is_empty()
                    && id.len() < 80
                    && id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                {
                    mods.push(id.to_string());
                }
            }
        }
    }
    mods.sort();
    mods.dedup();
    mods.truncate(40);
    mods
}

fn analyze_text(file_name: &str, text: &str) -> CrashAnalysis {
    let lines: Vec<&str> = text.lines().collect();
    let description = field(&lines, "Description:");
    let minecraft_version = field(&lines, "Minecraft Version:");
    let java_version = field(&lines, "Java Version:");
    let loader = field(&lines, "Fabric Loader:")
        .or_else(|| field(&lines, "Quilt Loader:"))
        .or_else(|| field(&lines, "Forge:"));
    let mut causes = classify(text);
    if causes.is_empty() {
        causes.push(cause(
            "未能确定根因",
            description
                .clone()
                .unwrap_or_else(|| "报告中没有匹配到已知的崩溃模式。".into()),
            "查看下方堆栈与最近安装的模组；可在诊断中心导出报告进一步排查。",
        ));
    }
    let excerpt = lines
        .iter()
        .take(80)
        .map(|l| util::redact_secrets(l))
        .collect::<Vec<_>>()
        .join("\n");
    CrashAnalysis {
        file_name: file_name.to_string(),
        description,
        minecraft_version,
        java_version,
        loader,
        causes,
        suspect_mods: suspect_mods(&lines),
        excerpt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::Instance;

    fn inst(dir: &tempfile::TempDir) -> Instance {
        let game_dir = dir.path().join("inst");
        std::fs::create_dir_all(game_dir.join("crash-reports")).unwrap();
        Instance {
            id: "i".into(),
            name: "t".into(),
            version_id: "1.20.4".into(),
            game_dir,
            java_path: None,
            memory_mb: 2048,
            jvm_args: vec![],
            game_args: vec![],
            created_at: util::now_rfc3339(),
            last_launched_at: None,
            account_id: None,
            resolution_width: None,
            resolution_height: None,
            mod_loader: None,
        }
    }

    const OOM: &str = "\
---- Minecraft Crash Report ----
// Uh... Did I do that?

Description: Ticking entity

Minecraft Version: 1.20.4
Java Version: 17.0.9, Eclipse Adoptium
Fabric Loader: 0.16.0

java.lang.OutOfMemoryError: Java heap space
\tat net.minecraft.client.Minecraft.run(Minecraft.java:1)

Fabric Mods:
\tsodium: Sodium 0.5.8
\tlithium: Lithium 0.12.1
";

    #[test]
    fn detects_oom_and_mods() {
        let a = analyze_text("crash-1.txt", OOM);
        assert_eq!(a.description.as_deref(), Some("Ticking entity"));
        assert_eq!(a.minecraft_version.as_deref(), Some("1.20.4"));
        assert!(a.causes.iter().any(|c| c.title.contains("内存")));
        assert_eq!(a.suspect_mods, vec!["lithium", "sodium"]);
        assert!(a.excerpt.contains("Crash Report"));
    }

    #[test]
    fn detects_java_mismatch_and_mixin() {
        let text = "Description: Mixin\n\nUnsupportedClassVersionError: class file version 65.0\nMixin apply failed: mixin.json";
        let a = analyze_text("crash.txt", text);
        assert!(a.causes.iter().any(|c| c.title.contains("Java")));
        assert!(a.causes.iter().any(|c| c.title.contains("Mixin")));
    }

    #[test]
    fn unknown_crash_has_no_fabricated_root_cause() {
        let a = analyze_text("x.txt", "Description: Something odd\n\nstack line");
        assert_eq!(a.causes.len(), 1);
        assert!(a.causes[0].title.contains("未能确定"));
    }

    #[tokio::test]
    async fn lists_and_reads_reports() {
        let dir = tempfile::tempdir().unwrap();
        let i = inst(&dir);
        tokio::fs::write(i.game_dir.join("crash-reports/crash-a.txt"), OOM)
            .await
            .unwrap();
        tokio::fs::write(i.game_dir.join("crash-reports/notes.md"), "x")
            .await
            .unwrap();
        let list = list(&i).await.unwrap();
        assert_eq!(list.len(), 1);
        let a = analyze(&i, "crash-a.txt").await.unwrap();
        assert_eq!(a.file_name, "crash-a.txt");
    }

    #[tokio::test]
    async fn deletes_report_inside_crash_dir_only() {
        let dir = tempfile::tempdir().unwrap();
        let i = inst(&dir);
        let report = i.game_dir.join("crash-reports/crash-delete.txt");
        tokio::fs::write(&report, OOM).await.unwrap();
        delete(&i, "crash-delete.txt").await.unwrap();
        assert!(!report.exists());

        let outside = dir.path().join("outside.txt");
        tokio::fs::write(&outside, "secret").await.unwrap();
        assert!(delete(&i, "../outside.txt").await.is_err());
        assert!(outside.exists());
    }

    #[tokio::test]
    async fn rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let i = inst(&dir);
        tokio::fs::write(dir.path().join("outside.txt"), "secret")
            .await
            .unwrap();
        assert!(analyze(&i, "../outside.txt").await.is_err());
        assert!(analyze(&i, "..").await.is_err());
    }
}
