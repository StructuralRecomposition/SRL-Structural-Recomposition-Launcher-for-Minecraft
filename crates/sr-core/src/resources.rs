//! Resource installers: verified downloads from Modrinth, single-file installs
//! into instance folders, and `.mrpack` modpack installation.
//!
//! Every download goes through `download_file`, which enforces HTTPS, the
//! trusted host list, and SHA-1 verification. Modpack manifests are parsed as
//! data only; nothing is executed.

use crate::curseforge::{CurseForgeFile, CurseForgeService};
use crate::download::{download_file, DownloadOutcome, DownloadSpec, TaskHandle, TaskStatus};
use crate::error::{AppError, ErrorCode, Result};
use crate::instance::Instance;
use crate::modrinth::{ResourceFile, ResourceVersion};
use crate::resourcepacks;
use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const MAX_OVERRIDE_FILES: u64 = 50_000;
const MAX_OVERRIDE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_INDEX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ResourceTarget {
    Instance { instance_id: String },
    Directory { path: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledResource {
    pub path: PathBuf,
    pub file_name: String,
}

pub fn primary_file(version: &ResourceVersion) -> Result<&ResourceFile> {
    version
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| AppError::not_found("该版本没有可下载文件"))
}

pub fn subdir_for_kind(kind: &str) -> Result<&'static str> {
    match kind {
        "mod" => Ok("mods"),
        "shader" => Ok("shaderpacks"),
        "resourcepack" => Ok("resourcepacks"),
        _ => Err(AppError::invalid_input("该资源类型不能直接装入实例")),
    }
}

fn instance_loader(instance: &Instance) -> Option<String> {
    instance
        .mod_loader
        .as_ref()
        .map(|m| m.kind.to_ascii_lowercase())
        .or_else(|| {
            let id = instance.version_id.to_ascii_lowercase();
            if id.contains("-forge-") {
                Some("forge".into())
            } else if id.contains("-neoforge-") {
                Some("neoforge".into())
            } else if id.contains("fabric-loader-") {
                Some("fabric".into())
            } else if id.contains("quilt-loader-") {
                Some("quilt".into())
            } else {
                None
            }
        })
}

pub fn validate_instance_compatibility(
    kind: &str,
    version: &ResourceVersion,
    instance: &Instance,
    base_version: &str,
) -> Result<()> {
    if !version.game_versions.is_empty() && !version.game_versions.iter().any(|v| v == base_version)
    {
        return Err(AppError::invalid_input(format!(
            "该资源不支持 Minecraft {base_version}（支持：{}）",
            version
                .game_versions
                .iter()
                .rev()
                .take(6)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if kind == "mod" || kind == "modpack" {
        let loader = instance_loader(instance)
            .ok_or_else(|| AppError::invalid_input("目标实例是原版客户端，没有可用的模组加载器"))?;
        if !version.loaders.is_empty()
            && !version
                .loaders
                .iter()
                .any(|l| l.eq_ignore_ascii_case(&loader))
        {
            return Err(AppError::invalid_input(format!(
                "目标实例加载器为 {loader}，该资源支持：{}",
                version.loaders.join(", ")
            )));
        }
    }
    Ok(())
}

fn validate_file_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 255
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name == "."
        || name == ".."
        || name.chars().any(|c| c.is_control())
    {
        return Err(AppError::path_unsafe("资源文件名非法"));
    }
    Ok(())
}

fn safe_relative(path: &str) -> Result<PathBuf> {
    if path.is_empty()
        || path.len() > 512
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.contains(':')
        || path.contains('\0')
    {
        return Err(AppError::new(ErrorCode::ZipUnsafe, "整合包文件路径非法"));
    }
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let mut out = PathBuf::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                "整合包包含不安全的路径",
            ));
        }
        let cleaned = part.trim_end_matches(['.', ' ']);
        let stem = cleaned.split('.').next().unwrap_or("").to_ascii_lowercase();
        if cleaned != part || cleaned.is_empty() || RESERVED.contains(&stem.as_str()) {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("整合包包含 Windows 保留名或非法文件名：{part}"),
            ));
        }
        if part.eq_ignore_ascii_case("instance.json") {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                "整合包不得覆盖 instance.json",
            ));
        }
        out.push(part);
    }
    Ok(out)
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

async fn finish_error(task: &TaskHandle, error: &AppError) {
    if error.code == ErrorCode::Cancelled {
        task.cancelled().await;
    } else {
        task.fail(error.clone()).await;
    }
}

async fn download_one(
    http: &reqwest::Client,
    file: &ResourceFile,
    dest: &Path,
    task: &TaskHandle,
    cancel: &CancellationToken,
    ticker: &ProgressTicker,
    base: u64,
) -> Result<u64> {
    validate_file_name(&file.filename)?;
    let sha1 = file
        .sha1
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::invalid_input("资源文件缺少 SHA-1，已拒绝下载"))?;
    task.set_current_file(&file.filename).await;
    let spec = DownloadSpec {
        url: &file.url,
        dest,
        sha1: Some(sha1),
        size: Some(file.size),
    };
    let outcome = download_file(http, &spec, cancel, false, |done| {
        ticker.set(base.saturating_add(done));
    })
    .await?;
    let written = match outcome {
        DownloadOutcome::Skipped => file.size,
        DownloadOutcome::Downloaded(bytes) => bytes,
    };
    ticker.set(base.saturating_add(written));
    Ok(written)
}

