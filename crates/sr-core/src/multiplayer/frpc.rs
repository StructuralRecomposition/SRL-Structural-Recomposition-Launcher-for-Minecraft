use crate::error::{AppError, ErrorCode, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::Mutex;

use super::types::FrpcView;

const MAX_FRPC_BYTES: u64 = 100 * 1024 * 1024;
const BUNDLE_LOCK: &str = "frpc.lock.json";
const RELEASE_API: &str = "https://api.github.com/repos/fatedier/frp/releases/latest";
const CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;

pub type DownloadProgress = Arc<dyn Fn(u64, Option<u64>) + Send + Sync>;

static PREPARE_LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct BundleLock {
    version: String,
    #[serde(default)]
    assets: HashMap<String, BundleAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct BundleAsset {
    name: String,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheMeta {
    version: String,
    platform: String,
    arch: String,
    asset_name: String,
    archive_sha256: String,
    exe_sha256: String,
    checked_at: i64,
}

#[derive(Debug, Clone)]
pub struct PreparedFrpc {
    pub path: PathBuf,
    pub version: String,
    pub sha256: String,
}

struct FrpcState {
    child: Option<Child>,
    config_path: Option<PathBuf>,
    version: Option<String>,
    sha256: Option<String>,
    log: Arc<Mutex<VecDeque<String>>>,
}

/// Terminate FRPC processes that belong to a previous SRL run. Matching is
/// scoped to this app's private config directory, never to arbitrary `frpc`
/// processes owned by the user.
fn kill_orphan_frpc_processes(dir: &std::path::Path) {
    let dir_text = dir.to_string_lossy().into_owned();
    if dir_text.is_empty() {
        return;
    }
    #[cfg(unix)]
    {
        let Ok(output) = std::process::Command::new("ps")
            .args(["-ax", "-o", "pid=,command="])
            .output()
        else {
            return;
        };
        let mut pids = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let line = line.trim();
            let Some((pid, command)) = line.split_once(char::is_whitespace) else {
                continue;
            };
            if command.contains(&dir_text) && command.contains("frpc-") {
                if let Ok(pid) = pid.trim().parse::<i32>() {
                    pids.push(pid);
                }
            }
        }
        for pid in &pids {
            unsafe {
                libc::kill(*pid, libc::SIGTERM);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(800));
        for pid in pids {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
    #[cfg(windows)]
    {
        let script = format!(
            "Get-CimInstance Win32_Process | Where-Object {{ $_.CommandLine -like '*{}*' -and $_.Name -like 'frpc*' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}",
            dir_text.replace('\'', "''")
        );
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output();
    }
}

pub struct FrpcManager {
    dir: PathBuf,
    bundled_dir: Option<PathBuf>,
    http: reqwest::Client,
    state: Mutex<FrpcState>,
}

impl FrpcManager {
    pub fn new(dir: PathBuf, http: reqwest::Client) -> Self {
        Self::new_with_bundle(dir, http, None)
    }

    pub fn new_with_bundle(
        dir: PathBuf,
        http: reqwest::Client,
        bundled_dir: Option<PathBuf>,
    ) -> Self {
        kill_orphan_frpc_processes(&dir);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("frpc-") && name.ends_with(".toml") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        Self {
            dir,
            bundled_dir,
            http,
            state: Mutex::new(FrpcState {
                child: None,
                config_path: None,
                version: None,
                sha256: None,
                log: Arc::new(Mutex::new(VecDeque::new())),
            }),
        }
    }

    pub fn bundled_dir_clone(&self) -> Option<PathBuf> {
        self.bundled_dir.clone()
    }

    /// Fast path: use the already downloaded/bundled executable without any
    /// network call. This is what makes the second and later room creations
    /// immediate.
    pub async fn prepare(&self, progress: Option<DownloadProgress>) -> Result<PreparedFrpc> {
        if let Some(cached) = self.cached() {
            return Ok(cached);
        }
        if let Some(bundled) = self.bundled().await {
            return Ok(bundled);
        }
        self.update_cache(progress).await
    }

    /// Called after connecting the account and at app startup. It does not
    /// block room creation and respects a 24-hour check interval.
    pub async fn warm_update(&self) {
        // A bundled client is pinned and offline; never touch the network.
        if self.bundled().await.is_some() {
            return;
        }
        if let Some(meta) = self.read_meta() {
            if chrono::Utc::now().timestamp() - meta.checked_at < CHECK_INTERVAL_SECS {
                return;
            }
        }
        let _ = self.update_cache(None).await;
    }

    pub async fn update_cache(&self, progress: Option<DownloadProgress>) -> Result<PreparedFrpc> {
        if let Some(bundled) = self.bundled().await {
            return Ok(bundled);
        }
        let _guard = PREPARE_LOCK.lock().await;
        if let Some(cached) = self.cached() {
            // The binary exists; refresh metadata at most once per day.
            if let Some(meta) = self.read_meta() {
                if chrono::Utc::now().timestamp() - meta.checked_at < CHECK_INTERVAL_SECS {
                    return Ok(cached);
                }
            }
        }

        let release = self.release().await?;
        let version = release.tag_name.trim_start_matches('v').to_string();
        let platform = platform_name(std::env::consts::OS)?;
        let arch = arch_name(std::env::consts::ARCH)?;
        let ext = if platform == "windows" {
            "zip"
        } else {
            "tar.gz"
        };
        let wanted = format!("frp_{version}_{platform}_{arch}.{ext}");
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == wanted)
            .or_else(|| {
                release.assets.iter().find(|a| {
                    a.name.contains(&format!("_{platform}_{arch}.")) && a.name.ends_with(ext)
                })
            })
            .ok_or_else(|| {
                AppError::not_found(format!("frp {version} 没有当前平台的客户端（{wanted}）"))
            })?;
        if asset.size > MAX_FRPC_BYTES {
            return Err(AppError::new(
                ErrorCode::Http,
                "FRPC 下载文件超过 100 MB，已拒绝",
            ));
        }
        verify_download_url(&asset.browser_download_url)?;

        let expected_archive = if let Some(digest) = asset
            .digest
            .as_deref()
            .and_then(|v| v.strip_prefix("sha256:"))
            .filter(|v| !v.is_empty())
        {
            digest.to_ascii_lowercase()
        } else {
            let checksums = release
                .assets
                .iter()
                .find(|a| a.name == "frp_sha256_checksums.txt")
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCode::ChecksumMismatch,
                        "frp Release 没有 SHA-256 校验信息，已拒绝下载",
                    )
                })?;
            let text = self.download_text(&checksums.browser_download_url).await?;
            checksum_for(&text, &asset.name).ok_or_else(|| {
                AppError::new(
                    ErrorCode::ChecksumMismatch,
                    "frp Release 的校验文件中没有当前客户端",
                )
            })?
        };

        if let Some(meta) = self.read_meta() {
            if meta.archive_sha256.eq_ignore_ascii_case(&expected_archive) {
                if let Some(cached) = self.cached() {
                    let mut updated = meta;
                    updated.checked_at = chrono::Utc::now().timestamp();
                    let _ = self.write_meta(&updated);
                    return Ok(cached);
                }
            }
        }

        let target_dir = self.dir.join(format!("{version}-{platform}-{arch}"));
        let target = target_dir.join(if platform == "windows" {
            "frpc.exe"
        } else {
            "frpc"
        });
        crate::util::ensure_dir(&target_dir).await?;

        let archive = self
            .download_with_fallback(&asset.browser_download_url, &expected_archive, progress)
            .await?;
        let archive_sha = sha256_hex(&archive);
        if !archive_sha.eq_ignore_ascii_case(&expected_archive) {
            return Err(AppError::checksum(&expected_archive, &archive_sha));
        }
        let extracted = extract_executable(&archive, platform)?;
        if !executable_magic_ok(&extracted) {
            return Err(AppError::new(
                ErrorCode::ChecksumMismatch,
                "下载的 FRPC 不是有效的可执行文件，已拒绝运行",
            ));
        }
        std::fs::write(&target, &extracted)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
        }
        let exe_sha = sha256_hex(&extracted);
        let meta = CacheMeta {
            version: version.clone(),
            platform: platform.to_string(),
            arch: arch.to_string(),
            asset_name: asset.name.clone(),
            archive_sha256: archive_sha,
            exe_sha256: exe_sha.clone(),
            checked_at: chrono::Utc::now().timestamp(),
        };
        self.write_meta(&meta)?;
        Ok(PreparedFrpc {
            path: target,
            version,
            sha256: exe_sha,
        })
    }

    fn meta_path(&self) -> PathBuf {
        self.dir.join("current.json")
    }

    fn read_meta(&self) -> Option<CacheMeta> {
        let text = std::fs::read_to_string(self.meta_path()).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write_meta(&self, meta: &CacheMeta) -> Result<()> {
        crate::util::ensure_dir_sync(&self.dir)?;
        let json = serde_json::to_vec_pretty(meta)?;
        crate::util::write_private_file_sync(&self.meta_path(), &json)
    }

    fn cached(&self) -> Option<PreparedFrpc> {
        let meta = self.read_meta()?;
        let platform = platform_name(std::env::consts::OS).ok()?;
        let arch = arch_name(std::env::consts::ARCH).ok()?;
        if meta.platform != platform || meta.arch != arch {
            return None;
        }
        let target = self
            .dir
            .join(format!("{}-{}-{}", meta.version, meta.platform, meta.arch));
        let target = target.join(if cfg!(windows) { "frpc.exe" } else { "frpc" });
        let bytes = std::fs::read(&target).ok()?;
        if !executable_magic_ok(&bytes)
            || !sha256_hex(&bytes).eq_ignore_ascii_case(&meta.exe_sha256)
        {
            return None;
        }
        Some(PreparedFrpc {
            path: target,
            version: meta.version,
            sha256: meta.exe_sha256,
        })
    }

    async fn bundled(&self) -> Option<PreparedFrpc> {
        let dir = self.bundled_dir.clone()?;
        let platform = platform_name(std::env::consts::OS).ok()?;
        let arch = arch_name(std::env::consts::ARCH).ok()?;
        let cache_dir = self.dir.clone();
        tokio::task::spawn_blocking(move || bundled_blocking(&dir, &cache_dir, platform, arch))
            .await
            .ok()?
    }

    async fn release(&self) -> Result<GitHubRelease> {
        let response = self
            .http
            .get(RELEASE_API)
            .header("Accept", "application/vnd.github+json")
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .map_err(|e| AppError::network("无法获取 frp 最新版本").with_detail(e.to_string()))?;
        if !response.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Http,
                format!("GitHub Release 返回 HTTP {}", response.status().as_u16()),
            ));
        }
        response.json::<GitHubRelease>().await.map_err(|e| {
            AppError::new(ErrorCode::Http, "无法解析 frp Release").with_detail(e.to_string())
        })
    }

    async fn download_text(&self, url: &str) -> Result<String> {
        verify_download_url(url)?;
        let response = self
            .http
            .get(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| AppError::network("下载 frp 校验文件失败").with_detail(e.to_string()))?;
        if !response.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Http,
                format!("下载校验文件返回 HTTP {}", response.status().as_u16()),
            ));
        }
        response
            .text()
            .await
            .map_err(|e| AppError::network("读取校验文件失败").with_detail(e.to_string()))
    }

    async fn download_with_fallback(
        &self,
        url: &str,
        expected_sha: &str,
        progress: Option<DownloadProgress>,
    ) -> Result<Vec<u8>> {
        // China-first mirror order. The archive SHA-256 from GitHub is the
        // source of truth, so a mirror cannot substitute a different binary.
        let candidates = [
            format!("https://ghproxy.net/{url}"),
            format!("https://gh-proxy.net/{url}"),
            format!("https://ghfast.top/{url}"),
            format!("https://ghproxy.com/{url}"),
            format!("https://gh-proxy.com/{url}"),
            format!("https://mirror.ghproxy.com/{url}"),
            url.to_string(),
        ];
        let mut last = None;
        let result = tokio::time::timeout(Duration::from_secs(50), async {
            for candidate in candidates {
                match self.download(&candidate, progress.clone()).await {
                    Ok(bytes) => {
                        if sha256_hex(&bytes).eq_ignore_ascii_case(expected_sha) {
                            return Ok(bytes);
                        }
                        last = Some(AppError::checksum(expected_sha, &sha256_hex(&bytes)));
                    }
                    Err(error) => last = Some(error),
                }
            }
            Err(last.unwrap_or_else(|| AppError::network("下载 FRPC 失败")))
        })
        .await;
        match result {
            Ok(value) => value,
            Err(_) => Err(AppError::timeout("下载 FRPC 超时，请检查网络后重试")),
        }
    }

    async fn download(&self, url: &str, progress: Option<DownloadProgress>) -> Result<Vec<u8>> {
        let response = self
            .http
            .get(url)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| AppError::network("下载 FRPC 失败").with_detail(e.to_string()))?;
        if !response.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Http,
                format!("下载 FRPC 返回 HTTP {}", response.status().as_u16()),
            ));
        }
        let total = response.content_length();
        if let Some(length) = total {
            if length > MAX_FRPC_BYTES {
                return Err(AppError::new(
                    ErrorCode::Http,
                    "FRPC 下载文件超过 100 MB，已拒绝",
                ));
            }
        }
        let started = Instant::now();
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk
                .map_err(|e| AppError::network("下载 FRPC 中断").with_detail(e.to_string()))?;
            if bytes.len() as u64 + chunk.len() as u64 > MAX_FRPC_BYTES {
                return Err(AppError::new(
                    ErrorCode::Http,
                    "FRPC 下载文件超过 100 MB，已拒绝",
                ));
            }
            bytes.extend_from_slice(&chunk);
            if let Some(callback) = &progress {
                if last_emit.elapsed() >= Duration::from_millis(300) {
                    callback(bytes.len() as u64, total);
                    last_emit = Instant::now();
                }
            }
        }
        if let Some(callback) = &progress {
            callback(bytes.len() as u64, total);
        }
        let _ = started;
        Ok(bytes)
    }

    pub async fn start(&self, binary: &Path, config: &str) -> Result<u32> {
        self.stop().await?;
        if !binary.is_file() {
            return Err(AppError::not_found("FRPC 客户端不存在，请重新下载"));
        }
        crate::util::ensure_dir(&self.dir).await?;
        let config_path = self.dir.join(format!("frpc-{}.toml", uuid::Uuid::new_v4()));
        write_private(&config_path, config)?;

        let mut command = tokio::process::Command::new(binary);
        command
            .arg("-c")
            .arg(&config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().map_err(|e| {
            let _ = std::fs::remove_file(&config_path);
            AppError::new(ErrorCode::ProcessFailed, "无法启动 FRPC").with_detail(e.to_string())
        })?;
        let pid = child.id().unwrap_or(0);
        let log = {
            let state = self.state.lock().await;
            state.log.clone()
        };

        if let Some(stdout) = child.stdout.take() {
            let log = log.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    push_log(&log, line).await;
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let log = log.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    push_log(&log, line).await;
                }
            });
        }

        tokio::time::sleep(Duration::from_millis(1200)).await;
        if let Ok(Some(status)) = child.try_wait() {
            let tail = log
                .lock()
                .await
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            let _ = std::fs::remove_file(&config_path);
            return Err(AppError::new(
                ErrorCode::ProcessFailed,
                format!("FRPC 启动后立即退出（{status}）"),
            )
            .with_detail(tail));
        }

        let mut state = self.state.lock().await;
        state.child = Some(child);
        state.config_path = Some(config_path);
        Ok(pid)
    }

    pub async fn stop(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        if let Some(mut child) = state.child.take() {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
        }
        if let Some(path) = state.config_path.take() {
            let _ = std::fs::remove_file(path);
        }
        Ok(())
    }

    pub async fn shutdown(&self) {
        let _ = self.stop().await;
    }

    pub async fn status(&self) -> FrpcView {
        let mut state = self.state.lock().await;
        let mut running = false;
        if let Some(child) = state.child.as_mut() {
            match child.try_wait() {
                Ok(None) => running = true,
                Ok(Some(_)) | Err(_) => {
                    state.child = None;
                    if let Some(path) = state.config_path.take() {
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
        }
        let log_tail = {
            let log = state.log.lock().await;
            let skip = log.len().saturating_sub(40);
            log.iter().skip(skip).cloned().collect()
        };
        FrpcView {
            running,
            version: state.version.clone(),
            path: None,
            sha256: state.sha256.clone(),
            log_tail,
        }
    }

    pub async fn set_prepared(&self, prepared: &PreparedFrpc) {
        let mut state = self.state.lock().await;
        state.version = Some(prepared.version.clone());
        state.sha256 = Some(prepared.sha256.clone());
    }

    pub async fn is_running(&self) -> bool {
        self.status().await.running
    }

    pub async fn pid(&self) -> Option<u32> {
        let mut state = self.state.lock().await;
        let child = state.child.as_mut()?;
        match child.try_wait() {
            Ok(None) => child.id(),
            _ => None,
        }
    }
}

async fn push_log(log: &Arc<Mutex<VecDeque<String>>>, line: String) {
    let clean = redact(&line);
    let mut guard = log.lock().await;
    guard.push_back(clean);
    while guard.len() > 200 {
        guard.pop_front();
    }
}

fn redact(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    if lower.contains("token") || lower.contains("secret") || lower.contains("password") {
        return "[已隐藏包含凭据的 FRPC 日志行]".into();
    }
    line.to_string()
}

fn platform_name(os: &str) -> Result<&'static str> {
    match os.to_ascii_lowercase().as_str() {
        "macos" | "darwin" | "osx" => Ok("darwin"),
        "windows" => Ok("windows"),
        "linux" => Ok("linux"),
        other => Err(AppError::unsupported(format!("不支持的操作系统：{other}"))),
    }
}

fn arch_name(arch: &str) -> Result<&'static str> {
    match arch.to_ascii_lowercase().as_str() {
        "x86_64" | "amd64" | "x64" => Ok("amd64"),
        "aarch64" | "arm64" => Ok("arm64"),
        "arm" | "armv7" => Ok("arm"),
        "x86" | "i686" | "i386" => Ok("386"),
        other => Err(AppError::unsupported(format!("不支持的 CPU 架构：{other}"))),
    }
}

