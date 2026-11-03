//! One-click Java runtime downloads from Eclipse Adoptium (Temurin).
//!
//! Runtimes are installed under `<app data>/java/<runtime>` and never touch the
//! system JDK. Archives are verified with SHA-256 before extraction.

use crate::download::{self, DownloadSpec, TaskHandle};
use crate::error::{AppError, ErrorCode, Result};
use crate::java::{self, JavaInfo};
use crate::paths::AppPaths;
use crate::util;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const API: &str = "https://api.adoptium.net/v3";
const MIRROR: &str = "https://mirrors.tuna.tsinghua.edu.cn/Adoptium";
const MAX_ARCHIVE_BYTES: u64 = 800 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRuntimeOptions {
    pub majors: Vec<u32>,
    pub lts: Vec<u32>,
    pub recommended: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRuntimeRelease {
    pub major: u32,
    pub version: String,
    pub release_name: String,
    pub download_size: u64,
    pub file_name: String,
    pub os: String,
    pub arch: String,
    #[serde(skip)]
    url: String,
    #[serde(skip)]
    checksum: String,
}

#[derive(Debug, Deserialize)]
struct AvailableRaw {
    #[serde(default)]
    available_releases: Vec<u32>,
    #[serde(default)]
    available_lts_releases: Vec<u32>,
    #[serde(default)]
    most_recent_lts: u32,
}

#[derive(Debug, Deserialize)]
struct AssetRaw {
    #[serde(default)]
    release_name: String,
    #[serde(default)]
    version: VersionRaw,
    #[serde(default)]
    binary: BinaryRaw,
}

#[derive(Debug, Deserialize, Default)]
struct VersionRaw {
    #[serde(default)]
    semver: String,
    #[serde(default)]
    major: u32,
}

#[derive(Debug, Deserialize, Default)]
struct BinaryRaw {
    #[serde(default)]
    os: String,
    #[serde(default)]
    architecture: String,
    #[serde(default)]
    package: PackageRaw,
}

#[derive(Debug, Deserialize, Default)]
struct PackageRaw {
    #[serde(default)]
    checksum: String,
    #[serde(default)]
    link: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    size: u64,
}

fn platform() -> Result<(&'static str, &'static str)> {
    #[cfg(target_os = "macos")]
    let os = "mac";
    #[cfg(target_os = "windows")]
    let os = "windows";
    #[cfg(all(unix, not(target_os = "macos")))]
    let os = "linux";
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    return Err(AppError::unsupported("当前系统不支持自动安装 Java"));

    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        "x86" => "x86",
        "arm" => "arm",
        other => {
            return Err(AppError::unsupported(format!(
                "当前 CPU 架构（{other}）没有可用的 Temurin 构建"
            )))
        }
    };
    Ok((os, arch))
}

async fn get_json<T: serde::de::DeserializeOwned>(http: &reqwest::Client, url: &str) -> Result<T> {
    let resp = http
        .get(url)
        .timeout(Duration::from_secs(25))
        .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::new(
            ErrorCode::Http,
            format!("Adoptium 返回 HTTP {}", status.as_u16()),
        )
        .retryable(status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS));
    }
    let bytes = resp.bytes().await?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(AppError::new(
            ErrorCode::VersionInvalid,
            "Adoptium 返回数据异常过大",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|e| {
        AppError::new(ErrorCode::InvalidInput, "Adoptium 返回数据格式无效")
            .with_detail(e.to_string())
    })
}

pub async fn options(http: &reqwest::Client) -> Result<JavaRuntimeOptions> {
    let raw: AvailableRaw = get_json(http, &format!("{API}/info/available_releases")).await?;
    let mut majors = raw.available_releases;
    majors.retain(|m| *m >= 8 && *m <= 40);
    majors.sort_unstable();
    majors.dedup();
    let mut lts = raw.available_lts_releases;
    lts.retain(|m| majors.contains(m));
    lts.sort_unstable();
    lts.dedup();
    // Java 21 is the safest default across current modpacks. Only 26.x needs
    // Java 25, and offering 25 as "recommended" led users to install it and
    // then break 1.21.1 Forge. Users can still pick 25 explicitly.
    let recommended = if majors.contains(&21) {
        21
    } else if raw.most_recent_lts >= 8 && majors.contains(&raw.most_recent_lts) {
        raw.most_recent_lts
    } else {
        lts.last()
            .copied()
            .or_else(|| majors.last().copied())
            .unwrap_or(21)
    };
    Ok(JavaRuntimeOptions {
        majors,
        lts,
        recommended,
    })
}