async fn run_single(
    http: &reqwest::Client,
    file: &ResourceFile,
    dir: &Path,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<InstalledResource> {
    task.set_status(TaskStatus::Running).await;
    task.set_total_files(1).await;
    task.set_total_bytes(file.size).await;
    let ticker = ProgressTicker::start(task);
    let result = async {
        util::ensure_dir(dir).await?;
        validate_file_name(&file.filename)?;
        let dest = dir.join(&file.filename);
        download_one(http, file, &dest, task, cancel, &ticker, 0).await?;
        task.file_finished().await;
        Ok::<_, AppError>(InstalledResource {
            path: dest,
            file_name: file.filename.clone(),
        })
    }
    .await;
    ticker.stop();
    match result {
        Ok(done) => {
            task.complete().await;
            Ok(done)
        }
        Err(e) => {
            finish_error(task, &e).await;
            Err(e)
        }
    }
}

pub async fn install_file_to_directory(
    http: &reqwest::Client,
    file: &ResourceFile,
    dir: &Path,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<InstalledResource> {
    run_single(http, file, dir, task, cancel).await
}

/// Download a single artifact that may not publish a SHA-1 (loader profiles,
/// installer jars). Hosts are still restricted by the shared downloader.
#[allow(clippy::too_many_arguments)]
pub async fn download_artifact_to_directory(
    http: &reqwest::Client,
    url: &str,
    filename: &str,
    size: Option<u64>,
    sha1: Option<&str>,
    dir: &Path,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<InstalledResource> {
    validate_file_name(filename)?;
    task.set_status(TaskStatus::Running).await;
    task.set_total_files(1).await;
    task.set_total_bytes(size.unwrap_or(0)).await;
    task.set_current_file(filename).await;
    util::ensure_dir(dir).await?;
    let dest = dir.join(filename);
    let ticker = ProgressTicker::start(task);
    let spec = DownloadSpec {
        url,
        dest: &dest,
        sha1,
        size,
    };
    let outcome = download_file(http, &spec, cancel, false, |done| ticker.set(done)).await;
    ticker.stop();
    match outcome {
        Ok(DownloadOutcome::Skipped) | Ok(DownloadOutcome::Downloaded(_)) => {
            task.file_finished().await;
            task.complete().await;
            Ok(InstalledResource {
                path: dest,
                file_name: filename.to_string(),
            })
        }
        Err(error) => {
            finish_error(task, &error).await;
            Err(error)
        }
    }
}

pub async fn install_file_to_instance(
    http: &reqwest::Client,
    kind: &str,
    file: &ResourceFile,
    instance: &Instance,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<InstalledResource> {
    let subdir = subdir_for_kind(kind)?;
    let dir = instance.game_dir.join(subdir);
    let installed = run_single(http, file, &dir, task, cancel).await?;
    if kind == "resourcepack" {
        if let Err(e) = resourcepacks::set_enabled(instance, &file.filename, true).await {
            tracing::warn!(
                instance = %instance.id,
                file = %file.filename,
                error = %e,
                "resource pack installed but could not be enabled"
            );
        }
    }
    Ok(installed)
}

// ---------------------------------------------------------------------------
// World archives (maps)
// ---------------------------------------------------------------------------

fn extract_world_zip_blocking(
    zip_path: &Path,
    instance: &Instance,
    fallback_name: &str,
) -> Result<PathBuf> {
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| {
        AppError::new(ErrorCode::ZipUnsafe, "地图不是有效的 ZIP").with_detail(e.to_string())
    })?;
    let mut roots: HashSet<String> = HashSet::new();
    for i in 0..zip.len() {
        let Ok(entry) = zip.by_index(i) else { continue };
        let name = entry.name()?.to_string();
        if name == "level.dat" || name.ends_with("/level.dat") {
            let root = name.strip_suffix("level.dat").unwrap_or_default();
            roots.insert(root.trim_end_matches('/').to_string());
        }
    }
    if roots.is_empty() {
        return Err(AppError::invalid_input(
            "ZIP 中没有找到 level.dat，不是 Minecraft 世界存档",
        ));
    }
    if roots.len() > 1 {
        let mut list: Vec<_> = roots.iter().cloned().collect();
        list.sort();
        return Err(AppError::invalid_input(format!(
            "ZIP 包含多个世界存档（{}），暂不支持一次导入多个地图",
            list.join(" / ")
        )));
    }
    let root = roots.into_iter().next().unwrap_or_default();
    let folder_from_root = root
        .rsplit('/')
        .next()
        .map(|s| util::safe_component(s, ""))
        .unwrap_or_default();
    let folder = if folder_from_root.is_empty() {
        util::safe_component(fallback_name, "Map")
    } else {
        folder_from_root
    };
    let saves = instance.game_dir.join("saves");
    std::fs::create_dir_all(&saves)?;
    let mut target = saves.join(&folder);
    let mut suffix = 2;
    while target.exists() {
        target = saves.join(format!("{folder}-{suffix}"));
        suffix += 1;
        if suffix > 100 {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                "同名存档过多，无法生成新名称",
            ));
        }
    }
    std::fs::create_dir_all(&target)?;
    let prefix = if root.is_empty() {
        String::new()
    } else {
        format!("{root}/")
    };
    let mut files = 0u64;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = match zip.by_index(i) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "地图包含符号链接"));
        }
        let name = entry.name()?.to_string();
        let Some(relative) = name.strip_prefix(&prefix) else {
            continue;
        };
        if relative.is_empty() {
            continue;
        }
        let relative = safe_relative(relative)?;
        if entry.compressed_size() > 0 && entry.size() / entry.compressed_size().max(1) > 1000 {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "地图压缩比异常"));
        }
        total = total.saturating_add(entry.size());
        if total > MAX_OVERRIDE_BYTES {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "地图解压后超过 8GB"));
        }
        files += 1;
        if files > MAX_OVERRIDE_FILES {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "地图文件数量过多"));
        }
        let dest = target.join(&relative);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = std::fs::File::create(&dest)?;
        std::io::copy(&mut entry, &mut output)?;
    }
    if !target.join("level.dat").exists() {
        let _ = std::fs::remove_dir_all(&target);
        return Err(AppError::invalid_input("地图解压后缺少 level.dat"));
    }
    Ok(target)
}

