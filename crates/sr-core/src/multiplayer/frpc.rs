use crate::error::{AppError, ErrorCode, Result};
use futures_util::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::Mutex;

use super::types::FrpcView;

const MAX_FRPC_BYTES: u64 = 100 * 1024 * 1024;

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

pub struct FrpcManager {
    dir: PathBuf,
    http: reqwest::Client,
    state: Mutex<FrpcState>,
}

impl FrpcManager {
    pub fn new(dir: PathBuf, http: reqwest::Client) -> Self {
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

    /// Download the upstream fatedier/frp client. 52FRP's tutorial explicitly
    /// uses the official frp releases, and those releases publish SHA-256
    /// checksums.
    pub async fn prepare(&self) -> Result<PreparedFrpc> {
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

        // GitHub's release API exposes the asset digest. Prefer it; fall back
        // to the checksum text asset only when the digest is absent.
        let expected = if let Some(digest) = asset
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
            let checksum_text = self.download_text(&checksums.browser_download_url).await?;
            checksum_for(&checksum_text, &asset.name).ok_or_else(|| {
                AppError::new(
                    ErrorCode::ChecksumMismatch,
                    "frp Release 的校验文件中没有当前客户端",
                )
            })?
        };

        let target_dir = self.dir.join(format!("{version}-{platform}-{arch}"));
        let target = target_dir.join(if platform == "windows" {
            "frpc.exe"
        } else {
            "frpc"
        });
        let hash_sidecar = target_dir.join("frpc.sha256");
        if target.is_file() {
            if let Ok(bytes) = std::fs::read(&target) {
                if executable_magic_ok(&bytes) {
                    let actual = sha256_hex(&bytes);
                    let cached = std::fs::read_to_string(&hash_sidecar)
                        .ok()
                        .map(|v| v.trim().to_string());
                    if actual.eq_ignore_ascii_case(&expected)
                        && cached.as_deref().is_none_or(|v| v == actual)
                    {
                        return Ok(PreparedFrpc {
                            path: target,
                            version,
                            sha256: actual,
                        });
                    }
                }
            }
        }

        crate::util::ensure_dir(&target_dir).await?;
        let bytes = self
            .download_with_fallback(&asset.browser_download_url)
            .await?;
        let extracted = extract_executable(&bytes, platform)?;
        if !executable_magic_ok(&extracted) {
            return Err(AppError::new(
                ErrorCode::ChecksumMismatch,
                "下载的 FRPC 不是有效的可执行文件，已拒绝运行",
            ));
        }
        let sha256 = sha256_hex(&extracted);
        if !sha256.eq_ignore_ascii_case(&expected) {
            return Err(AppError::checksum(&expected, &sha256));
        }
        std::fs::write(&target, &extracted)?;
        let _ = std::fs::write(&hash_sidecar, &sha256);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
        }
        Ok(PreparedFrpc {
            path: target,
            version,
            sha256,
        })
    }

    async fn release(&self) -> Result<GitHubRelease> {
        let response = self
            .http
            .get("https://api.github.com/repos/fatedier/frp/releases/latest")
            .header("Accept", "application/vnd.github+json")
            .timeout(std::time::Duration::from_secs(30))
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
            .timeout(std::time::Duration::from_secs(30))
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

    async fn download_with_fallback(&self, url: &str) -> Result<Vec<u8>> {
        let mut last = None;
        for candidate in [
            url.to_string(),
            format!("https://ghproxy.com/{url}"),
            format!("https://gh-proxy.com/{url}"),
            format!("https://mirror.ghproxy.com/{url}"),
        ] {
            match self.download(&candidate).await {
                Ok(bytes) => return Ok(bytes),
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or_else(|| AppError::network("下载 FRPC 失败")))
    }

    async fn download(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| AppError::network("下载 FRPC 失败").with_detail(e.to_string()))?;
        if !response.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Http,
                format!("下载 FRPC 返回 HTTP {}", response.status().as_u16()),
            ));
        }
        if let Some(length) = response.content_length() {
            if length > MAX_FRPC_BYTES {
                return Err(AppError::new(
                    ErrorCode::Http,
                    "FRPC 下载文件超过 100 MB，已拒绝",
                ));
            }
        }
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
        }
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

        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
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
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
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
        let log_tail = state.log.lock().await.iter().take(40).cloned().collect();
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
            let name = file.name().to_ascii_lowercase();
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
            if path.ends_with("/frpc")
                || path.ends_with("/frpc.exe")
                || path == "frpc"
                || path == "frpc.exe"
            {
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
}