fn mirror_link(major: u32, os: &str, arch: &str, file_name: &str) -> String {
    let os = match os {
        "windows" => "windows",
        "mac" => "mac",
        _ => "linux",
    };
    format!("{MIRROR}/{major}/jdk/{arch}/{os}/{file_name}")
}

async fn mirror_available(http: &reqwest::Client, url: &str) -> bool {
    http.head(url)
        .timeout(Duration::from_secs(8))
        .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
        .send()
        .await
        .map(|resp| resp.status().is_success())
        .unwrap_or(false)
}

pub async fn latest(
    http: &reqwest::Client,
    major: u32,
    use_mirror: bool,
) -> Result<JavaRuntimeRelease> {
    if !(8..=40).contains(&major) {
        return Err(AppError::invalid_input("Java 版本不在支持范围内"));
    }
    let (os, arch) = platform()?;
    let url = format!(
        "{API}/assets/latest/{major}/hotspot?os={os}&architecture={arch}&image_type=jdk&vendor=eclipse"
    );
    let assets: Vec<AssetRaw> = get_json(http, &url).await?;
    let asset = assets
        .into_iter()
        .next()
        .ok_or_else(|| AppError::not_found(format!("没有找到 Java {major} 的 Temurin 构建")))?;
    if asset.binary.package.link.trim().is_empty()
        || asset.binary.package.name.trim().is_empty()
        || asset.binary.package.checksum.trim().is_empty()
    {
        return Err(AppError::internal("Adoptium 构建信息不完整"));
    }
    if asset.binary.package.size > MAX_ARCHIVE_BYTES {
        return Err(AppError::invalid_input("Java 安装包超过允许大小"));
    }
    let mut link = asset.binary.package.link;
    if use_mirror {
        // TUNA mirrors the exact Adoptium archive names. Probe first so a
        // partially synced mirror falls back to the official GitHub asset.
        let candidate = mirror_link(major, os, arch, &asset.binary.package.name);
        if mirror_available(http, &candidate).await {
            link = candidate;
        }
    }
    // Validate before downloading so a malformed API response cannot send us to
    // an arbitrary host.
    download::ensure_trusted_url(&link, false)?;
    Ok(JavaRuntimeRelease {
        major: if asset.version.major == 0 {
            major
        } else {
            asset.version.major
        },
        version: asset.version.semver,
        release_name: asset.release_name,
        download_size: asset.binary.package.size,
        file_name: asset.binary.package.name,
        os: asset.binary.os,
        arch: asset.binary.architecture,
        url: link,
        checksum: asset.binary.package.checksum,
    })
}

fn safe_component(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 120
        || value == "."
        || value == ".."
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    {
        return Err(AppError::invalid_input("Java 安装目录名称非法"));
    }
    Ok(value.replace('+', "-"))
}

fn runtime_dir(paths: &AppPaths, release: &JavaRuntimeRelease) -> Result<PathBuf> {
    let name = safe_component(&format!(
        "temurin-{}-{}",
        release.release_name, release.arch
    ))?;
    Ok(paths.java_dir().join(name))
}

/// Find the `java` executable inside an extracted JDK tree.
fn find_java(root: &Path, os: &str) -> Option<PathBuf> {
    let executable = if os == "windows" { "java.exe" } else { "java" };
    fn walk(dir: &Path, executable: &str, depth: usize, out: &mut Option<PathBuf>) {
        if depth > 6 || out.is_some() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                walk(&path, executable, depth + 1, out);
            } else if path.file_name().and_then(|n| n.to_str()) == Some(executable) {
                let parent = path.parent().and_then(|p| p.file_name());
                if parent.and_then(|p| p.to_str()) == Some("bin") {
                    *out = Some(path);
                    return;
                }
            }
        }
    }
    let mut found = None;
    walk(root, executable, 0, &mut found);
    found
}