pub async fn install_world_archive(
    http: &reqwest::Client,
    file: &ResourceFile,
    instance: &Instance,
    cache_dir: &Path,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<PathBuf> {
    validate_file_name(&file.filename)?;
    if !file.filename.to_ascii_lowercase().ends_with(".zip") {
        return Err(AppError::invalid_input("地图文件必须是 .zip 世界存档"));
    }
    let sha1 = file
        .sha1
        .as_deref()
        .ok_or_else(|| AppError::invalid_input("地图文件缺少 SHA-1，已拒绝下载"))?;
    util::ensure_dir(cache_dir).await?;
    let zip_path = cache_dir.join(format!("map-{}", util::short_id()));
    task.set_status(TaskStatus::Running).await;
    task.set_total_files(2).await;
    task.set_total_bytes(file.size).await;
    task.set_current_file(&file.filename).await;
    let ticker = ProgressTicker::start(task);
    let spec = DownloadSpec {
        url: &file.url,
        dest: &zip_path,
        sha1: Some(sha1),
        size: Some(file.size),
    };
    let download = download_file(http, &spec, cancel, false, |done| {
        ticker.set(done);
    })
    .await;
    ticker.stop();
    if let Err(e) = download {
        let _ = tokio::fs::remove_file(&zip_path).await;
        finish_error(task, &e).await;
        return Err(e);
    }
    task.file_finished().await;
    task.set_current_file("解压地图存档…").await;
    let zip_for_task = zip_path.clone();
    let instance_for_task = instance.clone();
    let fallback = file
        .filename
        .trim_end_matches(".zip")
        .trim_end_matches(".ZIP")
        .to_string();
    let extracted = tokio::task::spawn_blocking(move || {
        extract_world_zip_blocking(&zip_for_task, &instance_for_task, &fallback)
    })
    .await;
    let _ = tokio::fs::remove_file(&zip_path).await;
    match extracted {
        Ok(Ok(path)) => {
            task.file_finished().await;
            task.complete().await;
            Ok(path)
        }
        Ok(Err(e)) => {
            finish_error(task, &e).await;
            Err(e)
        }
        Err(e) => {
            let error = AppError::internal(format!("解压地图失败：{e}"));
            finish_error(task, &error).await;
            Err(error)
        }
    }
}

// ---------------------------------------------------------------------------
// Modpacks
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ModpackIndex {
    #[serde(default)]
    format_version: u32,
    #[serde(default)]
    game: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    files: Vec<ModpackFile>,
    #[serde(default)]
    dependencies: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ModpackFile {
    #[serde(default)]
    path: String,
    #[serde(default)]
    hashes: ModpackHashes,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(default)]
    file_size: u64,
    #[serde(default)]
    env: Option<ModpackEnv>,
}

#[derive(Debug, Deserialize, Default)]
struct ModpackHashes {
    #[serde(default)]
    sha1: Option<String>,
    #[serde(default)]
    sha512: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ModpackEnv {
    #[serde(default)]
    client: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CurseManifest {
    #[serde(default)]
    minecraft: CurseMinecraft,
    #[serde(default)]
    manifest_type: String,
    #[serde(default)]
    manifest_version: u32,
    #[serde(default)]
    files: Vec<CurseFileRef>,
    #[serde(default)]
    overrides: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CurseMinecraft {
    #[serde(default)]
    version: String,
    #[serde(default)]
    mod_loaders: Vec<CurseLoader>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CurseLoader {
    #[serde(default)]
    id: String,
    #[serde(default)]
    primary: bool,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CurseFileRef {
    #[serde(default)]
    project_id: u64,
    #[serde(default)]
    file_id: u64,
    #[serde(default)]
    required: bool,
}

fn read_curse_manifest_optional(zip_path: &Path) -> Result<Option<CurseManifest>> {
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| {
        AppError::new(ErrorCode::ZipUnsafe, "整合包不是有效的 ZIP").with_detail(e.to_string())
    })?;
    let entry = match zip.by_name("manifest.json") {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };
    let mut bytes = Vec::new();
    entry
        .take(MAX_INDEX_BYTES as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| AppError::invalid_input(format!("无法读取 CurseForge 清单：{e}")))?;
    let manifest: CurseManifest = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::invalid_input(format!("CurseForge 清单格式无效：{e}")))?;
    if !manifest.manifest_type.is_empty() && manifest.manifest_type != "minecraftModpack" {
        return Err(AppError::unsupported(
            "ZIP 不是 CurseForge Minecraft 整合包",
        ));
    }
    if manifest.manifest_version != 0 && manifest.manifest_version != 1 {
        return Err(AppError::unsupported("不支持的 CurseForge 整合包版本"));
    }
    if manifest.minecraft.version.trim().is_empty() {
        return Err(AppError::invalid_input(
            "CurseForge 整合包缺少 Minecraft 版本",
        ));
    }
    Ok(Some(manifest))
}

fn curse_loader_kind(id: &str) -> Option<&'static str> {
    let lower = id.to_ascii_lowercase();
    if lower.starts_with("fabric-") {
        Some("fabric")
    } else if lower.starts_with("quilt-") {
        Some("quilt")
    } else if lower.starts_with("neoforge-") {
        Some("neoforge")
    } else if lower.starts_with("forge-") {
        Some("forge")
    } else {
        None
    }
}

fn validate_curse_manifest(
    manifest: &CurseManifest,
    instance: &Instance,
    base_version: &str,
) -> Result<()> {
    if manifest.minecraft.version != base_version {
        return Err(AppError::invalid_input(format!(
            "整合包要求 Minecraft {}，目标实例是 {base_version}",
            manifest.minecraft.version
        )));
    }
    let required = manifest
        .minecraft
        .mod_loaders
        .iter()
        .find(|l| l.primary)
        .or_else(|| manifest.minecraft.mod_loaders.first())
        .and_then(|l| curse_loader_kind(&l.id));
    match (required, instance_loader(instance)) {
        (Some(want), Some(have)) if want == have => Ok(()),
        (Some(want), Some(have)) => Err(AppError::invalid_input(format!(
            "整合包需要 {want}，目标实例是 {have}"
        ))),
        (Some(want), None) => Err(AppError::invalid_input(format!(
            "整合包需要 {want}，目标实例是原版客户端"
        ))),
        (None, _) => Ok(()),
    }
}

fn read_index_optional(zip_path: &Path) -> Result<Option<ModpackIndex>> {
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| {
        AppError::new(ErrorCode::ZipUnsafe, "整合包不是有效的 ZIP").with_detail(e.to_string())
    })?;
    let entry = match zip.by_name("modrinth.index.json") {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };
    let mut bytes = Vec::new();
    entry
        .take(MAX_INDEX_BYTES as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| AppError::invalid_input(format!("无法读取整合包清单：{e}")))?;
    let index: ModpackIndex = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::invalid_input(format!("整合包清单格式无效：{e}")))?;
    if index.format_version != 0 && index.format_version != 1 {
        return Err(AppError::unsupported("不支持的整合包格式版本"));
    }
    if !index.game.is_empty() && index.game != "minecraft" {
        return Err(AppError::unsupported("整合包不是 Minecraft 整合包"));
    }
    if let Some(mc) = index.dependencies.get("minecraft") {
        if mc.trim().is_empty() {
            return Err(AppError::invalid_input("整合包缺少 Minecraft 版本"));
        }
    }
    for file in &index.files {
        safe_relative(&file.path)?;
        if file.downloads.is_empty() {
            return Err(AppError::invalid_input(format!(
                "整合包文件缺少下载地址：{}",
                file.path
            )));
        }
        if file.hashes.sha1.as_deref().unwrap_or("").is_empty() {
            return Err(AppError::invalid_input(format!(
                "整合包文件缺少 SHA-1：{}",
                file.path
            )));
        }
    }
    Ok(Some(index))
}

fn read_index(zip_path: &Path) -> Result<ModpackIndex> {
    read_index_optional(zip_path)?
        .ok_or_else(|| AppError::invalid_input("整合包缺少 modrinth.index.json"))
}

fn loader_matches_dependency(instance: &Instance, deps: &HashMap<String, String>) -> Result<()> {
    let required = if deps.contains_key("fabric-loader") {
        Some("fabric")
    } else if deps.contains_key("quilt-loader") {
        Some("quilt")
    } else if deps.contains_key("forge") {
        Some("forge")
    } else if deps.contains_key("neoforge") {
        Some("neoforge")
    } else {
        None
    };
    match (required, instance_loader(instance)) {
        (Some(want), Some(have)) if want == have => Ok(()),
        (Some(want), Some(have)) => Err(AppError::invalid_input(format!(
            "整合包需要 {want}，目标实例是 {have}"
        ))),
        (Some(want), None) => Err(AppError::invalid_input(format!(
            "整合包需要 {want}，目标实例是原版客户端"
        ))),
        (None, _) => Ok(()),
    }
}

fn backup_file_if_exists(instance: &Instance, relative: &Path) -> Result<()> {
    if relative == Path::new("options.txt") {
        let source = instance.game_dir.join(relative);
        if source.exists() {
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let backup = instance
                .game_dir
                .join(format!("options.txt.sr-backup-{stamp}"));
            if !backup.exists() {
                std::fs::copy(&source, &backup)?;
            }
        }
    }
    Ok(())
}

fn extract_overrides_blocking(
    zip_path: &Path,
    instance: &Instance,
    prefixes: &[String],
) -> Result<u32> {
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| {
        AppError::new(ErrorCode::ZipUnsafe, "整合包不是有效的 ZIP").with_detail(e.to_string())
    })?;

    // Phase 1: validate the complete manifest before touching the instance.
    let mut entries: Vec<(String, PathBuf, u64)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let entry = match zip.by_index(i) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name()?.to_string();
        let Some(relative) = prefixes.iter().find_map(|prefix| name.strip_prefix(prefix)) else {
            continue;
        };
        if relative.is_empty() {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "整合包包含符号链接"));
        }
        let relative = safe_relative(relative)?;
        if relative == Path::new("instance.json") {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                "整合包不得覆盖 instance.json",
            ));
        }
        if !seen.insert(relative.clone()) {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("整合包包含重复文件：{}", relative.display()),
            ));
        }
        total = total.saturating_add(entry.size());
        if total > MAX_OVERRIDE_BYTES {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                "整合包 overrides 超过 8GB",
            ));
        }
        if entries.len() as u64 >= MAX_OVERRIDE_FILES {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "整合包文件数量过多"));
        }
        entries.push((name, relative, entry.size()));
    }

    // Phase 2: extract everything into a staging directory.
    let stamp = crate::util::short_id();
    let staging_root = instance
        .game_dir
        .join(format!(".srl-import-staging-{stamp}"));
    let backup_root = instance
        .game_dir
        .join(format!(".srl-import-backup-{stamp}"));
    std::fs::create_dir_all(&staging_root)?;
    std::fs::create_dir_all(&backup_root)?;
    for (name, relative, _) in &entries {
        let mut entry = zip.by_name(name).map_err(|e| {
            AppError::new(ErrorCode::ZipUnsafe, format!("无法读取整合包条目 {name}"))
                .with_detail(e.to_string())
        })?;
        let staged = staging_root.join(relative);
        if let Some(parent) = staged.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut output = std::fs::File::create(&staged)?;
        std::io::copy(&mut entry, &mut output)?;
        output.sync_all()?;
    }
    drop(zip);

    // Phase 3: back up every existing target before committing.
    let mut backed_up: Vec<PathBuf> = Vec::new();
    for (_, relative, _) in &entries {
        let dest = instance.game_dir.join(relative);
        crate::util::ensure_path_within_root(&instance.game_dir, &dest)?;
        if dest.exists() {
            if dest.is_dir() {
                return Err(AppError::new(
                    ErrorCode::ZipUnsafe,
                    format!("目标路径是目录，拒绝用文件覆盖：{}", relative.display()),
                ));
            }
            let backup = backup_root.join(relative);
            if let Some(parent) = backup.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&dest, &backup)?;
            backed_up.push(relative.clone());
        }
    }

    // Phase 4: commit. Any failure triggers a best-effort rollback.
    let mut committed: Vec<PathBuf> = Vec::new();
    let commit = (|| -> Result<()> {
        for (_, relative, _) in &entries {
            let dest = instance.game_dir.join(relative);
            crate::util::ensure_path_within_root(&instance.game_dir, &dest)?;
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if dest.exists() {
                std::fs::remove_file(&dest)?;
            }
            committed.push(relative.clone());
            std::fs::rename(staging_root.join(relative), &dest)?;
        }
        Ok(())
    })();

    if let Err(error) = commit {
        let mut rollback_failures = Vec::new();
        for relative in committed.iter().rev() {
            let dest = instance.game_dir.join(relative);
            let backup = backup_root.join(relative);
            let result = if backup.exists() {
                std::fs::copy(&backup, &dest).map(|_| ())
            } else if dest.exists() {
                std::fs::remove_file(&dest)
            } else {
                Ok(())
            };
            if let Err(failure) = result {
                rollback_failures.push(format!("{}: {failure}", relative.display()));
            }
        }
        let _ = std::fs::remove_dir_all(&staging_root);
        if rollback_failures.is_empty() {
            let _ = std::fs::remove_dir_all(&backup_root);
            return Err(error);
        }
        return Err(AppError::new(
            ErrorCode::Io,
            "整合包导入失败，且自动回滚未完全成功；恢复材料已保留",
        )
        .with_detail(format!(
            "备份目录：{}；回滚失败：{}",
            backup_root.display(),
            rollback_failures.join("; ")
        )));
    }

    let _ = std::fs::remove_dir_all(&staging_root);
    let _ = std::fs::remove_dir_all(&backup_root);
    let _ = backed_up;
    Ok(entries.len() as u32)
}

