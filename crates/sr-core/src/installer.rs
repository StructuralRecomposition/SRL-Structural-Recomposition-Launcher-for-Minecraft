//! Forge/NeoForge installation by running the **official installer** with an
//! explicit user action.
//!
//! This is the one place where the launcher executes third-party code. It is
//! never silent: the UI must call a dedicated command, the installer is the
//! unmodified official jar, its SHA-1 is not fabricated, and the process runs
//! against the user's chosen game directory. Libraries are pre-fetched from
//! the BMCLAPI/USTC mirror where possible so the installer does not have to
//! reach blocked Maven hosts.

use crate::download::{ensure_trusted_url, TaskHandle, TaskStatus};
use crate::error::{AppError, ErrorCode, Result};
use crate::paths::{AppPaths, GamePaths};
use crate::recommend::{select_forge, select_neoforge};
use crate::util;
use crate::version::VersionService;
use futures_util::stream::{self, StreamExt};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const BMCLAPI: &str = "https://bmclapi2.bangbang93.com";
pub const USTC: &str = "https://cmcc.mirrors.ustc.edu.cn/bmclapi";
const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;
const MAX_LIBRARY_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallerKind {
    Forge,
    NeoForge,
}

impl InstallerKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Forge => "Forge",
            Self::NeoForge => "NeoForge",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallerPlan {
    pub kind: InstallerKind,
    pub game_version: String,
    pub loader_version: String,
    pub version_id: String,
    pub installer_url: String,
    pub fallback_url: Option<String>,
}

async fn get_json(http: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let parsed = ensure_trusted_url(url, false)?;
    let resp = http
        .get(parsed)
        .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(AppError::new(
            ErrorCode::Http,
            format!("版本索引返回 HTTP {}", resp.status().as_u16()),
        ));
    }
    let bytes = resp.bytes().await?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(AppError::new(ErrorCode::VersionInvalid, "版本索引异常过大"));
    }
    Ok(bytes.to_vec())
}

pub async fn plan(
    http: &reqwest::Client,
    game_version: &str,
    kind: InstallerKind,
    loader_version: Option<&str>,
) -> Result<InstallerPlan> {
    if game_version.is_empty()
        || game_version.len() > 64
        || !game_version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    {
        return Err(AppError::invalid_input("Minecraft 版本格式非法"));
    }
    match kind {
        InstallerKind::Forge => {
            let url = format!("{BMCLAPI}/forge/minecraft/{game_version}");
            let bytes = get_json(http, &url).await?;
            let selected = loader_version.map(|v| v.to_string()).filter(|v| {
                !v.is_empty()
                    && v.len() <= 64
                    && v.chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
            });
            let loader = match selected {
                Some(v) => v,
                None => select_forge(&bytes, game_version)?
                    .map(|(v, _)| v)
                    .ok_or_else(|| AppError::not_found("没有匹配的 Forge 版本"))?,
            };
            let installer_url = format!(
                "{USTC}/net/minecraftforge/forge/{game_version}-{loader}/forge-{game_version}-{loader}-installer.jar"
            );
            let fallback = format!(
                "{BMCLAPI}/forge/download?mcversion={game_version}&version={loader}&category=installer&format=jar"
            );
            Ok(InstallerPlan {
                kind,
                game_version: game_version.into(),
                version_id: format!("{game_version}-forge-{loader}"),
                loader_version: loader,
                installer_url,
                fallback_url: Some(fallback),
            })
        }
        InstallerKind::NeoForge => {
            let url =
                "https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge/maven-metadata.xml";
            let bytes = get_json(http, url).await?;
            let text = String::from_utf8_lossy(&bytes);
            let selected = loader_version.map(|v| v.to_string()).filter(|v| {
                !v.is_empty()
                    && v.len() <= 64
                    && v.chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
            });
            let loader = match selected {
                Some(v) => v,
                None => select_neoforge(&text, game_version)
                    .map(|(v, _)| v)
                    .ok_or_else(|| AppError::not_found("没有匹配的 NeoForge 版本"))?,
            };
            let installer_url =
                format!("{USTC}/net/neoforged/neoforge/{loader}/neoforge-{loader}-installer.jar");
            let fallback_url = format!(
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/{loader}/neoforge-{loader}-installer.jar"
            );
            Ok(InstallerPlan {
                kind,
                game_version: game_version.into(),
                version_id: format!("neoforge-{loader}"),
                loader_version: loader,
                installer_url,
                fallback_url: Some(fallback_url),
            })
        }
    }
}