fn checksum_for(text: &str, filename: &str) -> Option<String> {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else { continue };
        let Some(name) = parts.next() else { continue };
        if name.trim_start_matches('*') == filename {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

/// Extract the client shipped inside the installer into the writable app data
/// directory. Shipping the archive (not a bare executable) keeps macOS code
/// signing happy and lets us verify the pinned SHA-256 before every first run.
fn bundled_blocking(
    bundle: &Path,
    cache: &Path,
    platform: &str,
    arch: &str,
) -> Option<PreparedFrpc> {
    let lock_bytes = std::fs::read(bundle.join(BUNDLE_LOCK)).ok()?;
    let lock: BundleLock = serde_json::from_slice(&lock_bytes).ok()?;
    let key = format!("{platform}-{arch}");
    let asset = lock.assets.get(&key)?;
    let archive_path = bundle.join(platform).join(arch).join(&asset.name);
    let target_dir = cache.join(format!("bundled-{}-{key}", lock.version));
    let exe_name = if platform == "windows" {
        "frpc.exe"
    } else {
        "frpc"
    };
    let target = target_dir.join(exe_name);
    let record = target_dir.join(".sha256");

    if let Ok(bytes) = std::fs::read(&target) {
        if executable_magic_ok(&bytes) {
            let sha = sha256_hex(&bytes);
            let recorded = std::fs::read_to_string(&record).unwrap_or_default();
            if recorded.trim().eq_ignore_ascii_case(&sha) {
                return Some(PreparedFrpc {
                    path: target,
                    version: lock.version,
                    sha256: sha,
                });
            }
        }
    }

    let archive = match std::fs::read(&archive_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(path = %archive_path.display(), %error, "bundled frpc archive missing");
            return None;
        }
    };
    if !sha256_hex(&archive).eq_ignore_ascii_case(&asset.sha256) {
        tracing::warn!(path = %archive_path.display(), "bundled frpc archive checksum mismatch");
        return None;
    }
    let exe = match extract_executable(&archive, platform) {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(%error, "cannot extract bundled frpc");
            return None;
        }
    };
    if !executable_magic_ok(&exe) {
        tracing::warn!("bundled frpc is not a native executable");
        return None;
    }
    crate::util::ensure_dir_sync(&target_dir).ok()?;
    let tmp = target_dir.join(format!(".{exe_name}.tmp"));
    if std::fs::write(&tmp, &exe).is_err() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_file(&target);
    if std::fs::rename(&tmp, &target).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    let sha = sha256_hex(&exe);
    let _ = std::fs::write(&record, format!("{sha}\n"));
    tracing::info!(version = %lock.version, arch = %arch, "using bundled FRPC");
    Some(PreparedFrpc {
        path: target,
        version: lock.version,
        sha256: sha,
    })
}