fn default_override_prefixes() -> Vec<String> {
    vec!["overrides/".into(), "client-overrides/".into()]
}

async fn extract_overrides(zip_path: &Path, instance: &Instance) -> Result<u32> {
    let zip_path = zip_path.to_path_buf();
    let instance = instance.clone();
    tokio::task::spawn_blocking(move || {
        extract_overrides_blocking(&zip_path, &instance, &default_override_prefixes())
    })
    .await
    .map_err(|e| AppError::internal(format!("解压整合包 overrides 失败：{e}")))?
}

fn dependency_minecraft(index: &ModpackIndex) -> Option<&str> {
    index.dependencies.get("minecraft").map(|s| s.as_str())
}

#[allow(clippy::too_many_arguments)]
async fn install_modrinth_pack(
    http: &reqwest::Client,
    zip_path: &Path,
    instance: &Instance,
    base_version: &str,
    task: &TaskHandle,
    cancel: &CancellationToken,
    mrpack_size: u64,
    index: ModpackIndex,
) -> Result<()> {
    if let Some(mc) = dependency_minecraft(&index) {
        if mc != base_version {
            return Err(AppError::invalid_input(format!(
                "整合包要求 Minecraft {mc}，目标实例是 {base_version}"
            )));
        }
    }
    loader_matches_dependency(instance, &index.dependencies)?;

    let files: Vec<&ModpackFile> = index
        .files
        .iter()
        .filter(|f| {
            f.env
                .as_ref()
                .and_then(|e| e.client.as_deref())
                .map(|v| v != "unsupported")
                .unwrap_or(true)
        })
        .collect();
    let downloads_total: u64 = files.iter().map(|f| f.file_size).sum();
    task.set_total_files(files.len() as u32 + 1).await;
    task.set_total_bytes(mrpack_size.saturating_add(downloads_total))
        .await;
    task.set_status(TaskStatus::Running).await;
    let ticker = ProgressTicker::start(task);
    let result = async {
        let mut base = mrpack_size;
        for file in &files {
            let relative = safe_relative(&file.path)?;
            let dest = instance.game_dir.join(&relative);
            backup_file_if_exists(instance, &relative)?;
            let spec_file = ResourceFile {
                filename: relative
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "file".into()),
                url: file.downloads[0].clone(),
                size: file.file_size,
                primary: true,
                sha1: file.hashes.sha1.clone(),
                sha512: file.hashes.sha512.clone(),
                file_type: None,
            };
            validate_file_name(&spec_file.filename)?;
            task.set_current_file(&file.path).await;
            let written =
                download_one(http, &spec_file, &dest, task, cancel, &ticker, base).await?;
            base = base.saturating_add(written);
            task.file_finished().await;
        }
        task.set_current_file("解压 overrides…").await;
        extract_overrides(zip_path, instance).await?;
        task.file_finished().await;
        task.set_done_bytes(base).await;
        task.complete().await;
        Ok::<_, AppError>(())
    }
    .await;
    ticker.stop();
    if let Err(e) = &result {
        finish_error(task, e).await;
    }
    result
}

