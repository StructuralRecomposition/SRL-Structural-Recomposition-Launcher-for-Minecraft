use crate::download::{download_file, DownloadManager, DownloadSpec, TaskHandle, TaskStatus};
use crate::error::{AppError, ErrorCode, Result};
use crate::paths::{AppPaths, GamePaths};
use crate::settings::Settings;
use crate::util;
use crate::version::{InstallPlan, NativeExtract, RuleContext, VersionService};
use futures_util::stream::{self, StreamExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct InstallOptions {
    pub include_assets: bool,
    pub concurrency: u32,
    pub allow_custom_hosts: bool,
}

impl InstallOptions {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            include_assets: settings.download_assets_on_install,
            concurrency: settings.max_concurrent_downloads,
            allow_custom_hosts: settings.allow_custom_hosts,
        }
    }
}

/// Installer bundles the services needed for one installation. Keeping this
/// as a struct avoids long argument lists and makes the call sites explicit.
pub struct Installer<'a> {
    pub versions: &'a VersionService,
    pub http: &'a reqwest::Client,
    pub paths: &'a AppPaths,
    pub game: &'a GamePaths,
    pub downloads: &'a DownloadManager,
    /// Pre-resolved metadata for loader installs (base files already exist).
    /// When set, remote resolution and asset enumeration are skipped.
    pub prepared: Option<crate::version::VersionJson>,
    pub options: InstallOptions,
}