fn verify_download_url(url: &str) -> Result<()> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| AppError::invalid_input("FRPC 下载地址格式无效"))?;
    if parsed.scheme() != "https" {
        return Err(AppError::path_unsafe("FRPC 下载地址必须使用 HTTPS"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::invalid_input("FRPC 下载地址缺少主机名"))?;
    let allowed = host == "github.com"
        || host == "objects.githubusercontent.com"
        || host.ends_with(".githubusercontent.com")
        || host == "ghproxy.com"
        || host == "gh-proxy.com"
        || host == "gh-proxy.net"
        || host == "ghproxy.net"
        || host == "ghfast.top"
        || host == "mirror.ghproxy.com";
    if !allowed {
        return Err(AppError::path_unsafe(format!(
            "FRPC 下载来源不是 frp 官方 GitHub：{host}"
        )));
    }
    Ok(())
}

fn extract_executable(bytes: &[u8], os: &str) -> Result<Vec<u8>> {
    if bytes.starts_with(b"PK\x03\x04") {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| {
            AppError::new(ErrorCode::Internal, "FRPC ZIP 损坏").with_detail(e.to_string())
        })?;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).map_err(|e| {
                AppError::new(ErrorCode::Internal, "无法读取 FRPC ZIP").with_detail(e.to_string())
            })?;
            if file.is_dir() {
                continue;
            }
            let name = file.name()?.to_ascii_lowercase();
            if name.ends_with("/frpc")
                || name.ends_with("/frpc.exe")
                || name == "frpc"
                || name == "frpc.exe"
            {
                let mut output = Vec::new();
                file.read_to_end(&mut output)?;
                return Ok(output);
            }
        }
        return Err(AppError::not_found("FRPC 压缩包中没有找到客户端可执行文件"));
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries().map_err(|e| {
            AppError::new(ErrorCode::Internal, "FRPC 压缩包损坏").with_detail(e.to_string())
        })? {
            let mut entry = entry.map_err(|e| {
                AppError::new(ErrorCode::Internal, "无法读取 FRPC 压缩包")
                    .with_detail(e.to_string())
            })?;
            let path = entry
                .path()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if path.ends_with("/frpc") || path.ends_with("/frpc.exe") || path == "frpc" {
                let mut output = Vec::new();
                entry.read_to_end(&mut output)?;
                return Ok(output);
            }
        }
        return Err(AppError::not_found("FRPC 压缩包中没有找到客户端可执行文件"));
    }
    let _ = os;
    Ok(bytes.to_vec())
}

