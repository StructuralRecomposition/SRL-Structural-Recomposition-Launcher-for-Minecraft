use crate::error::{AppError, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JavaInfo {
    pub path: PathBuf,
    pub version_string: String,
    pub major: u32,
    pub vendor: Option<String>,
    pub arch: Option<String>,
    /// Where the launcher found it: manual | java_home | path | scan | system.
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JavaCompatibility {
    Compatible,
    Warning,
    Incompatible,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaCheck {
    pub compatibility: JavaCompatibility,
    pub required_major: u32,
    pub actual_major: u32,
    pub message: String,
}

pub fn executable_name() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

/// Parse the `major` component from common Java version strings.
pub fn parse_major(version: &str) -> Option<u32> {
    let v = version.trim();
    if v.is_empty() {
        return None;
    }
    if let Some(rest) = v.strip_prefix("1.") {
        let major = rest.split(['.', '_', '-', '+']).next()?;
        return major.parse().ok();
    }
    let first = v
        .split(['.', '_', '-', '+', ' '])
        .next()
        .unwrap_or_default();
    let digits: String = first.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// Highest Java major that is still safe for a version targeting `required`.
///
/// Minecraft/Forge are sensitive to bytecode newer than the version was built
/// for: a Java 21 Forge 52 pack fails on Java 25 with "Unsupported class file
/// major version 69". Exact matches are always preferred; this range is only
/// the fallback when the exact runtime is not installed.
pub fn max_supported_major(required: u32) -> u32 {
    match required {
        0..=8 => 8,
        9..=11 => 11,
        12..=16 => 17,
        17..=20 => 21,
        _ => required.saturating_add(2),
    }
}

pub fn is_compatible(required: u32, actual: u32) -> bool {
    actual >= required && actual <= max_supported_major(required)
}

pub fn check_compatibility(required: u32, actual: u32) -> JavaCheck {
    if actual < required {
        JavaCheck {
            compatibility: JavaCompatibility::Incompatible,
            required_major: required,
            actual_major: actual,
            message: format!(
                "该版本需要 Java {required}（兼容范围 {required}–{}），当前 Java {actual} 过旧，无法启动。",
                max_supported_major(required)
            ),
        }
    } else if actual > max_supported_major(required) {
        JavaCheck {
            compatibility: JavaCompatibility::Incompatible,
            required_major: required,
            actual_major: actual,
            message: format!(
                "该版本以 Java {required} 为目标，Java {actual} 过新（兼容上限 {}），Forge/模组可能因 ASM 不支持该字节码而崩溃。请改用 Java {required}。",
                max_supported_major(required)
            ),
        }
    } else if actual != required {
        JavaCheck {
            compatibility: JavaCompatibility::Warning,
            required_major: required,
            actual_major: actual,
            message: format!(
                "Java {actual} 可用，但该版本以 Java {required} 为目标；为保证 Forge/模组稳定，建议安装并使用 Java {required}。"
            ),
        }
    } else {
        JavaCheck {
            compatibility: JavaCompatibility::Compatible,
            required_major: required,
            actual_major: actual,
            message: format!("Java {actual} 与该版本目标完全匹配。"),
        }
    }
}

/// Probe one executable. Never executes anything but `-version` style flags.
pub async fn probe(path: &Path, source: &str) -> Result<JavaInfo> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Err(AppError::new(
            ErrorCode::JavaInvalid,
            format!("Java 可执行文件不存在：{}", path.display()),
        ));
    }
    let output = tokio::time::timeout(
        Duration::from_secs(8),
        tokio::process::Command::new(path)
            .args(["-XshowSettings:properties", "-version"])
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .map_err(|_| AppError::timeout("检测 Java 版本超时"))??;

    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));

    if !output.status.success() && text.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::JavaInvalid,
            "无法执行该文件，可能不是有效的 Java",
        ));
    }

    let mut version = None;
    let mut vendor = None;
    let mut arch = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("java.version =") {
            version = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("java.vendor =") {
            vendor = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("os.arch =") {
            arch = Some(v.trim().to_string());
        }
    }
    if version.is_none() {
        // Fallback for JVMs that do not support -XshowSettings.
        let out = tokio::time::timeout(
            Duration::from_secs(8),
            tokio::process::Command::new(path)
                .arg("-version")
                .stdin(Stdio::null())
                .output(),
        )
        .await
        .map_err(|_| AppError::timeout("检测 Java 版本超时"))??;
        let t = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        // Example: openjdk version "17.0.9" 2023-10-17
        if let Some(start) = t.find('"') {
            if let Some(end) = t[start + 1..].find('"') {
                version = Some(t[start + 1..start + 1 + end].to_string());
            }
        }
    }
    let version_string =
        version.ok_or_else(|| AppError::new(ErrorCode::JavaInvalid, "无法识别 Java 版本"))?;
    let major = parse_major(&version_string).ok_or_else(|| {
        AppError::new(
            ErrorCode::JavaInvalid,
            format!("无法解析 Java 版本号：{version_string}"),
        )
    })?;

    Ok(JavaInfo {
        path: path.to_path_buf(),
        version_string,
        major,
        vendor,
        arch,
        source: source.to_string(),
    })
}