impl Installer<'_> {
    /// Install a Minecraft version: resolve metadata, download and verify all
    /// files, extract natives, then write the resolved metadata as the final
    /// step. The version is only listed as installed after every step succeeds.
    pub async fn install(&self, task: TaskHandle, version_id: &str) -> Result<()> {
        let cancel = task.cancel_token();
        self.game.ensure_layout().await?;

        let result = self.run(&task, &cancel, version_id).await;

        match result {
            Ok(()) => {
                task.complete().await;
                Ok(())
            }
            Err(e) if e.code == ErrorCode::Cancelled => {
                task.cancelled().await;
                Err(e)
            }
            Err(e) => {
                task.fail(e.clone()).await;
                Err(e)
            }
        }
    }

    async fn run(
        &self,
        task: &TaskHandle,
        cancel: &CancellationToken,
        version_id: &str,
    ) -> Result<()> {
        let versions = self.versions;
        let http = self.http;
        let paths = self.paths;
        let game = self.game;
        let downloads = self.downloads;
        let options = &self.options;
        task.set_status(TaskStatus::Running).await;
        task.set_current_file("获取版本元数据…").await;

        let prepared = self.prepared.is_some();
        let resolved = match &self.prepared {
            Some(v) => v.clone(),
            None => versions.resolve_remote(paths, version_id).await?,
        };
        if cancel.is_cancelled() {
            return Err(AppError::cancelled());
        }

        let index = if prepared {
            None
        } else {
            task.set_current_file("获取资源索引…").await;
            Some(
                versions
                    .ensure_asset_index(game, &resolved, options.allow_custom_hosts)
                    .await?,
            )
        };

        let include_assets = options.include_assets && !prepared;
        let ctx = RuleContext::default();
        let plan: InstallPlan = VersionService::build_install_plan(
            game,
            &resolved,
            index.as_ref(),
            include_assets,
            &ctx,
        )?;

        if !plan.warnings.is_empty() {
            tracing::warn!(warnings = ?plan.warnings, "install plan warnings");
        }

        task.set_total_files(plan.files.len() as u32).await;
        task.set_total_bytes(plan.total_bytes).await;
        if plan.unknown_size_files > 0 {
            tracing::info!(
                count = plan.unknown_size_files,
                "files without size metadata"
            );
        }

        // One absolute counter per planned file. `download_file` reports
        // absolute bytes for the current file, so summing these counters yields
        // accurate aggregate progress even with retries or resumed downloads.
        let progress: Arc<Vec<AtomicU64>> =
            Arc::new((0..plan.files.len()).map(|_| AtomicU64::new(0)).collect());
        let done_flag = Arc::new(AtomicBool::new(false));
        let ticker = {
            let task = task.clone();
            let progress = progress.clone();
            let done_flag = done_flag.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(std::time::Duration::from_millis(350)) => {}
                        _ = cancel.cancelled() => break,
                    }
                    if done_flag.load(Ordering::Relaxed) {
                        break;
                    }
                    let total: u64 = progress.iter().map(|p| p.load(Ordering::Relaxed)).sum();
                    task.set_done_bytes(total).await;
                }
            })
        };

        let concurrency = options.concurrency.clamp(1, 16) as usize;
        let files = plan.files.clone();
        let result = async {
            let mut stream = stream::iter(files.into_iter().enumerate().map(|(idx, file)| {
                let http = http.clone();
                let task = task.clone();
                let cancel = cancel.clone();
                let progress = progress.clone();
                let allow_custom = options.allow_custom_hosts;
                async move {
                    if cancel.is_cancelled() {
                        return Err(AppError::cancelled());
                    }
                    // Global limiter shared by every install task.
                    let _permit = downloads.acquire().await;
                    // Deterministic temp paths require a per-destination lock.
                    let _target = downloads.lock_target(&file.dest).await;
                    task.set_current_file(&file.label).await;
                    let label = file.label.clone();
                    let outcome = download_file(
                        &http,
                        &DownloadSpec {
                            url: &file.url,
                            dest: &file.dest,
                            sha1: file.sha1.as_deref(),
                            size: file.size,
                        },
                        &cancel,
                        allow_custom,
                        |absolute| {
                            progress[idx].store(absolute, Ordering::Relaxed);
                        },
                    )
                    .await?;
                    if matches!(outcome, crate::download::DownloadOutcome::Skipped) {
                        if let Some(size) = file.size {
                            progress[idx].store(size, Ordering::Relaxed);
                        }
                    }
                    tracing::debug!(file = %label, ?outcome, "downloaded");
                    task.file_finished().await;
                    Ok(())
                }
            }))
            .buffer_unordered(concurrency);

            while let Some(item) = stream.next().await {
                item?;
            }
            Ok::<(), AppError>(())
        }
        .await;

        done_flag.store(true, Ordering::Relaxed);
        ticker.abort();

        result?;

        if cancel.is_cancelled() {
            return Err(AppError::cancelled());
        }

        task.set_status(TaskStatus::Verifying).await;
        task.set_current_file("解压 natives…").await;
        if !plan.natives.is_empty() {
            // Clear once before extracting all native archives; each archive must
            // add to the same directory instead of wiping previous results.
            let natives_dir = game.natives_dir(&resolved.id);
            if tokio::fs::try_exists(&natives_dir).await.unwrap_or(false) {
                tokio::fs::remove_dir_all(&natives_dir).await?;
            }
            util::ensure_dir(&natives_dir).await?;
        }
        for native in &plan.natives {
            if cancel.is_cancelled() {
                return Err(AppError::cancelled());
            }
            extract_natives(native).await?;
        }

        if let Some(index) = &index {
            task.set_current_file("整理资源…").await;
            if let Err(e) =
                crate::version::materialize_virtual_assets(game, index, &resolved.id).await
            {
                // Virtual assets are only needed by very old versions; a
                // failure here does not invalidate the download.
                tracing::warn!(error = %e, "failed to materialize virtual assets");
            }
        }

        task.set_current_file("写入版本元数据…").await;
        versions.save_installed_metadata(game, &resolved).await?;

        // Final safety check: the client jar must exist and match its hash.
        let client = resolved.client_download()?;
        let jar = game.version_jar(&resolved.jar_id());
        if !tokio::fs::try_exists(&jar).await.unwrap_or(false) {
            return Err(AppError::new(
                ErrorCode::ChecksumMismatch,
                "安装结束后客户端 JAR 缺失",
            ));
        }
        let actual = util::sha1_file(&jar).await?;
        if !actual.eq_ignore_ascii_case(&client.sha1) {
            return Err(AppError::checksum(&client.sha1, &actual));
        }

        Ok(())
    }
}

/// Safely extract a natives archive. Rejects traversal, absolute paths, symlinks
/// and enforces a size ceiling to bound zip-bomb damage.
pub async fn extract_natives(native: &NativeExtract) -> Result<u32> {
    let archive = native.archive.clone();
    let dest = native.dest_dir.clone();
    let exclude = native.exclude.clone();
    tokio::task::spawn_blocking(move || extract_natives_blocking(&archive, &dest, &exclude))
        .await
        .map_err(|e| AppError::internal(format!("解压任务失败：{e}")))?
}