async fn download_installer(
    http: &reqwest::Client,
    plan: &InstallerPlan,
    dest: &Path,
    task: &TaskHandle,
) -> Result<()> {
    let mut urls = vec![plan.installer_url.clone()];
    if let Some(fallback) = &plan.fallback_url {
        urls.push(fallback.clone());
    }
    let mut last_error: Option<AppError> = None;
    for url in urls {
        let Ok(parsed) = ensure_trusted_url(&url, false) else {
            continue;
        };
        task.set_current_file(&format!("下载 {} 安装器…", plan.kind.name()))
            .await;
        match http
            .get(parsed)
            .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
            .timeout(Duration::from_secs(600))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Some(len) = resp.content_length() {
                    if len > MAX_INSTALLER_BYTES {
                        return Err(AppError::new(
                            ErrorCode::VersionInvalid,
                            "安装器文件异常过大",
                        ));
                    }
                }
                match resp.bytes().await {
                    Ok(bytes) if !bytes.is_empty() => {
                        util::atomic_write(dest, &bytes).await?;
                        return Ok(());
                    }
                    Ok(_) => last_error = Some(AppError::new(ErrorCode::Http, "安装器文件为空")),
                    Err(e) => last_error = Some(e.into()),
                }
            }
            Ok(resp) => {
                last_error = Some(AppError::new(
                    ErrorCode::Http,
                    format!("安装器下载返回 HTTP {}", resp.status().as_u16()),
                ))
            }
            Err(e) => last_error = Some(e.into()),
        }
    }
    Err(last_error.unwrap_or_else(|| AppError::network("无法下载安装器")))
}

#[derive(Debug, Clone)]
struct LibrarySpec {
    path: String,
    url: String,
    sha1: Option<String>,
    size: Option<u64>,
}

fn collect_libraries(installer: &Path) -> Result<Vec<LibrarySpec>> {
    let file = std::fs::File::open(installer)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| {
        AppError::new(ErrorCode::ZipUnsafe, "安装器不是有效的 JAR").with_detail(e.to_string())
    })?;
    let mut specs: BTreeMap<String, LibrarySpec> = BTreeMap::new();
    for name in ["version.json", "install_profile.json"] {
        let mut entry = match zip.by_name(name) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes)?;
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
            AppError::new(
                ErrorCode::VersionInvalid,
                format!("安装器内 {name} 格式无效"),
            )
            .with_detail(e.to_string())
        })?;
        let Some(libs) = value.get("libraries").and_then(|v| v.as_array()) else {
            continue;
        };
        for lib in libs {
            let Some(artifact) = lib
                .get("downloads")
                .and_then(|d| d.get("artifact"))
                .and_then(|a| a.as_object())
            else {
                continue;
            };
            let Some(path) = artifact.get("path").and_then(|v| v.as_str()) else {
                continue;
            };
            if path.contains("..") || path.starts_with('/') || path.contains('\\') {
                continue;
            }
            let url = artifact
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let sha1 = artifact
                .get("sha1")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let size = artifact.get("size").and_then(|v| v.as_u64());
            specs.entry(path.to_string()).or_insert(LibrarySpec {
                path: path.to_string(),
                url,
                sha1,
                size,
            });
        }
    }
    Ok(specs.into_values().collect())
}

