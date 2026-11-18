//! Launcher self-update: GitHub release discovery, mirror-aware download and
//! a platform-specific hand-off script. The download is verified against the
//! SHA-256 digest published by GitHub when one is available.

use crate::state::AppState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sr_core::error::{AppError, ErrorCode, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter, State};

const API: &str = "https://api.github.com/repos/StructuralRecomposition/SRL-Structural-Recomposition-Launcher-for-Minecraft/releases/latest";
const MIRRORS: &[&str] = &["https://gh-proxy.com/", "https://ghfast.top/"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub install_kind: String,
    pub available: bool,
    pub release_url: String,
    pub notes: String,
    pub published_at: Option<String>,
    pub assets: Vec<UpdateAsset>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: u64,
    pub percent: f64,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn parse_version(value: &str) -> (Vec<u64>, bool) {
    let raw = value.trim().trim_start_matches('v');
    let (core, pre) = raw.split_once('-').unwrap_or((raw, ""));
    let parts = core
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect();
    (parts, !pre.is_empty())
}

fn is_newer(latest: &str, current: &str) -> bool {
    let (a, a_pre) = parse_version(latest);
    let (b, b_pre) = parse_version(current);
    let len = a.len().max(b.len());
    for i in 0..len {
        let av = a.get(i).copied().unwrap_or(0);
        let bv = b.get(i).copied().unwrap_or(0);
        if av != bv {
            return av > bv;
        }
    }
    // A final release supersedes a prerelease with the same core version.
    b_pre && !a_pre
}

fn install_kind() -> String {
    #[cfg(target_os = "macos")]
    {
        "dmg".to_string()
    }
    #[cfg(target_os = "windows")]
    {
        "zip".to_string()
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("APPIMAGE").is_some() {
            "appimage".to_string()
        } else if std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|p| p.to_path_buf()))
            .map(|dir| {
                dir.as_path() == Path::new("/usr/bin") || dir.as_path() == Path::new("/usr/lib")
            })
            .unwrap_or(false)
        {
            "deb".to_string()
        } else {
            "appimage".to_string()
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "unknown".to_string()
    }
}

#[tauri::command]
pub async fn check_update(state: State<'_, AppState>) -> Result<UpdateInfo> {
    let response = state
        .http
        .get(API)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| AppError::new(ErrorCode::Http, format!("无法连接 GitHub：{e}")))?;

    let status = response.status();
    if !status.is_success() {
        let message = if status.as_u16() == 404 {
            "GitHub 上还没有可用的 release".to_string()
        } else {
            format!("GitHub 返回 HTTP {}", status.as_u16())
        };
        return Err(AppError::new(ErrorCode::Http, message));
    }

    let release: GhRelease = response
        .json()
        .await
        .map_err(|e| AppError::new(ErrorCode::Http, format!("无法解析 release 信息：{e}")))?;

    let latest = release.tag_name.trim_start_matches('v').to_string();
    let current = current_version();
    let assets = release
        .assets
        .into_iter()
        .filter(|asset| {
            let name = asset.name.to_ascii_lowercase();
            name.ends_with(".dmg")
                || name.ends_with(".zip")
                || name.ends_with(".appimage")
                || name.ends_with(".deb")
        })
        .map(|asset| UpdateAsset {
            name: asset.name,
            url: asset.browser_download_url,
            size: asset.size,
            sha256: asset
                .digest
                .and_then(|digest| digest.strip_prefix("sha256:").map(str::to_string))
                .filter(|value| value.len() == 64),
        })
        .collect();

    Ok(UpdateInfo {
        available: is_newer(&latest, &current),
        install_kind: install_kind(),
        current_version: current,
        latest_version: latest,
        release_url: release.html_url,
        notes: release.body.unwrap_or_default(),
        published_at: release.published_at,
        assets,
    })
}

fn safe_file_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if !(lower.ends_with(".dmg")
        || lower.ends_with(".zip")
        || lower.ends_with(".appimage")
        || lower.ends_with(".deb"))
    {
        return None;
    }
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    if cleaned.is_empty() || cleaned != name {
        None
    } else {
        Some(cleaned)
    }
}

fn download_candidates(url: &str) -> Vec<String> {
    let mut out = Vec::new();
    if url.starts_with("https://github.com/") {
        for mirror in MIRRORS {
            out.push(format!("{mirror}{url}"));
        }
    }
    out.push(url.to_string());
    out
}