const MAX_EXTRACT_BYTES: u64 = 512 * 1024 * 1024;

fn extract_natives_blocking(archive: &Path, dest: &Path, exclude: &[String]) -> Result<u32> {
    let file = std::fs::File::open(archive).map_err(|e| {
        AppError::new(
            ErrorCode::Io,
            format!("无法打开 natives 压缩包：{}", archive.display()),
        )
        .with_detail(e.to_string())
    })?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| {
        AppError::new(ErrorCode::ZipUnsafe, "natives 压缩包损坏").with_detail(e.to_string())
    })?;

    // The caller clears the shared natives directory once; individual archives
    // must not delete each other's files.
    std::fs::create_dir_all(dest)?;

    let mut total = 0u64;
    let mut count = 0u32;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| {
            AppError::new(ErrorCode::ZipUnsafe, "无法读取压缩包条目").with_detail(e.to_string())
        })?;
        let name = entry.name()?.to_string();
        if name.ends_with('/') {
            continue;
        }
        if exclude.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        // Symlinks and other special entries must not be materialised.
        if let Some(mode) = entry.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                continue;
            }
        }
        let rel = entry.enclosed_name().ok_or_else(|| {
            AppError::new(ErrorCode::ZipUnsafe, format!("压缩包内含非法路径：{name}"))
        })?;
        let target = util::safe_join(dest, &rel.to_string_lossy())?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        total += entry.size();
        if total > MAX_EXTRACT_BYTES {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                "natives 解压后体积超过安全上限",
            ));
        }
        let mut out = std::fs::File::create(&target)?;
        std::io::copy(&mut entry, &mut out)?;
        count += 1;
    }
    Ok(count)
}

/// Resolve and install a Fabric-style version JSON that is already on disk
/// (used by loader adapters in later phases).
pub async fn install_from_local_json(
    versions: &VersionService,
    game: &GamePaths,
    task: TaskHandle,
    json_bytes: &[u8],
    options: &InstallOptions,
) -> Result<()> {
    let cancel = task.cancel_token();
    let resolved = {
        let child = crate::version::VersionJson::parse(json_bytes)?;
        if child.inherits_from.is_some() {
            return Err(AppError::unsupported(
                "本地 JSON 安装仅支持已解析的完整元数据",
            ));
        }
        child
    };
    let index = versions
        .ensure_asset_index(game, &resolved, options.allow_custom_hosts)
        .await?;
    let plan = VersionService::build_install_plan(
        game,
        &resolved,
        Some(&index),
        options.include_assets,
        &RuleContext::default(),
    )?;
    task.set_total_files(plan.files.len() as u32).await;
    task.set_total_bytes(plan.total_bytes).await;
    for file in &plan.files {
        if cancel.is_cancelled() {
            task.cancelled().await;
            return Err(AppError::cancelled());
        }
        task.set_current_file(&file.label).await;
        download_file(
            versions.http(),
            &DownloadSpec {
                url: &file.url,
                dest: &file.dest,
                sha1: file.sha1.as_deref(),
                size: file.size,
            },
            &cancel,
            options.allow_custom_hosts,
            |_n| {},
        )
        .await?;
        task.file_finished().await;
    }
    task.complete().await;
    Ok(())
}