fn unsafe_entry_path(path: &Path) -> bool {
    path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    let mut total = 0u64;
    let mut count = 0usize;
    for entry in tar.entries().map_err(|e| {
        AppError::new(ErrorCode::ZipUnsafe, "Java 安装包无法读取").with_detail(e.to_string())
    })? {
        let mut entry = entry.map_err(|e| {
            AppError::new(ErrorCode::ZipUnsafe, "Java 安装包条目无效").with_detail(e.to_string())
        })?;
        count += 1;
        if count > MAX_ENTRIES {
            return Err(AppError::path_unsafe("Java 安装包条目过多"));
        }
        let path = entry.path().map_err(|e| {
            AppError::new(ErrorCode::ZipUnsafe, "Java 安装包路径无效").with_detail(e.to_string())
        })?;
        if unsafe_entry_path(&path) {
            return Err(AppError::path_unsafe("Java 安装包包含不安全路径"));
        }
        let kind = entry.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            // Temurin JDK archives are regular files/directories; links could
            // escape the extraction root, so reject them.
            return Err(AppError::path_unsafe("Java 安装包包含符号链接"));
        }
        total = total.saturating_add(entry.size());
        if total > MAX_EXTRACTED_BYTES {
            return Err(AppError::path_unsafe("Java 解压后体积超过限制"));
        }
        entry.unpack_in(dest).map_err(|e| {
            AppError::new(ErrorCode::ZipUnsafe, "Java 安装包解压失败").with_detail(e.to_string())
        })?;
    }
    Ok(())
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| {
        AppError::new(ErrorCode::ZipUnsafe, "Java 安装包不是有效的 ZIP").with_detail(e.to_string())
    })?;
    let mut total = 0u64;
    for i in 0..zip.len() {
        if i > MAX_ENTRIES {
            return Err(AppError::path_unsafe("Java 安装包条目过多"));
        }
        let mut entry = zip.by_index(i).map_err(|e| {
            AppError::new(ErrorCode::ZipUnsafe, "Java 安装包条目无效").with_detail(e.to_string())
        })?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| AppError::path_unsafe("Java 安装包包含不安全路径"))?;
        let target = dest.join(&relative);
        total = total.saturating_add(entry.size());
        if total > MAX_EXTRACTED_BYTES {
            return Err(AppError::path_unsafe("Java 解压后体积超过限制"));
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&target)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }
    Ok(())
}

fn extract_archive(archive: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".zip") {
        extract_zip(archive, dest)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive, dest)
    } else {
        Err(AppError::unsupported("不支持的 Java 安装包格式"))
    }
}

async fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = tokio::io::AsyncReadExt::read(&mut file, &mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

struct ProgressTicker {
    value: Arc<AtomicU64>,
    handle: tokio::task::JoinHandle<()>,
}

impl ProgressTicker {
    fn start(task: &TaskHandle) -> Self {
        let value = Arc::new(AtomicU64::new(0));
        let ticker_value = value.clone();
        let ticker_task = task.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(350)).await;
                ticker_task
                    .set_done_bytes(ticker_value.load(Ordering::Relaxed))
                    .await;
            }
        });
        Self { value, handle }
    }

    fn set(&self, bytes: u64) {
        self.value.store(bytes, Ordering::Relaxed);
    }

    fn stop(self) {
        self.handle.abort();
    }
}