fn known_instance_root(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "mods"
            | "config"
            | "resourcepacks"
            | "shaderpacks"
            | "saves"
            | "defaultconfigs"
            | "kubejs"
            | "scripts"
            | "datapacks"
            | "structures"
            | "openloader"
            | "patchouli_books"
            | "ftbquests"
            | "journeymap"
            | "xaero"
            | "iris"
            | "oculus"
            | "overrides"
    )
}

fn extract_generic_blocking(zip_path: &Path, instance: &Instance) -> Result<(u32, u64)> {
    let file = std::fs::File::open(zip_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| {
        AppError::new(ErrorCode::ZipUnsafe, "整合包不是有效的 ZIP").with_detail(e.to_string())
    })?;
    let mut names = Vec::new();
    for i in 0..zip.len() {
        if let Ok(entry) = zip.by_index(i) {
            names.push(entry.name()?.to_string());
        }
    }
    let roots: HashSet<&str> = names
        .iter()
        .filter_map(|name| name.split('/').next())
        .filter(|root| !root.is_empty())
        .collect();
    let strip_root = roots.len() == 1
        && names.iter().all(|name| name.contains('/'))
        && roots
            .iter()
            .next()
            .is_some_and(|root| !known_instance_root(root));

    let mut files = 0u64;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = match zip.by_index(i) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "整合包包含符号链接"));
        }
        let name = entry.name()?.to_string();
        let relative_name = if strip_root {
            name.split_once('/').map(|(_, rest)| rest).unwrap_or(&name)
        } else {
            name.as_str()
        };
        if relative_name.is_empty() {
            continue;
        }
        let relative = safe_relative(relative_name)?;
        backup_file_if_exists(instance, &relative)?;
        total = total.saturating_add(entry.size());
        if total > MAX_OVERRIDE_BYTES {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "整合包解压后超过 8GB"));
        }
        files += 1;
        if files > MAX_OVERRIDE_FILES {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "整合包文件数量过多"));
        }
        let dest = instance.game_dir.join(&relative);
        crate::util::ensure_path_within_root(&instance.game_dir, &dest)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::util::ensure_path_within_root(&instance.game_dir, &dest)?;
        let mut output = std::fs::File::create(&dest)?;
        std::io::copy(&mut entry, &mut output)?;
    }
    Ok((files as u32, total))
}