/// Compute the directory a version's loader-specific files should live in.
pub fn version_staging_dir(game: &GamePaths, version_id: &str) -> PathBuf {
    game.version_dir(version_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, name: &str, bytes: &[u8]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn multiple_native_archives_are_merged_not_wiped() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("natives");
        let a = dir.path().join("a.jar");
        let b = dir.path().join("b.jar");
        make_zip(&a, "liba.dylib", b"AAA");
        make_zip(&b, "libb.dylib", b"BBB");

        let first = NativeExtract {
            archive: a,
            dest_dir: dest.clone(),
            exclude: vec![],
        };
        let second = NativeExtract {
            archive: b,
            dest_dir: dest.clone(),
            exclude: vec![],
        };
        extract_natives_blocking(&first.archive, &first.dest_dir, &first.exclude).unwrap();
        extract_natives_blocking(&second.archive, &second.dest_dir, &second.exclude).unwrap();

        assert_eq!(std::fs::read(dest.join("liba.dylib")).unwrap(), b"AAA");
        assert_eq!(std::fs::read(dest.join("libb.dylib")).unwrap(), b"BBB");
    }

    async fn serve_files(files: Vec<(String, Vec<u8>)>) -> std::net::SocketAddr {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let files = files.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let mut raw = Vec::new();
                    loop {
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let req = String::from_utf8_lossy(&raw);
                    let path = req
                        .lines()
                        .next()
                        .unwrap_or("")
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("");
                    let body = files
                        .iter()
                        .find(|(name, _)| path.ends_with(name))
                        .map(|(_, b)| b.clone())
                        .unwrap_or_default();
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn aggregate_progress_never_exceeds_total() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().join("data"));
        let game = GamePaths::new(dir.path().join("mc"));
        paths.ensure_layout().await.unwrap();
        game.ensure_layout().await.unwrap();
        let client = b"client-jar-bytes".to_vec();
        let lib1 = vec![7u8; 120_000];
        let lib2 = vec![9u8; 80_000];
        let addr = serve_files(vec![
            ("/client.jar".into(), client.clone()),
            ("/lib1.jar".into(), lib1.clone()),
            ("/lib2.jar".into(), lib2.clone()),
        ])
        .await;
        let sha = util::sha1_bytes;
        let version: crate::version::VersionJson = serde_json::from_value(serde_json::json!({
            "id": "1.0.0",
            "type": "release",
            "mainClass": "M",
            "assets": "x",
            "assetIndex": {"id": "x", "url": "https://piston-meta.mojang.com/x"},
            "downloads": {"client": {
                "sha1": sha(&client), "size": client.len(),
                "url": format!("http://{addr}/client.jar")
            }},
            "javaVersion": {"majorVersion": 17},
            "libraries": [
                {"name": "g:a:1", "downloads": {"artifact": {
                    "path": "g/a/1/a-1.jar", "sha1": sha(&lib1), "size": lib1.len(),
                    "url": format!("http://{addr}/lib1.jar")}}},
                {"name": "g:b:1", "downloads": {"artifact": {
                    "path": "g/b/1/b-1.jar", "sha1": sha(&lib2), "size": lib2.len(),
                    "url": format!("http://{addr}/lib2.jar")}}}
            ],
            "arguments": {"game": [], "jvm": []}
        }))
        .unwrap();
        let total = (client.len() + lib1.len() + lib2.len()) as u64;
        let http = reqwest::Client::new();
        let versions = VersionService::new(http.clone());
        let downloads = DownloadManager::new(Arc::new(|_| {}));
        let task = downloads.create("install", "p", 0, 0).await;
        let installer = Installer {
            versions: &versions,
            http: &http,
            paths: &paths,
            game: &game,
            downloads: &downloads,
            prepared: Some(version),
            options: InstallOptions {
                include_assets: false,
                concurrency: 3,
                allow_custom_hosts: false,
            },
        };
        let run = installer.install(task.clone(), "1.0.0");
        let mut max_seen = 0u64;
        tokio::pin!(run);
        loop {
            tokio::select! {
                result = &mut run => {
                    result.unwrap();
                    break;
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(15)) => {
                    if let Some(view) = downloads.list().await.first() {
                        max_seen = max_seen.max(view.done_bytes);
                        assert!(view.done_bytes <= total, "progress {} > total {}", view.done_bytes, total);
                    }
                }
            }
        }
        let final_view = downloads.list().await.remove(0);
        assert_eq!(final_view.status, TaskStatus::Completed);
        assert_eq!(final_view.done_bytes, total);
        assert!(max_seen <= total);
    }

    #[test]
    fn excludes_and_traversal_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("bad.zip");
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("META-INF/MANIFEST.MF", options).unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
        let dest = dir.path().join("out");
        let count = extract_natives_blocking(&archive, &dest, &["META-INF/".to_string()]).unwrap();
        assert_eq!(count, 0);
        assert!(!dest.join("META-INF").exists());
    }
}