fn candidate_from_java_home() -> Option<PathBuf> {
    let home = std::env::var_os("JAVA_HOME")?;
    let p = PathBuf::from(home).join("bin").join(executable_name());
    if p.is_file() {
        Some(p)
    } else {
        None
    }
}

fn candidates_from_path() -> Vec<PathBuf> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(&path)
        .map(|dir| dir.join(executable_name()))
        .filter(|p| p.is_file())
        .collect()
}

fn candidates_from_platform() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(target_os = "macos")]
    {
        for base in [
            PathBuf::from("/Library/Java/JavaVirtualMachines"),
            dirs_home().join("Library/Java/JavaVirtualMachines"),
        ] {
            if let Ok(entries) = std::fs::read_dir(&base) {
                for entry in entries.flatten() {
                    let p = entry.path().join("Contents/Home/bin/java");
                    if p.is_file() {
                        out.push(p);
                    }
                }
            }
        }
        // Legacy Oracle JRE (Java 8) is not under JavaVirtualMachines.
        let legacy = PathBuf::from(
            "/Library/Internet Plug-Ins/JavaAppletPlugin.plugin/Contents/Home/bin/java",
        );
        if legacy.is_file() {
            out.push(legacy);
        }
        // `java_home -V` lists every registered JVM with its Home path.
        if let Ok(output) = std::process::Command::new("/usr/libexec/java_home")
            .arg("-V")
            .output()
        {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            for line in text.lines() {
                if let Some(home) = line.split_whitespace().last() {
                    if home.starts_with('/') {
                        let p = PathBuf::from(home).join("bin/java");
                        if p.is_file() {
                            out.push(p);
                        }
                    }
                }
            }
        }
        // Ask the system helper last; it may print a path even if unlisted.
        if let Ok(output) = std::process::Command::new("/usr/libexec/java_home").output() {
            if output.status.success() {
                let p =
                    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()).join("bin/java");
                if p.is_file() {
                    out.push(p);
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        for base in ["/usr/lib/jvm", "/usr/java", "/opt/java"] {
            if let Ok(entries) = std::fs::read_dir(base) {
                for entry in entries.flatten() {
                    let p = entry.path().join("bin/java");
                    if p.is_file() {
                        out.push(p);
                    }
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let mut roots = Vec::new();
        for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(v) = std::env::var_os(var) {
                roots.push(PathBuf::from(v));
            }
        }
        for root in roots {
            for sub in [
                "Java",
                "Eclipse Adoptium",
                "Microsoft",
                "Zulu",
                "BellSoft",
                "Amazon Corretto",
                "Programs/Eclipse Adoptium",
            ] {
                let dir = root.join(sub);
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        for p in [
                            entry.path().join("bin/java.exe"),
                            entry.path().join("Contents/Home/bin/java.exe"),
                        ] {
                            if p.is_file() {
                                out.push(p);
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

#[allow(dead_code)]
fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Discover Java installations. Unreadable/invalid candidates are skipped (and
/// logged) instead of failing the whole scan.
pub async fn discover(extra: Option<&Path>) -> Vec<JavaInfo> {
    discover_with_managed(extra, None).await
}

fn java_under(root: &Path) -> Option<PathBuf> {
    for relative in [
        "Contents/Home/bin/java",
        "Contents/Home/bin/java.exe",
        "bin/java",
        "bin/java.exe",
    ] {
        let candidate = root.join(relative);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn managed_candidates(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let root = entry.path();
        if !root.is_dir() {
            continue;
        }
        if let Some(p) = java_under(&root) {
            out.push(p);
            continue;
        }
        // Temurin archives extract to <version>/Contents/Home (macOS) or
        // <version>/bin (Windows/Linux), so look one level deeper.
        if let Ok(children) = std::fs::read_dir(&root) {
            for child in children.flatten() {
                if child.path().is_dir() {
                    if let Some(p) = java_under(&child.path()) {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

/// Discover system Java plus runtimes installed by the launcher itself.
pub async fn discover_with_managed(
    extra: Option<&Path>,
    managed_dir: Option<&Path>,
) -> Vec<JavaInfo> {
    let mut candidates: Vec<(PathBuf, String)> = Vec::new();
    if let Some(p) = extra {
        candidates.push((p.to_path_buf(), "manual".into()));
    }
    if let Some(p) = candidate_from_java_home() {
        candidates.push((p, "java_home".into()));
    }
    for p in candidates_from_path() {
        candidates.push((p, "path".into()));
    }
    for p in candidates_from_platform() {
        candidates.push((p, "scan".into()));
    }
    if let Some(dir) = managed_dir {
        for p in managed_candidates(dir) {
            candidates.push((p, "downloaded".into()));
        }
    }

    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (path, source) in candidates {
        let canonical = tokio::fs::canonicalize(&path)
            .await
            .unwrap_or_else(|_| path.clone());
        if !seen.insert(canonical) {
            continue;
        }
        match probe(&path, &source).await {
            Ok(info) => out.push(info),
            Err(e) => {
                tracing::debug!(path = %path.display(), error = %e, "skipping java candidate")
            }
        }
    }
    out.sort_by(|a, b| b.major.cmp(&a.major).then_with(|| a.path.cmp(&b.path)));
    out
}

/// Pick a Java that is safe to launch for `required`.
///
/// Order: exact major (preferred build first), then the user's preferred
/// default if it is in range, then the closest major inside the safe range.
/// Returns `None` when only too-old/too-new runtimes exist, so the caller can
/// show an actionable "install Java N" error instead of crashing the game.
pub fn select_for_launch<'a>(
    javas: &'a [JavaInfo],
    required: u32,
    preferred: Option<&Path>,
) -> Option<&'a JavaInfo> {
    let exact = |j: &&JavaInfo| j.major == required;
    if let Some(p) = preferred {
        if let Some(j) = javas.iter().find(|j| j.path == p && j.major == required) {
            return Some(j);
        }
    }
    if let Some(j) = javas.iter().find(exact) {
        return Some(j);
    }
    if let Some(p) = preferred {
        if let Some(j) = javas
            .iter()
            .find(|j| j.path == p && is_compatible(required, j.major))
        {
            return Some(j);
        }
    }
    javas
        .iter()
        .filter(|j| is_compatible(required, j.major))
        .min_by_key(|j| (j.major - required, std::cmp::Reverse(j.major)))
}

/// Best-effort selection for diagnostics/UI. Falls back to the closest or
/// newest installed runtime so the user can see what is wrong.
pub fn select<'a>(
    javas: &'a [JavaInfo],
    required: u32,
    preferred: Option<&Path>,
) -> Option<&'a JavaInfo> {
    select_for_launch(javas, required, preferred)
        .or_else(|| preferred.and_then(|p| javas.iter().find(|j| j.path == p)))
        .or_else(|| {
            javas
                .iter()
                .filter(|j| j.major >= required)
                .min_by_key(|j| (j.major - required, std::cmp::Reverse(j.major)))
        })
        .or_else(|| javas.iter().max_by_key(|j| j.major))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_managed_runtimes() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("temurin-21/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let java = bin.join("java");
        std::fs::write(&java, b"stub").unwrap();
        let nested = dir.path().join("temurin-25/jdk-25.0.5+8/Contents/Home/bin");
        std::fs::create_dir_all(&nested).unwrap();
        let nested_java = nested.join("java");
        std::fs::write(&nested_java, b"stub").unwrap();
        let mut found = managed_candidates(dir.path());
        found.sort();
        let mut expected = vec![java, nested_java];
        expected.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn parses_common_version_strings() {
        assert_eq!(parse_major("1.8.0_392"), Some(8));
        assert_eq!(parse_major("17.0.9"), Some(17));
        assert_eq!(parse_major("21"), Some(21));
        assert_eq!(parse_major("25-ea"), Some(25));
        assert_eq!(parse_major("1.7.0_80"), Some(7));
        assert_eq!(parse_major("garbage"), None);
        assert_eq!(parse_major(""), None);
    }

    #[test]
    fn smart_java_ranges() {
        assert_eq!(max_supported_major(8), 8);
        assert_eq!(max_supported_major(16), 17);
        assert_eq!(max_supported_major(17), 21);
        assert_eq!(max_supported_major(21), 23);
        assert_eq!(max_supported_major(25), 27);
        assert!(is_compatible(21, 21));
        assert!(!is_compatible(21, 25));
        assert!(!is_compatible(8, 17));
    }

    #[test]
    fn compatibility_rules() {
        let c = check_compatibility(17, 16);
        assert_eq!(c.compatibility, JavaCompatibility::Incompatible);
        assert!(c.message.contains("无法启动"));
        let c = check_compatibility(17, 17);
        assert_eq!(c.compatibility, JavaCompatibility::Compatible);
        assert_eq!(
            check_compatibility(8, 8).compatibility,
            JavaCompatibility::Compatible
        );
        assert_eq!(
            check_compatibility(8, 17).compatibility,
            JavaCompatibility::Incompatible
        );
        assert_eq!(
            check_compatibility(17, 21).compatibility,
            JavaCompatibility::Warning
        );
        assert_eq!(
            check_compatibility(21, 23).compatibility,
            JavaCompatibility::Warning
        );
        let c = check_compatibility(21, 25);
        assert_eq!(c.compatibility, JavaCompatibility::Incompatible);
        assert!(c.message.contains("过新"));
    }

    #[test]
    fn selects_closest_compatible_java() {
        let mk = |p: &str, major: u32| JavaInfo {
            path: PathBuf::from(p),
            version_string: major.to_string(),
            major,
            vendor: None,
            arch: None,
            source: "test".into(),
        };
        let list = vec![mk("/j8", 8), mk("/j17", 17), mk("/j21", 21)];
        assert_eq!(select(&list, 17, None).unwrap().major, 17);
        assert_eq!(select(&list, 8, None).unwrap().major, 8);
        assert_eq!(select(&list, 21, None).unwrap().major, 21);
        // No compatible candidate: fall back to the newest for diagnostics.
        assert_eq!(select(&list, 25, None).unwrap().major, 21);
        assert_eq!(select_for_launch(&list, 25, None), None);
        // A preferred Java outside the safe range must not beat the exact match.
        assert_eq!(
            select(&list, 17, Some(Path::new("/j21"))).unwrap().major,
            17
        );
        let list = vec![mk("/j21", 21), mk("/j25", 25)];
        assert_eq!(
            select(&list, 21, Some(Path::new("/j25"))).unwrap().major,
            21
        );
        assert_eq!(
            select_for_launch(&list, 25, Some(Path::new("/j25")))
                .unwrap()
                .major,
            25
        );
        assert_eq!(select_for_launch(&list, 21, None).unwrap().major, 21);
        // No exact 17, but Java 21 is inside the safe range for a 17 target.
        let list = vec![mk("/j8", 8), mk("/j21", 21)];
        assert_eq!(select_for_launch(&list, 17, None).unwrap().major, 21);
    }
}