async fn download_one(
    app: &AppHandle,
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
) -> Result<u64> {
    let response = client
        .get(url)
        .timeout(std::time::Duration::from_secs(900))
        .send()
        .await
        .map_err(|e| AppError::new(ErrorCode::Http, format!("下载失败：{e}")))?;
    if !response.status().is_success() {
        return Err(AppError::new(
            ErrorCode::Http,
            format!("下载返回 HTTP {}", response.status().as_u16()),
        ));
    }
    let total = response.content_length().unwrap_or(0);
    let mut response = response;
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| AppError::new(ErrorCode::Io, format!("无法写入更新文件：{e}")))?;
    let mut downloaded = 0u64;
    let mut last_emit = 0u64;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AppError::new(ErrorCode::Http, format!("下载中断：{e}")))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|e| AppError::new(ErrorCode::Io, format!("写入更新文件失败：{e}")))?;
        downloaded += chunk.len() as u64;
        if downloaded - last_emit > 512 * 1024 || downloaded == total {
            last_emit = downloaded;
            let percent = if total > 0 {
                downloaded as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            let _ = app.emit(
                "update-progress",
                UpdateProgress {
                    downloaded,
                    total,
                    percent,
                },
            );
        }
    }
    file.flush()
        .await
        .map_err(|e| AppError::new(ErrorCode::Io, format!("写入更新文件失败：{e}")))?;
    Ok(downloaded)
}

#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    name: String,
    sha256: Option<String>,
) -> Result<String> {
    let file_name =
        safe_file_name(&name).ok_or_else(|| AppError::invalid_input("更新文件名无效"))?;
    let parsed = reqwest::Url::parse(&url).map_err(|_| AppError::invalid_input("更新地址无效"))?;
    if parsed.scheme() != "https" {
        return Err(AppError::invalid_input("更新地址必须使用 HTTPS"));
    }
    let host = parsed.host_str().unwrap_or_default();
    if !matches!(
        host,
        "github.com" | "objects.githubusercontent.com" | "api.github.com"
    ) {
        return Err(AppError::path_unsafe("更新地址不在允许的域名内"));
    }

    let dir = state.paths.base().join("updates");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::new(ErrorCode::Io, format!("无法创建更新目录：{e}")))?;
    let dest = dir.join(&file_name);

    let mut last_error = None;
    for candidate in download_candidates(&url) {
        let _ = tokio::fs::remove_file(&dest).await;
        match download_one(&app, &state.http, &candidate, &dest).await {
            Ok(bytes) => {
                if let Some(expected) = sha256.as_deref() {
                    let data = tokio::fs::read(&dest).await.map_err(|e| {
                        AppError::new(ErrorCode::Io, format!("无法校验更新文件：{e}"))
                    })?;
                    let actual = format!("{:x}", Sha256::digest(&data));
                    if !actual.eq_ignore_ascii_case(expected) {
                        let _ = tokio::fs::remove_file(&dest).await;
                        last_error =
                            Some(AppError::new(ErrorCode::Http, "更新文件 SHA-256 校验失败"));
                        continue;
                    }
                }
                if bytes == 0 {
                    last_error = Some(AppError::new(ErrorCode::Http, "更新文件为空"));
                    continue;
                }
                return Ok(dest.to_string_lossy().into_owned());
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| AppError::new(ErrorCode::Http, "更新下载失败")))
}

fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn spawn_macos(app: &AppHandle, state: &AppState, file: &Path, name: &str) -> Result<()> {
    if !name.to_ascii_lowercase().ends_with(".dmg") {
        return Err(AppError::invalid_input("macOS 更新需要 DMG 安装包"));
    }
    let exe = std::env::current_exe()
        .map_err(|e| AppError::new(ErrorCode::Io, format!("无法定位当前程序：{e}")))?;
    let bundle = exe
        .ancestors()
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("app"))
        .map(Path::to_path_buf);
    let script_path = state.paths.base().join("updates").join("apply-update.sh");
    let log_path = state.paths.base().join("updates").join("update.log");
    let mut script = String::from("#!/bin/sh\nsleep 2\n");
    script.push_str("DMG=");
    script.push_str(&sh_quote(&file.to_string_lossy()));
    script.push('\n');
    match bundle {
        Some(bundle) => {
            script.push_str("TARGET=");
            script.push_str(&sh_quote(&bundle.to_string_lossy()));
            script.push('\n');
            script.push_str(
                "MNT=$(mktemp -d \"${TMPDIR:-/tmp}/srl-update.XXXXXX\")\n\
                 if ! hdiutil attach -nobrowse -quiet -mountpoint \"$MNT\" \"$DMG\"; then\n\
                   open \"$DMG\"\n\
                   exit 1\n\
                 fi\n\
                 APP=$(find \"$MNT\" -maxdepth 1 -name '*.app' -print -quit)\n\
                 if [ -z \"$APP\" ]; then hdiutil detach \"$MNT\" -quiet; open \"$DMG\"; exit 1; fi\n\
                 rm -rf \"$TARGET\"\n\
                 ditto \"$APP\" \"$TARGET\"\n\
                 xattr -dr com.apple.quarantine \"$TARGET\" 2>/dev/null || true\n\
                 hdiutil detach \"$MNT\" -quiet 2>/dev/null || true\n\
                 open \"$TARGET\"\n",
            );
        }
        None => {
            script.push_str("open \"$DMG\"\n");
        }
    }
    std::fs::write(&script_path, script)
        .map_err(|e| AppError::new(ErrorCode::Io, format!("无法写入更新脚本：{e}")))?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| AppError::new(ErrorCode::Io, format!("无法写入更新日志：{e}")))?;
    Command::new("/bin/sh")
        .arg(&script_path)
        .stdin(Stdio::null())
        .stdout(
            log.try_clone()
                .map_err(|e| AppError::new(ErrorCode::Io, e.to_string()))?,
        )
        .stderr(log)
        .spawn()
        .map_err(|e| AppError::new(ErrorCode::Io, format!("无法启动更新程序：{e}")))?;
    app.exit(0);
    Ok(())
}