fn executable_magic_ok(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    if bytes.starts_with(b"MZ") || bytes.starts_with(b"\x7fELF") {
        return true;
    }
    matches!(
        &bytes[..4],
        [0xcf, 0xfa, 0xed, 0xfe]
            | [0xce, 0xfa, 0xed, 0xfe]
            | [0xca, 0xfe, 0xba, 0xbe]
            | [0xbe, 0xba, 0xfe, 0xca]
            | [0xfe, 0xed, 0xfa, 0xcf]
            | [0xfe, 0xed, 0xfa, 0xce]
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn write_private(path: &Path, contents: &str) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checksum_file() {
        let text = "abc123  frp_0.72.0_linux_amd64.tar.gz\ndef456  frp_0.72.0_windows_amd64.zip\n";
        assert_eq!(
            checksum_for(text, "frp_0.72.0_windows_amd64.zip").as_deref(),
            Some("def456")
        );
        assert_eq!(checksum_for(text, "missing"), None);
    }

    #[test]
    fn maps_platforms_and_architectures() {
        assert_eq!(platform_name("macos").unwrap(), "darwin");
        assert_eq!(platform_name("windows").unwrap(), "windows");
        assert_eq!(arch_name("x86_64").unwrap(), "amd64");
        assert_eq!(arch_name("aarch64").unwrap(), "arm64");
    }

    #[test]
    fn rejects_executable_from_random_host() {
        assert!(verify_download_url("https://evil.example/frpc.zip").is_err());
        assert!(verify_download_url(
            "https://github.com/fatedier/frp/releases/download/v0.72.0/x.tar.gz"
        )
        .is_ok());
        assert!(verify_download_url("https://objects.githubusercontent.com/x").is_ok());
    }

    #[test]
    fn redacts_credentials() {
        assert!(redact("token = abc").contains("隐藏"));
        assert_eq!(redact("start proxy success"), "start proxy success");
    }

    #[test]
    fn validates_executable_magic() {
        assert!(executable_magic_ok(b"MZ...."));
        assert!(executable_magic_ok(b"\x7fELF...."));
        assert!(!executable_magic_ok(b"hello"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn starts_monitors_and_stops_child() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("frpc");
        std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let manager = FrpcManager::new(dir.path().join("cache"), reqwest::Client::new());
        let pid = manager.start(&script, "test = true\n").await.unwrap();
        assert!(pid > 0);
        assert!(manager.is_running().await);
        manager.stop().await.unwrap();
        assert!(!manager.is_running().await);
    }

    fn fake_elf() -> Vec<u8> {
        let mut bytes = b"\x7fELF".to_vec();
        bytes.extend_from_slice(&[0u8; 60]);
        bytes
    }

    fn write_bundle(root: &Path, sha_override: Option<String>) -> PathBuf {
        let bundle = root.join("bundle");
        let dir = bundle.join("darwin").join("amd64");
        std::fs::create_dir_all(&dir).unwrap();
        let name = "frp_test_darwin_amd64.tar.gz";
        let exe = fake_elf();
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(exe.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "frp_test/frpc", exe.as_slice())
            .unwrap();
        let archive = builder.into_inner().unwrap().finish().unwrap();
        std::fs::write(dir.join(name), &archive).unwrap();
        let sha = sha_override.unwrap_or_else(|| sha256_hex(&archive));
        let lock = serde_json::json!({
            "version": "test",
            "assets": { "darwin-amd64": { "name": name, "sha256": sha } }
        });
        std::fs::write(bundle.join(BUNDLE_LOCK), serde_json::to_vec(&lock).unwrap()).unwrap();
        bundle
    }

    #[test]
    fn extracts_bundled_frpc_and_reuses_cache() {
        let root = tempfile::tempdir().unwrap();
        let bundle = write_bundle(root.path(), None);
        let cache = root.path().join("cache");
        let first = bundled_blocking(&bundle, &cache, "darwin", "amd64").unwrap();
        assert_eq!(first.version, "test");
        assert!(first.path.is_file());
        assert!(executable_magic_ok(&std::fs::read(&first.path).unwrap()));

        // Remove the archive: the extracted cache must still work.
        let archive = bundle
            .join("darwin")
            .join("amd64")
            .join("frp_test_darwin_amd64.tar.gz");
        std::fs::remove_file(archive).unwrap();
        let second = bundled_blocking(&bundle, &cache, "darwin", "amd64").unwrap();
        assert_eq!(second.sha256, first.sha256);
    }

    #[test]
    fn rejects_bundle_with_bad_checksum() {
        let root = tempfile::tempdir().unwrap();
        let bundle = write_bundle(root.path(), Some("0".repeat(64)));
        let cache = root.path().join("cache");
        assert!(bundled_blocking(&bundle, &cache, "darwin", "amd64").is_none());
    }

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    #[test]
    fn extracts_real_bundled_archive_when_present() {
        let bundle = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("src-tauri")
            .join("resources")
            .join("frpc");
        if !bundle.join(BUNDLE_LOCK).is_file() {
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let prepared = bundled_blocking(&bundle, root.path(), "darwin", "amd64");
        let Some(prepared) = prepared else {
            panic!("bundled frpc should be usable");
        };
        let bytes = std::fs::read(&prepared.path).unwrap();
        assert!(executable_magic_ok(&bytes));
        assert!(bytes.len() > 1_000_000);
    }
}