async fn install_generic_zip(
    zip_path: &Path,
    instance: &Instance,
    task: &TaskHandle,
) -> Result<()> {
    task.set_status(TaskStatus::Running).await;
    task.set_current_file("解压 ZIP 整合包…").await;
    let zip_path = zip_path.to_path_buf();
    let instance = instance.clone();
    let result =
        tokio::task::spawn_blocking(move || extract_generic_blocking(&zip_path, &instance))
            .await
            .map_err(|e| AppError::internal(format!("解压整合包失败：{e}")))?;
    match result {
        Ok((files, bytes)) => {
            task.set_total_files(files.max(1)).await;
            task.set_total_bytes(bytes).await;
            task.set_done_bytes(bytes).await;
            task.file_finished().await;
            task.complete().await;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[allow(clippy::too_many_arguments)]
async fn install_curseforge_pack(
    http: &reqwest::Client,
    zip_path: &Path,
    instance: &Instance,
    base_version: &str,
    manifest: CurseManifest,
    curseforge_key: Option<String>,
    allow_incomplete: bool,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<()> {
    validate_curse_manifest(&manifest, instance, base_version)?;
    let required: Vec<&CurseFileRef> = manifest.files.iter().filter(|f| f.required).collect();
    let overrides = if manifest.overrides.trim().is_empty() {
        "overrides".to_string()
    } else {
        manifest.overrides.trim().trim_end_matches('/').to_string()
    };
    let prefixes = vec![format!("{overrides}/"), "client-overrides/".to_string()];

    let Some(key) = curseforge_key else {
        if !required.is_empty() && !allow_incomplete {
            return Err(AppError::new(
                ErrorCode::NotConfigured,
                format!(
                    "该 CurseForge 整合包需要下载 {} 个文件，请先在设置中配置 CurseForge API key",
                    required.len()
                ),
            ));
        }
        task.set_status(TaskStatus::Running).await;
        task.set_current_file("解压 ZIP 内容…").await;
        let zip_path = zip_path.to_path_buf();
        let instance = instance.clone();
        let count = tokio::task::spawn_blocking(move || {
            extract_overrides_blocking(&zip_path, &instance, &prefixes)
        })
        .await
        .map_err(|e| AppError::internal(format!("解压整合包失败：{e}")))??;
        task.set_total_files(count.max(1)).await;
        task.set_total_bytes(0).await;
        task.file_finished().await;
        task.complete().await;
        return Ok(());
    };

    let service = CurseForgeService::new(http.clone(), key);
    task.set_status(TaskStatus::Running).await;
    task.set_current_file("读取 CurseForge 文件信息…").await;
    let mut resolved: Vec<CurseForgeFile> = Vec::new();
    for file in &required {
        resolved.push(service.file(file.project_id, file.file_id).await?);
    }
    let total_bytes: u64 = resolved.iter().map(|f| f.size).sum();
    task.set_total_files(resolved.len() as u32 + 1).await;
    task.set_total_bytes(total_bytes).await;
    let ticker = ProgressTicker::start(task);
    let result = async {
        let mut base = 0u64;
        for file in &resolved {
            validate_file_name(&file.file_name)?;
            let dest = instance.game_dir.join("mods").join(&file.file_name);
            let spec = ResourceFile {
                filename: file.file_name.clone(),
                url: file.download_url.clone(),
                size: file.size,
                primary: true,
                sha1: file.sha1.clone(),
                sha512: None,
                file_type: None,
            };
            task.set_current_file(&file.file_name).await;
            let written = download_one(http, &spec, &dest, task, cancel, &ticker, base).await?;
            base = base.saturating_add(written);
            task.file_finished().await;
        }
        task.set_current_file("解压 overrides…").await;
        let zip_path = zip_path.to_path_buf();
        let instance = instance.clone();
        let prefixes = prefixes.clone();
        tokio::task::spawn_blocking(move || {
            extract_overrides_blocking(&zip_path, &instance, &prefixes)
        })
        .await
        .map_err(|e| AppError::internal(format!("解压整合包失败：{e}")))??;
        task.file_finished().await;
        task.set_done_bytes(base).await;
        task.complete().await;
        Ok::<_, AppError>(())
    }
    .await;
    ticker.stop();
    result
}

#[allow(clippy::too_many_arguments)]
async fn install_modpack_zip(
    http: &reqwest::Client,
    zip_path: &Path,
    instance: &Instance,
    base_version: &str,
    task: &TaskHandle,
    cancel: &CancellationToken,
    mrpack_size: u64,
    allow_incomplete: bool,
    curseforge_key: Option<String>,
) -> Result<()> {
    if let Some(index) = read_index_optional(zip_path)? {
        return install_modrinth_pack(
            http,
            zip_path,
            instance,
            base_version,
            task,
            cancel,
            mrpack_size,
            index,
        )
        .await;
    }
    if let Some(manifest) = read_curse_manifest_optional(zip_path)? {
        return install_curseforge_pack(
            http,
            zip_path,
            instance,
            base_version,
            manifest,
            curseforge_key,
            allow_incomplete,
            task,
            cancel,
        )
        .await;
    }
    install_generic_zip(zip_path, instance, task).await
}

async fn download_mrpack(
    http: &reqwest::Client,
    file: &ResourceFile,
    cache_dir: &Path,
    version_id: &str,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<(PathBuf, u64)> {
    util::ensure_dir(cache_dir).await?;
    let name = format!("{version_id}.mrpack");
    validate_file_name(&name)?;
    let dest = cache_dir.join(name);
    let sha1 = file
        .sha1
        .as_deref()
        .ok_or_else(|| AppError::invalid_input("整合包缺少 SHA-1，已拒绝下载"))?;
    task.set_total_files(1).await;
    task.set_total_bytes(file.size).await;
    task.set_current_file(&file.filename).await;
    task.set_status(TaskStatus::Running).await;
    let ticker = ProgressTicker::start(task);
    let spec = DownloadSpec {
        url: &file.url,
        dest: &dest,
        sha1: Some(sha1),
        size: Some(file.size),
    };
    let outcome = download_file(http, &spec, cancel, false, |done| {
        ticker.set(done);
    })
    .await;
    ticker.stop();
    match outcome {
        Ok(outcome) => {
            let written = match outcome {
                DownloadOutcome::Skipped => file.size,
                DownloadOutcome::Downloaded(bytes) => bytes,
            };
            Ok((dest, written))
        }
        Err(e) => {
            finish_error(task, &e).await;
            Err(e)
        }
    }
}

pub struct ModpackInstall<'a> {
    pub http: &'a reqwest::Client,
    pub version: &'a ResourceVersion,
    pub file: &'a ResourceFile,
    pub instance: &'a Instance,
    pub base_version: &'a str,
    pub cache_dir: &'a Path,
    pub task: &'a TaskHandle,
    pub cancel: &'a CancellationToken,
    pub allow_incomplete: bool,
    pub curseforge_key: Option<String>,
}

pub async fn install_modpack(ctx: ModpackInstall<'_>) -> Result<()> {
    let ModpackInstall {
        http,
        version,
        file,
        instance,
        base_version,
        cache_dir,
        task,
        cancel,
        allow_incomplete,
        curseforge_key,
    } = ctx;
    if file.size > 2 * 1024 * 1024 * 1024 {
        return Err(AppError::invalid_input("整合包文件超过 2GB"));
    }
    let (zip_path, mrpack_size) =
        download_mrpack(http, file, cache_dir, &version.id, task, cancel).await?;
    let result = install_modpack_zip(
        http,
        &zip_path,
        instance,
        base_version,
        task,
        cancel,
        mrpack_size,
        allow_incomplete,
        curseforge_key,
    )
    .await;
    if let Err(error) = &result {
        finish_error(task, error).await;
    }
    result
}

#[allow(clippy::too_many_arguments)]
pub async fn import_modpack(
    http: &reqwest::Client,
    path: &Path,
    instance: &Instance,
    base_version: &str,
    task: &TaskHandle,
    cancel: &CancellationToken,
    allow_incomplete: bool,
    curseforge_key: Option<String>,
) -> Result<()> {
    let meta = tokio::fs::metadata(path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("整合包文件不存在")
        } else {
            e.into()
        }
    })?;
    if !meta.is_file() {
        return Err(AppError::invalid_input("整合包必须是普通文件"));
    }
    if meta.len() > 2 * 1024 * 1024 * 1024 {
        return Err(AppError::invalid_input("整合包文件超过 2GB"));
    }
    task.set_status(TaskStatus::Running).await;
    let result = install_modpack_zip(
        http,
        path,
        instance,
        base_version,
        task,
        cancel,
        0,
        allow_incomplete,
        curseforge_key,
    )
    .await;
    if let Err(error) = &result {
        finish_error(task, error).await;
    }
    result
}

pub fn modpack_index_summary(path: &Path) -> Result<(String, String, String, usize)> {
    let index = read_index(path)?;
    let mc = dependency_minecraft(&index).unwrap_or("未知").to_string();
    let loader = if index.dependencies.contains_key("fabric-loader") {
        "Fabric"
    } else if index.dependencies.contains_key("quilt-loader") {
        "Quilt"
    } else if index.dependencies.contains_key("forge") {
        "Forge"
    } else if index.dependencies.contains_key("neoforge") {
        "NeoForge"
    } else {
        "原版"
    };
    Ok((index.name, mc, loader.into(), index.files.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_instance(game_dir: &Path) -> Instance {
        Instance {
            id: "inst".into(),
            name: "test".into(),
            version_id: "1.21.1-fabric-loader-0.16.0-1.21.1".into(),
            game_dir: game_dir.to_path_buf(),
            java_path: None,
            memory_mb: 2048,
            jvm_args: vec![],
            game_args: vec![],
            created_at: "2026-01-01T00:00:00Z".into(),
            last_launched_at: None,
            account_id: None,
            resolution_width: None,
            resolution_height: None,
            mod_loader: Some(crate::instance::ModLoaderInfo {
                kind: "Fabric".into(),
                version: "0.16.0".into(),
                installed: true,
            }),
        }
    }

    #[tokio::test]
    async fn installs_modpack_with_downloads_and_overrides() {
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mod_bytes = b"hello".to_vec();
        let mod_sha1 = util::sha1_bytes(&mod_bytes);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let pack_bytes = {
            let index = serde_json::json!({
                "formatVersion": 1,
                "game": "minecraft",
                "name": "Test Pack",
                "versionId": "1.0",
                "files": [{
                    "path": "mods/a.jar",
                    "hashes": {"sha1": mod_sha1},
                    "downloads": [format!("http://{addr}/a.jar")],
                    "fileSize": mod_bytes.len()
                }],
                "dependencies": {"minecraft": "1.21.1", "fabric-loader": "0.16.0"}
            });
            let mut out = Vec::new();
            {
                let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
                let options = zip::write::SimpleFileOptions::default();
                zip.start_file("modrinth.index.json", options).unwrap();
                zip.write_all(serde_json::to_string(&index).unwrap().as_bytes())
                    .unwrap();
                zip.start_file("overrides/config/x.txt", options).unwrap();
                zip.write_all(b"cfg").unwrap();
                zip.finish().unwrap();
            }
            out
        };
        let pack_sha1 = util::sha1_bytes(&pack_bytes);
        let pack_for_server = pack_bytes.clone();
        let mod_for_server = mod_bytes.clone();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]);
                let body = if head.contains("pack.mrpack") {
                    pack_for_server.clone()
                } else {
                    mod_for_server.clone()
                };
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let mut inst = test_instance(dir.path());
        let version = ResourceVersion {
            id: "ver1".into(),
            project_id: "proj".into(),
            name: "Test Pack".into(),
            version_number: "1.0".into(),
            version_type: "release".into(),
            date_published: None,
            downloads: 0,
            game_versions: vec!["1.21.1".into()],
            loaders: vec!["fabric".into()],
            files: vec![ResourceFile {
                filename: "pack.mrpack".into(),
                url: format!("http://{addr}/pack.mrpack"),
                size: pack_bytes.len() as u64,
                primary: true,
                sha1: Some(pack_sha1),
                sha512: None,
                file_type: None,
            }],
            dependencies: vec![],
        };
        let file = version.files[0].clone();
        inst.game_dir = dir.path().to_path_buf();
        let http = reqwest::Client::new();
        let sink: crate::download::TaskSink = Arc::new(|_| {});
        let downloads = crate::download::DownloadManager::new(sink);
        let task = downloads.create("resource", "test", 0, 0).await;
        let cancel = task.cancel_token();
        install_modpack(ModpackInstall {
            http: &http,
            version: &version,
            file: &file,
            instance: &inst,
            base_version: "1.21.1",
            cache_dir: cache.path(),
            task: &task,
            cancel: &cancel,
            allow_incomplete: false,
            curseforge_key: None,
        })
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(
            std::fs::read(inst.game_dir.join("mods/a.jar")).unwrap(),
            b"hello"
        );
        assert_eq!(
            std::fs::read_to_string(inst.game_dir.join("config/x.txt")).unwrap(),
            "cfg"
        );
    }

    #[test]
    fn extracts_generic_zip_and_strips_single_root() {
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        std::fs::create_dir_all(&inst.game_dir).unwrap();
        let zip_path = dir.path().join("pack.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            for (name, body) in [
                ("MyPack/mods/a.jar", b"jar".as_slice()),
                ("MyPack/config/x.txt", b"cfg".as_slice()),
            ] {
                zip.start_file(name, options).unwrap();
                zip.write_all(body).unwrap();
            }
            zip.finish().unwrap();
        }
        let (files, bytes) = extract_generic_blocking(&zip_path, &inst).unwrap();
        assert_eq!(files, 2);
        assert_eq!(bytes, 6);
        assert_eq!(
            std::fs::read(inst.game_dir.join("mods/a.jar")).unwrap(),
            b"jar"
        );
        assert_eq!(
            std::fs::read_to_string(inst.game_dir.join("config/x.txt")).unwrap(),
            "cfg"
        );
    }

    #[test]
    fn reads_and_validates_curseforge_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        let zip_path = dir.path().join("cf.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            let manifest = serde_json::json!({
                "minecraft": {
                    "version": "1.21.1",
                    "modLoaders": [{"id": "fabric-0.16.0", "primary": true}]
                },
                "manifestType": "minecraftModpack",
                "manifestVersion": 1,
                "files": [{"projectID": 1, "fileID": 2, "required": true}],
                "overrides": "overrides"
            });
            zip.start_file("manifest.json", options).unwrap();
            zip.write_all(serde_json::to_string(&manifest).unwrap().as_bytes())
                .unwrap();
            zip.start_file("overrides/config/cf.txt", options).unwrap();
            zip.write_all(b"cf").unwrap();
            zip.finish().unwrap();
        }
        let manifest = read_curse_manifest_optional(&zip_path).unwrap().unwrap();
        assert!(validate_curse_manifest(&manifest, &inst, "1.21.1").is_ok());
        assert!(validate_curse_manifest(&manifest, &inst, "1.20.1").is_err());
        let count = extract_overrides_blocking(
            &zip_path,
            &inst,
            &["overrides/".into(), "client-overrides/".into()],
        )
        .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            std::fs::read_to_string(inst.game_dir.join("config/cf.txt")).unwrap(),
            "cf"
        );
    }

    #[tokio::test]
    async fn curseforge_without_key_can_import_zip_content_when_allowed() {
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        std::fs::create_dir_all(&inst.game_dir).unwrap();
        let zip_path = dir.path().join("cf.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            let manifest = serde_json::json!({
                "minecraft": {
                    "version": "1.21.1",
                    "modLoaders": [{"id": "fabric-0.16.0", "primary": true}]
                },
                "manifestType": "minecraftModpack",
                "manifestVersion": 1,
                "files": [{"projectID": 1, "fileID": 2, "required": true}],
                "overrides": "overrides"
            });
            zip.start_file("manifest.json", options).unwrap();
            zip.write_all(serde_json::to_string(&manifest).unwrap().as_bytes())
                .unwrap();
            zip.start_file("overrides/config/cf.txt", options).unwrap();
            zip.write_all(b"cf").unwrap();
            zip.finish().unwrap();
        }
        let http = reqwest::Client::new();
        let sink: crate::download::TaskSink = Arc::new(|_| {});
        let downloads = crate::download::DownloadManager::new(sink);
        let task = downloads.create("resource", "test", 0, 0).await;
        let cancel = task.cancel_token();
        install_modpack_zip(
            &http, &zip_path, &inst, "1.21.1", &task, &cancel, 0, true, None,
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(inst.game_dir.join("config/cf.txt")).unwrap(),
            "cf"
        );
    }

    #[test]
    fn extracts_world_zip_and_chooses_unique_folder() {
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        std::fs::create_dir_all(inst.game_dir.join("saves/MyMap")).unwrap();
        let zip_path = dir.path().join("map.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            for (name, body) in [
                ("MyMap/level.dat", b"level".as_slice()),
                ("MyMap/region/r.0.0.mca", b"region".as_slice()),
                ("README.txt", b"readme".as_slice()),
            ] {
                zip.start_file(name, options).unwrap();
                zip.write_all(body).unwrap();
            }
            zip.finish().unwrap();
        }
        let target = extract_world_zip_blocking(&zip_path, &inst, "map").unwrap();
        assert_eq!(target.file_name().unwrap(), "MyMap-2");
        assert_eq!(std::fs::read(target.join("level.dat")).unwrap(), b"level");
        assert!(!target.join("README.txt").exists());
        assert_eq!(
            std::fs::read(target.join("region/r.0.0.mca")).unwrap(),
            b"region"
        );
    }

    #[test]
    fn rejects_map_with_multiple_worlds() {
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        let zip_path = dir.path().join("multi.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            for name in ["A/level.dat", "B/level.dat"] {
                zip.start_file(name, options).unwrap();
                zip.write_all(b"x").unwrap();
            }
            zip.finish().unwrap();
        }
        assert!(extract_world_zip_blocking(&zip_path, &inst, "multi").is_err());
    }

    #[test]
    fn deserializes_frontend_targets() {
        let inst: ResourceTarget = serde_json::from_value(serde_json::json!({
            "kind": "instance", "instanceId": "abc"
        }))
        .unwrap();
        match inst {
            ResourceTarget::Instance { instance_id } => assert_eq!(instance_id, "abc"),
            _ => panic!("wrong target"),
        }
        let dir: ResourceTarget = serde_json::from_value(serde_json::json!({
            "kind": "directory", "path": "/tmp"
        }))
        .unwrap();
        match dir {
            ResourceTarget::Directory { path } => assert_eq!(path, "/tmp"),
            _ => panic!("wrong target"),
        }
    }

    #[test]
    fn rejects_unsafe_relative_paths() {
        assert!(safe_relative("mods/foo.jar").is_ok());
        assert!(safe_relative("../evil").is_err());
        assert!(safe_relative("/abs").is_err());
        assert!(safe_relative("mods/../evil").is_err());
        assert!(safe_relative("instance.json").is_err());
        assert!(safe_relative("mods\\evil").is_err());
    }

    #[test]
    fn reads_modpack_index_and_summary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pack.mrpack");
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            let index = serde_json::json!({
                "formatVersion": 1,
                "game": "minecraft",
                "name": "Test Pack",
                "versionId": "1.0",
                "files": [{"path":"mods/a.jar","hashes":{"sha1":"aa"},"downloads":["https://cdn.modrinth.com/x.jar"],"fileSize":10}],
                "dependencies": {"minecraft":"1.21.1","fabric-loader":"0.16.0"}
            });
            zip.start_file("modrinth.index.json", options).unwrap();
            zip.write_all(serde_json::to_string(&index).unwrap().as_bytes())
                .unwrap();
            zip.start_file("overrides/config/test.txt", options)
                .unwrap();
            zip.write_all(b"x").unwrap();
            zip.finish().unwrap();
        }
        let (name, mc, loader, files) = modpack_index_summary(&path).unwrap();
        assert_eq!(name, "Test Pack");
        assert_eq!(mc, "1.21.1");
        assert_eq!(loader, "Fabric");
        assert_eq!(files, 1);

        let inst = test_instance(dir.path());
        assert!(loader_matches_dependency(
            &inst,
            &HashMap::from([
                ("minecraft".into(), "1.21.1".into()),
                ("fabric-loader".into(), "0.16.0".into())
            ])
        )
        .is_ok());
        assert!(loader_matches_dependency(
            &inst,
            &HashMap::from([("forge".into(), "52.0.16".into())])
        )
        .is_err());
    }

    #[test]
    fn rejects_windows_reserved_names() {
        assert!(safe_relative("mods/con.jar").is_err());
        assert!(safe_relative("mods/NUL.txt").is_err());
        assert!(safe_relative("mods/foo.").is_err());
        assert!(safe_relative("mods/foo").is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_to_write_through_existing_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        std::fs::create_dir_all(&inst.game_dir).unwrap();
        std::fs::create_dir_all(outside.path()).unwrap();
        std::os::unix::fs::symlink(outside.path(), inst.game_dir.join("config")).unwrap();
        let zip_path = dir.path().join("pack.mrpack");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("overrides/config/escape.txt", options)
                .unwrap();
            zip.write_all(b"owned").unwrap();
            zip.finish().unwrap();
        }
        let result = extract_overrides_blocking(&zip_path, &inst, &default_override_prefixes());
        assert!(result.is_err());
        assert!(!outside.path().join("escape.txt").exists());
    }

    #[test]
    fn extracts_overrides_transactionally() {
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        std::fs::create_dir_all(&inst.game_dir).unwrap();
        std::fs::write(inst.game_dir.join("options.txt"), b"fov:0.5\n").unwrap();
        let zip_path = dir.path().join("pack.mrpack");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("overrides/config/test.txt", options)
                .unwrap();
            zip.write_all(b"hello").unwrap();
            zip.start_file("client-overrides/options.txt", options)
                .unwrap();
            zip.write_all(b"fov:0.8\n").unwrap();
            zip.start_file("server-overrides/server.properties", options)
                .unwrap();
            zip.write_all(b"no").unwrap();
            zip.finish().unwrap();
        }
        let count =
            extract_overrides_blocking(&zip_path, &inst, &default_override_prefixes()).unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            std::fs::read_to_string(inst.game_dir.join("config/test.txt")).unwrap(),
            "hello"
        );
        assert_eq!(
            std::fs::read_to_string(inst.game_dir.join("options.txt")).unwrap(),
            "fov:0.8\n"
        );
        assert!(!inst.game_dir.join("server.properties").exists());
        let leftovers: Vec<_> = std::fs::read_dir(&inst.game_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with(".srl-import-staging-") || name.starts_with(".srl-import-backup-")
            })
            .collect();
        assert!(leftovers.is_empty());
    }
}
