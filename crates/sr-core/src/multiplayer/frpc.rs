use crate::error::{AppError, ErrorCode, Result};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;
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
#[serde(rename_all = "camelCase")]
pub struct FrpcAsset {
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub download_link: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub kind: String,
    /// Optional official checksum. The v3 documentation does not currently
    /// list one, but if the API starts returning it we verify it.
    #[serde(default)]
    pub sha256: Option<String>,
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
        // A previous crash may have left a private config file behind. Remove
        // only our own `frpc-*.toml` files; never touch unrelated files.
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

    pub async fn assets(&self, os: &str, arch: &str) -> Result<Vec<FrpcAsset>> {
        let bases = [super::types::API_BASE, super::types::API_BACKUP_BASE];
        let mut last_error = None;
        for base in bases {
            let response = self
                .http
                .get(format!("{base}/software/frp"))
                .query(&[("os", os), ("arch", arch)])
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await;
            match response {
                Ok(response) => {
                    let value: Value = response.json().await.map_err(|e| {
                        AppError::new(ErrorCode::Http, "无法解析 LoCyanFrp 软件列表")
                            .with_detail(e.to_string())
                    })?;
                    let list = value
                        .get("data")
                        .and_then(|d| d.get("list"))
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let mut assets = Vec::new();
                    for item in list {
                        let mut asset: FrpcAsset =
                            serde_json::from_value(item.clone()).unwrap_or(FrpcAsset {
                                os: String::new(),
                                arch: String::new(),
                                name: String::new(),
                                version: String::new(),
                                download_link: String::new(),
                                tag: String::new(),
                                kind: String::new(),
                                sha256: None,
                            });
                        if asset.download_link.is_empty() {
                            asset.download_link = item
                                .get("downloadLink")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                        }
                        if asset.kind.is_empty() {
                            asset.kind = item
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                        }
                        assets.push(asset);
                    }
                    return Ok(assets);
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(AppError::network("无法获取 LoCyanFrp 客户端列表")
            .with_detail(last_error.map(|e| e.to_string()).unwrap_or_default()))
    }

    pub async fn prepare(&self, os: &str, arch: &str) -> Result<PreparedFrpc> {
        let assets = self.assets(os, arch).await?;
        let asset = pick_asset(&assets, os, arch)
            .ok_or_else(|| AppError::not_found("官方资源列表中没有当前平台的 FRPC 客户端"))?;
        if asset.download_link.is_empty() {
            return Err(AppError::new(
                ErrorCode::Http,
                "LoCyanFrp 没有返回 FRPC 下载地址",
            ));
        }
        verify_download_url(&asset.download_link)?;

        let safe_version = sanitize_component(if asset.version.is_empty() {
            &asset.name
        } else {
            &asset.version
        });
        let target_dir = self.dir.join(format!("{safe_version}-{arch}"));
        let exe_name = if cfg!(windows) { "frpc.exe" } else { "frpc" };
        let target = target_dir.join(exe_name);
        let hash_sidecar = target_dir.join("frpc.sha256");
        if target.is_file() {
            if let Ok(bytes) = std::fs::read(&target) {
                if executable_magic_ok(&bytes) {
                    let actual = sha256_hex(&bytes);
                    let expected = std::fs::read_to_string(&hash_sidecar)
                        .ok()
                        .map(|v| v.trim().to_string());
                    if expected.as_deref().is_none_or(|value| value == actual) {
                        let _ = std::fs::write(&hash_sidecar, &actual);
                        return Ok(PreparedFrpc {
                            sha256: actual,
                            version: asset.version.clone(),
                            path: target,
                        });
                    }
                }
            }
        }

        crate::util::ensure_dir(&target_dir).await?;
        let bytes = self.download(&asset.download_link).await?;
        let extracted = extract_executable(&bytes, os)?;
        if !executable_magic_ok(&extracted) {
            return Err(AppError::new(
                ErrorCode::ChecksumMismatch,
                "下载的 FRPC 不是有效的可执行文件，已拒绝运行",
            ));
        }
        let sha256 = sha256_hex(&extracted);
        if let Some(expected) = asset
            .sha256
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            if !expected.eq_ignore_ascii_case(&sha256) {
                return Err(AppError::checksum(expected, &sha256));
            }
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
            version: asset.version.clone(),
            sha256,
        })
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

pub fn pick_asset<'a>(assets: &'a [FrpcAsset], os: &str, arch: &str) -> Option<&'a FrpcAsset> {
    let os_aliases = os_aliases(os);
    let arch_aliases = arch_aliases(arch);
    let mut candidates: Vec<&FrpcAsset> = assets
        .iter()
        .filter(|a| {
            os_aliases.iter().any(|v| a.os.eq_ignore_ascii_case(v))
                && arch_aliases.iter().any(|v| a.arch.eq_ignore_ascii_case(v))
                && !a.download_link.is_empty()
        })
        .collect();
    candidates.sort_by(|a, b| b.version.cmp(&a.version));
    candidates.into_iter().next()
}

fn os_aliases(os: &str) -> Vec<&'static str> {
    match os.to_ascii_lowercase().as_str() {
        "macos" | "darwin" | "osx" => vec!["darwin", "macos", "osx"],
        "windows" | "win" => vec!["windows", "win"],
        _ => vec!["linux"],
    }
}

fn arch_aliases(arch: &str) -> Vec<&'static str> {
    match arch.to_ascii_lowercase().as_str() {
        "aarch64" | "arm64" => vec!["arm64", "aarch64"],
        "x86" | "i686" => vec!["386", "x86", "i386"],
        _ => vec!["amd64", "x86_64", "x64"],
    }
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
    let allowed = host == "locyanfrp.cn"
        || host.ends_with(".locyanfrp.cn")
        || host == "locyan.cn"
        || host.ends_with(".locyan.cn")
        || host == "github.com"
        || host.ends_with(".githubusercontent.com");
    if !allowed {
        return Err(AppError::path_unsafe(format!(
            "FRPC 下载来源不在官方域名内：{host}"
        )));
    }
    Ok(())
}

fn sanitize_component(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .take(48)
        .collect();
    if cleaned.is_empty() {
        "unknown".into()
    } else {
        cleaned
    }
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
    fn picks_matching_asset() {
        let assets = vec![
            FrpcAsset {
                os: "linux".into(),
                arch: "amd64".into(),
                name: "frpc".into(),
                version: "0.61.0".into(),
                download_link: "https://a".into(),
                tag: String::new(),
                kind: "zip".into(),
                sha256: None,
            },
            FrpcAsset {
                os: "linux".into(),
                arch: "amd64".into(),
                name: "frpc".into(),
                version: "0.62.0".into(),
                download_link: "https://b".into(),
                tag: String::new(),
                kind: "zip".into(),
                sha256: None,
            },
        ];
        assert_eq!(
            pick_asset(&assets, "linux", "x86_64").unwrap().version,
            "0.62.0"
        );
    }

    #[test]
    fn rejects_executable_from_random_host() {
        assert!(verify_download_url("https://evil.example/frpc.zip").is_err());
        assert!(verify_download_url("https://cdn.locyanfrp.cn/frpc.zip").is_ok());
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