async fn prefetch_libraries(
    http: &reqwest::Client,
    game: &GamePaths,
    installer: &Path,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<()> {
    let libs = collect_libraries(installer)?;
    if libs.is_empty() {
        return Ok(());
    }
    task.set_total_files(libs.len() as u32).await;
    let total: u64 = libs.iter().filter_map(|l| l.size).sum();
    task.set_total_bytes(total).await;
    let progress: Arc<Vec<AtomicU64>> = Arc::new(libs.iter().map(|_| AtomicU64::new(0)).collect());
    task.set_current_file("预下载加载器库…").await;

    let libs = Arc::new(libs);
    let result = async {
        let mut stream = stream::iter((0..libs.len()).map(|idx| {
            let http = http.clone();
            let game = game.clone();
            let libs = libs.clone();
            let cancel = cancel.clone();
            let progress = progress.clone();
            let task = task.clone();
            async move {
                if cancel.is_cancelled() {
                    return Err(AppError::cancelled());
                }
                let lib = &libs[idx];
                let dest = util::safe_join(&game.libraries_dir(), &lib.path)?;
                if let Some(expected) = &lib.sha1 {
                    if tokio::fs::try_exists(&dest).await.unwrap_or(false)
                        && util::sha1_file(&dest).await.ok().as_deref() == Some(expected.as_str())
                    {
                        progress[idx].store(lib.size.unwrap_or(0), Ordering::Relaxed);
                        task.file_finished().await;
                        return Ok::<(), AppError>(());
                    }
                }
                task.set_current_file(&format!("库 {}", lib.path)).await;
                let mut urls = vec![format!("{USTC}/{}", lib.path)];
                if !lib.url.is_empty() {
                    urls.push(lib.url.clone());
                }
                let mut last: Option<AppError> = None;
                for url in urls {
                    let Ok(parsed) = ensure_trusted_url(&url, false) else {
                        continue;
                    };
                    let resp = match http
                        .get(parsed)
                        .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
                        .timeout(Duration::from_secs(120))
                        .send()
                        .await
                    {
                        Ok(r) if r.status().is_success() => r,
                        Ok(r) => {
                            last = Some(AppError::new(
                                ErrorCode::Http,
                                format!("库 {} 返回 HTTP {}", lib.path, r.status().as_u16()),
                            ));
                            continue;
                        }
                        Err(e) => {
                            last = Some(e.into());
                            continue;
                        }
                    };
                    if let Some(len) = resp.content_length() {
                        if len > MAX_LIBRARY_BYTES {
                            last = Some(AppError::new(ErrorCode::VersionInvalid, "库文件异常过大"));
                            continue;
                        }
                    }
                    match resp.bytes().await {
                        Ok(bytes) => {
                            if let Some(expected) = &lib.sha1 {
                                let actual = util::sha1_bytes(&bytes);
                                if !actual.eq_ignore_ascii_case(expected) {
                                    last = Some(AppError::checksum(expected, &actual));
                                    continue;
                                }
                            }
                            util::atomic_write(&dest, &bytes).await?;
                            progress[idx].store(bytes.len() as u64, Ordering::Relaxed);
                            task.file_finished().await;
                            return Ok(());
                        }
                        Err(e) => last = Some(e.into()),
                    }
                }
                Err(last.unwrap_or_else(|| AppError::network(format!("无法下载库 {}", lib.path))))
            }
        }))
        .buffer_unordered(6);
        while let Some(item) = stream.next().await {
            if let Err(e) = item {
                // A missing optional artifact must not stop the installer; it
                // will try its own download and report the real error.
                tracing::warn!(error = %e, "loader library prefetch failed");
            }
        }
        Ok::<(), AppError>(())
    }
    .await;
    result?;
    let done: u64 = progress.iter().map(|p| p.load(Ordering::Relaxed)).sum();
    task.set_done_bytes(done.min(total)).await;
    Ok(())
}

async fn ensure_launcher_profiles(game: &GamePaths) -> Result<()> {
    let path = game.root().join("launcher_profiles.json");
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Ok(());
    }
    let bytes = br#"{"profiles":{},"settings":{},"version":3}"#;
    util::atomic_write(&path, bytes).await
}