async fn download_archive(
    http: &reqwest::Client,
    release: &JavaRuntimeRelease,
    paths: &AppPaths,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<PathBuf> {
    let cache = paths.cache_dir().join("java");
    util::ensure_dir(&cache).await?;
    let file_name = safe_component(&release.file_name)?;
    let dest = cache.join(file_name);
    task.set_total_files(1).await;
    task.set_total_bytes(release.download_size).await;
    task.set_current_file(&release.file_name).await;
    task.set_status(crate::download::TaskStatus::Running).await;

    if tokio::fs::try_exists(&dest).await.unwrap_or(false) {
        if let Ok(actual) = sha256_file(&dest).await {
            if actual.eq_ignore_ascii_case(&release.checksum) {
                return Ok(dest);
            }
        }
        let _ = tokio::fs::remove_file(&dest).await;
    }

    let ticker = ProgressTicker::start(task);
    let spec = DownloadSpec {
        url: &release.url,
        dest: &dest,
        sha1: None,
        size: Some(release.download_size),
    };
    let outcome =
        download::download_file(http, &spec, cancel, false, |done| ticker.set(done)).await;
    ticker.stop();
    match outcome {
        Ok(crate::download::DownloadOutcome::Skipped)
        | Ok(crate::download::DownloadOutcome::Downloaded(_)) => Ok(dest),
        Err(e) => Err(e),
    }
}

// The release URL/checksum stay private on the struct so the UI payload remains
// small while the downloader can still verify the archive.

pub async fn install(
    http: &reqwest::Client,
    paths: &AppPaths,
    major: u32,
    use_mirror: bool,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<JavaInfo> {
    let (os, _) = platform()?;
    let release = latest(http, major, use_mirror).await?;
    let final_dir = runtime_dir(paths, &release)?;
    let executable = if os == "windows" { "java.exe" } else { "java" };

    if final_dir.is_dir() {
        if let Some(binary) = find_java(&final_dir, os) {
            if binary.file_name().and_then(|n| n.to_str()) == Some(executable) {
                if let Ok(info) = java::probe(&binary, "downloaded").await {
                    task.complete().await;
                    return Ok(info);
                }
            }
        }
    }

    let archive = download_archive(http, &release, paths, task, cancel).await?;

    task.set_status(crate::download::TaskStatus::Verifying)
        .await;
    task.set_current_file("校验 SHA-256…").await;
    let actual = sha256_file(&archive).await?;
    let expected = &release.checksum;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(AppError::new(
            ErrorCode::ChecksumMismatch,
            "Java 安装包校验失败，下载内容可能损坏",
        )
        .with_detail(format!("期望 SHA-256 {expected}，实际 {actual}"))
        .retryable(true));
    }

    task.set_current_file("解压 Java…").await;
    let temp = paths
        .cache_dir()
        .join("java")
        .join(format!("extract-{}", util::short_id()));
    let archive_clone = archive.clone();
    let temp_clone = temp.clone();
    tokio::task::spawn_blocking(move || extract_archive(&archive_clone, &temp_clone))
        .await
        .map_err(|e| AppError::internal(format!("解压任务失败：{e}")))??;

    let binary_rel = find_java(&temp, os)
        .ok_or_else(|| AppError::new(ErrorCode::JavaInvalid, "解压后没有找到 java 可执行文件"))?;
    let relative = binary_rel
        .strip_prefix(&temp)
        .map_err(|_| AppError::internal("Java 路径解析失败"))?
        .to_path_buf();

    util::ensure_dir(&paths.java_dir()).await?;
    if tokio::fs::try_exists(&final_dir).await.unwrap_or(false) {
        let managed = final_dir.starts_with(paths.java_dir());
        if !managed {
            return Err(AppError::path_unsafe("Java 安装目录不在启动器数据目录内"));
        }
        tokio::fs::remove_dir_all(&final_dir).await?;
    }
    tokio::fs::rename(&temp, &final_dir).await?;

    let binary = final_dir.join(relative);
    let info = java::probe(&binary, "downloaded").await?;
    task.file_finished().await;
    task.complete().await;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_runtime_names() {
        assert!(safe_component("jdk-21.0.13+9").is_ok());
        assert!(safe_component("../escape").is_err());
        assert!(safe_component("a/b").is_err());
        assert!(safe_component("").is_err());
    }

    #[test]
    fn finds_java_in_extracted_tree() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("jdk/Contents/Home/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let java = bin.join("java");
        std::fs::write(&java, b"stub").unwrap();
        assert_eq!(find_java(dir.path(), "mac"), Some(java));
    }
}