#[cfg(target_os = "windows")]
fn spawn_windows(app: &AppHandle, state: &AppState, file: &Path, name: &str) -> Result<()> {
    use std::os::windows::process::CommandExt;
    if !name.to_ascii_lowercase().ends_with(".zip") {
        return Err(AppError::invalid_input("Windows 更新需要便携 ZIP"));
    }
    let exe = std::env::current_exe()
        .map_err(|e| AppError::new(ErrorCode::Io, format!("无法定位当前程序：{e}")))?;
    let dest = exe
        .parent()
        .ok_or_else(|| AppError::new(ErrorCode::Io, "无法定位安装目录"))?;
    let script_path = state.paths.base().join("updates").join("apply-update.ps1");
    let quote = |value: &str| format!("'{}'", value.replace('\'', "''"));
    let script = format!(
        "$ErrorActionPreference='Stop'\nStart-Sleep -Seconds 2\nExpand-Archive -LiteralPath {} -DestinationPath {} -Force\nStart-Process -FilePath {}\n",
        quote(&file.to_string_lossy()),
        quote(&dest.to_string_lossy()),
        quote(&exe.to_string_lossy())
    );
    std::fs::write(&script_path, script)
        .map_err(|e| AppError::new(ErrorCode::Io, format!("无法写入更新脚本：{e}")))?;
    Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script_path)
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::new(ErrorCode::Io, format!("无法启动更新程序：{e}")))?;
    app.exit(0);
    Ok(())
}

#[cfg(target_os = "linux")]
fn spawn_linux(app: &AppHandle, state: &AppState, file: &Path, name: &str) -> Result<()> {
    let lower = name.to_ascii_lowercase();
    let script_path = state.paths.base().join("updates").join("apply-update.sh");
    let mut script = String::from("#!/bin/sh\nsleep 2\n");
    script.push_str("FILE=");
    script.push_str(&sh_quote(&file.to_string_lossy()));
    script.push('\n');
    if lower.ends_with(".appimage") {
        let target = std::env::var("APPIMAGE").unwrap_or_else(|_| {
            std::env::current_exe()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        script.push_str("TARGET=");
        script.push_str(&sh_quote(&target));
        script.push('\n');
        script.push_str(
            "cp \"$FILE\" \"$TARGET\"\nchmod +x \"$TARGET\"\n\"$TARGET\" >/dev/null 2>&1 &\n",
        );
    } else if lower.ends_with(".deb") {
        script.push_str(
            "if command -v pkexec >/dev/null 2>&1; then\n\
               pkexec dpkg -i \"$FILE\" && (nohup sr-launcher >/dev/null 2>&1 &)\n\
             else\n\
               xdg-open \"$FILE\"\n\
             fi\n",
        );
    } else {
        return Err(AppError::invalid_input("不支持的 Linux 更新包"));
    }
    std::fs::write(&script_path, script)
        .map_err(|e| AppError::new(ErrorCode::Io, format!("无法写入更新脚本：{e}")))?;
    Command::new("/bin/sh")
        .arg(&script_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::new(ErrorCode::Io, format!("无法启动更新程序：{e}")))?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub async fn apply_update(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> Result<()> {
    let file = PathBuf::from(&path);
    if !file.exists() {
        return Err(AppError::invalid_input("更新文件不存在"));
    }
    #[cfg(target_os = "macos")]
    {
        return spawn_macos(&app, &state, &file, &name);
    }
    #[cfg(target_os = "windows")]
    {
        return spawn_windows(&app, &state, &file, &name);
    }
    #[cfg(target_os = "linux")]
    {
        return spawn_linux(&app, &state, &file, &name);
    }
    #[allow(unreachable_code)]
    Err(AppError::new(
        ErrorCode::Unsupported,
        "当前平台不支持自动更新",
    ))
}