async fn run_installer(
    java: &Path,
    installer: &Path,
    game_dir: &Path,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<()> {
    task.set_status(TaskStatus::Running).await;
    task.set_current_file("运行官方安装器…").await;
    let workdir = installer
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut child = tokio::process::Command::new(java)
        .current_dir(&workdir)
        .arg("-Djava.net.preferIPv4Stack=true")
        .arg("-jar")
        .arg(installer)
        .arg("--installClient")
        .arg(game_dir)
        .arg("--mirror")
        .arg(format!("{USTC}/"))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            AppError::new(ErrorCode::ProcessFailed, "无法启动 Java 安装器")
                .with_detail(e.to_string())
        })?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let task_out = task.clone();
    let out_handle = tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        if let Some(stdout) = stdout {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim();
                if !line.is_empty() {
                    task_out
                        .set_current_file(&line.chars().take(180).collect::<String>())
                        .await;
                }
            }
        }
    });
    let task_err = task.clone();
    let err_handle = tokio::spawn(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        if let Some(stderr) = stderr {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim();
                if !line.is_empty() {
                    task_err
                        .set_current_file(&line.chars().take(180).collect::<String>())
                        .await;
                }
            }
        }
    });

    let status = tokio::select! {
        result = child.wait() => result?,
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            let _ = out_handle.await;
            let _ = err_handle.await;
            return Err(AppError::cancelled());
        }
    };
    let _ = out_handle.await;
    let _ = err_handle.await;
    if !status.success() {
        return Err(AppError::new(
            ErrorCode::ProcessFailed,
            format!(
                "{} 安装器退出码 {}",
                "Forge/NeoForge",
                status.code().unwrap_or(-1)
            ),
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn install(
    http: &reqwest::Client,
    versions: &VersionService,
    paths: &AppPaths,
    game: &GamePaths,
    java: &Path,
    plan: &InstallerPlan,
    task: TaskHandle,
    cancel: CancellationToken,
) -> Result<String> {
    let result = async {
        task.set_status(TaskStatus::Running).await;
        let installer_dir = paths.cache_dir().join("installers");
        util::ensure_dir(&installer_dir).await?;
        let installer = installer_dir.join(format!(
            "{}-{}-installer.jar",
            plan.kind.name().to_lowercase(),
            plan.loader_version
        ));
        if !tokio::fs::try_exists(&installer).await.unwrap_or(false) {
            download_installer(http, plan, &installer, &task).await?;
        }
        if cancel.is_cancelled() {
            return Err(AppError::cancelled());
        }
        ensure_launcher_profiles(game).await?;
        prefetch_libraries(http, game, &installer, &task, &cancel).await?;
        if cancel.is_cancelled() {
            return Err(AppError::cancelled());
        }
        run_installer(java, &installer, game.root(), &task, &cancel).await?;
        if cancel.is_cancelled() {
            return Err(AppError::cancelled());
        }
        let version_json = game.version_json(&plan.version_id);
        if !tokio::fs::try_exists(&version_json).await.unwrap_or(false) {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                format!(
                    "安装器结束但未生成版本 {}，请查看安装器日志",
                    plan.version_id
                ),
            ));
        }
        // The official installer writes inheritance-based metadata. Flatten it
        // so launch/preflight can use the base assets/libraries directly.
        versions.flatten_installed(game, &plan.version_id).await?;
        task.complete().await;
        Ok(plan.version_id.clone())
    }
    .await;
    if let Err(e) = &result {
        task.fail(e.clone()).await;
    }
    result
}
