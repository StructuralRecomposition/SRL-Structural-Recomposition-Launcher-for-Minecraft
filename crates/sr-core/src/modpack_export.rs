//! Modrinth `.mrpack` export.
//!
//! The exporter prefers a portable modpack over a byte-for-byte instance
//! backup: mods that can be identified on Modrinth become `files` entries
//! (downloaded by the importing launcher), everything else is copied into
//! `overrides/`. This keeps packs small and lets other launchers (and future
//! SRL versions) rebuild the instance from metadata.
//!
//! Trust rules:
//! * Only files inside the instance's `mods/` directory are hashed.
//! * Modrinth download URLs must be HTTPS on the official CDN.
//! * Hashes are computed locally; an API response that disagrees is ignored
//!   and the mod falls back to `overrides/`.
//! * Symlinks are skipped everywhere.

use crate::download::ensure_trusted_url;
use crate::error::{AppError, ErrorCode, Result};
use crate::instance::{Instance, ModLoaderInfo};
use crate::paths::GamePaths;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha512};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const MODRINTH_API: &str = "https://api.modrinth.com/v2";
const MAX_MODS: usize = 2_000;
const MAX_ENTRIES: usize = 20_000;
const MAX_ENTRY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const HASH_BATCH: usize = 100;

/// Directories copied verbatim into `overrides/`. Everything else (saves,
/// logs, screenshots, crash reports, launcher metadata) is intentionally left
/// out: a modpack is not a personal backup.
const OVERRIDE_DIRS: &[&str] = &[
    "config",
    "defaultconfigs",
    "kubejs",
    "scripts",
    "resourcepacks",
    "shaderpacks",
];
const OVERRIDE_FILES: &[&str] = &["options.txt", "optionsof.txt", "servers.dat", "icon.png"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackExportSummary {
    pub path: PathBuf,
    pub name: String,
    pub version_id: String,
    pub minecraft_version: String,
    pub resolved_mods: u32,
    pub bundled_mods: u32,
    pub overrides: u32,
    pub bytes: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct HashedMod {
    file_name: String,
    relative: String,
    size: u64,
    sha1: String,
    sha512: String,
    enabled: bool,
}

#[derive(Debug, Clone)]
struct ResolvedMod {
    mod_info: HashedMod,
    url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModrinthIndex {
    format_version: u32,
    game: String,
    version_id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    files: Vec<IndexFile>,
    dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct IndexFile {
    path: String,
    hashes: BTreeMap<String, String>,
    env: BTreeMap<String, String>,
    downloads: Vec<String>,
    file_size: u64,
}

#[derive(Debug, Deserialize)]
struct ApiVersion {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    project_id: String,
    #[serde(default)]
    files: Vec<ApiFile>,
}

#[derive(Debug, Deserialize)]
struct ApiFile {
    #[serde(default)]
    hashes: HashMap<String, String>,
    url: String,
    #[serde(default)]
    size: u64,
}

/// Export `instance` as a Modrinth `.mrpack` archive.
pub async fn export_mrpack(
    game: &GamePaths,
    instance: &Instance,
    dest: &Path,
    overwrite: bool,
    name: Option<String>,
    version_id: Option<String>,
    http: &reqwest::Client,
) -> Result<ModpackExportSummary> {
    if dest.extension().and_then(|e| e.to_str()) != Some("mrpack") {
        return Err(AppError::invalid_input("整合包导出文件必须以 .mrpack 结尾"));
    }
    if dest.starts_with(&instance.game_dir) {
        return Err(AppError::invalid_input("导出文件不能放在实例目录内部"));
    }
    if tokio::fs::try_exists(dest).await.unwrap_or(false) && !overwrite {
        return Err(AppError::new(
            ErrorCode::AlreadyExists,
            "导出文件已存在，请更换路径或确认覆盖",
        ));
    }
    if !tokio::fs::try_exists(&instance.game_dir)
        .await
        .unwrap_or(false)
    {
        return Err(AppError::not_found("实例目录不存在"));
    }

    let pack_name = name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&instance.name)
        .to_string();
    let pack_version = version_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("1.0.0")
        .to_string();
    if pack_name.len() > 128 || pack_version.len() > 64 {
        return Err(AppError::invalid_input("整合包名称或版本号过长"));
    }
    let minecraft_version = base_minecraft_version(game, &instance.version_id).await?;

    let mods_dir = instance.game_dir.join("mods");
    let hashed = hash_mods(mods_dir).await?;
    let mut warnings = Vec::new();

    let (resolved, bundled) = resolve_mods(http, &hashed, &mut warnings).await;
    let mut index_files = Vec::with_capacity(resolved.len());
    for item in &resolved {
        let mut hashes = BTreeMap::new();
        hashes.insert("sha1".to_string(), item.mod_info.sha1.clone());
        hashes.insert("sha512".to_string(), item.mod_info.sha512.clone());
        let mut env = BTreeMap::new();
        // SRL is a client launcher; do not claim server compatibility for mods
        // whose side metadata we did not inspect.
        env.insert("client".to_string(), "required".to_string());
        env.insert("server".to_string(), "optional".to_string());
        index_files.push(IndexFile {
            path: item.mod_info.relative.clone(),
            hashes,
            env,
            downloads: vec![item.url.clone()],
            file_size: item.mod_info.size,
        });
    }

    let mut dependencies = BTreeMap::new();
    dependencies.insert("minecraft".to_string(), minecraft_version.clone());
    if let Some(loader) = loader_dependency(instance.mod_loader.as_ref()) {
        dependencies.insert(loader.0, loader.1);
    } else if instance.mod_loader.is_some() {
        warnings.push("无法识别模组加载器版本，导出的整合包只声明了 Minecraft 版本".into());
    }

    let index = ModrinthIndex {
        format_version: 1,
        game: "minecraft".to_string(),
        version_id: pack_version.clone(),
        name: pack_name.clone(),
        summary: Some(format!("由 SRL 导出的 {} 实例", instance.name)),
        files: index_files,
        dependencies,
    };
    let index_bytes = serde_json::to_vec_pretty(&index)?;

    let dir = instance.game_dir.clone();
    let dest_buf = dest.to_path_buf();
    let bundled_for_task = bundled.clone();
    let (overrides, bytes) = tokio::task::spawn_blocking(move || {
        write_mrpack(&dir, &dest_buf, &index_bytes, &bundled_for_task, overwrite)
    })
    .await
    .map_err(|e| AppError::internal(format!("整合包导出任务失败：{e}")))??;

    let summary = ModpackExportSummary {
        path: dest.to_path_buf(),
        name: pack_name,
        version_id: pack_version,
        minecraft_version,
        resolved_mods: resolved.len() as u32,
        bundled_mods: bundled.len() as u32,
        overrides,
        bytes,
        warnings,
    };
    tracing::info!(
        instance = %instance.id,
        resolved = summary.resolved_mods,
        bundled = summary.bundled_mods,
        "modpack exported"
    );
    Ok(summary)
}

/// Walk `inheritsFrom`/`jar` until the vanilla version id is reached. Falls
/// back to parsing the loader-style version id when the metadata is missing.
async fn base_minecraft_version(game: &GamePaths, version_id: &str) -> Result<String> {
    let mut current = version_id.to_string();
    for _ in 0..16 {
        let path = game.version_json(&current);
        let text = match tokio::fs::read_to_string(&path).await {
            Ok(text) => text,
            Err(_) => break,
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(_) => break,
        };
        if let Some(jar) = value.get("jar").and_then(|v| v.as_str()) {
            if !jar.is_empty() && jar != current {
                current = jar.to_string();
                continue;
            }
        }
        if let Some(parent) = value.get("inheritsFrom").and_then(|v| v.as_str()) {
            if !parent.is_empty() && parent != current {
                current = parent.to_string();
                continue;
            }
        }
        return Ok(current);
    }
    Ok(parse_minecraft_version(version_id).unwrap_or_else(|| version_id.to_string()))
}

fn parse_minecraft_version(version_id: &str) -> Option<String> {
    if let Some(rest) = version_id.strip_prefix("fabric-loader-") {
        return rest.rsplit_once('-').map(|(_, mc)| mc.to_string());
    }
    if let Some(rest) = version_id.strip_prefix("quilt-loader-") {
        return rest.rsplit_once('-').map(|(_, mc)| mc.to_string());
    }
    if let Some((mc, _)) = version_id.split_once("-forge-") {
        return Some(mc.to_string());
    }
    if let Some(rest) = version_id.strip_prefix("neoforge-") {
        // NeoForge 21.1.77 -> Minecraft 1.21.1, 20.6.1 -> 1.20.6.
        let mut parts = rest.split('.');
        let major = parts.next()?;
        let minor = parts.next()?;
        return Some(format!("1.{major}.{minor}"));
    }
    if !version_id.contains('-') {
        return Some(version_id.to_string());
    }
    None
}

fn loader_dependency(loader: Option<&ModLoaderInfo>) -> Option<(String, String)> {
    let loader = loader?;
    if loader.version.trim().is_empty() {
        return None;
    }
    let key = match loader.kind.to_ascii_lowercase().as_str() {
        "fabric" => "fabric-loader",
        "quilt" => "quilt-loader",
        "forge" => "forge",
        "neoforge" => "neoforge",
        _ => return None,
    };
    Some((key.to_string(), loader.version.clone()))
}

async fn hash_mods(mods_dir: PathBuf) -> Result<Vec<HashedMod>> {
    tokio::task::spawn_blocking(move || hash_mods_blocking(&mods_dir))
        .await
        .map_err(|e| AppError::internal(format!("读取模组目录失败：{e}")))?
}

fn hash_mods_blocking(mods_dir: &Path) -> Result<Vec<HashedMod>> {
    let mut out = Vec::new();
    if let Ok(meta) = std::fs::symlink_metadata(mods_dir) {
        if meta.file_type().is_symlink() {
            return Err(AppError::path_unsafe("模组目录不能是符号链接"));
        }
    }
    let rd = match std::fs::read_dir(mods_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }
        let file_name = match path.file_name().and_then(|s| s.to_str()) {
            Some(value) => value.to_string(),
            None => continue,
        };
        let enabled = file_name.ends_with(".jar");
        if !enabled && !file_name.ends_with(".jar.disabled") {
            continue;
        }
        if meta.len() > MAX_ENTRY_BYTES {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("模组 {file_name} 超过 2GB，无法导出"),
            ));
        }
        if out.len() >= MAX_MODS {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("模组数量超过 {MAX_MODS} 个，无法导出"),
            ));
        }
        let (sha1, sha512) = hash_file(&path)?;
        out.push(HashedMod {
            relative: format!("mods/{file_name}"),
            file_name,
            size: meta.len(),
            sha1,
            sha512,
            enabled,
        });
    }
    out.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(out)
}

fn hash_file(path: &Path) -> Result<(String, String)> {
    let mut file = std::fs::File::open(path)?;
    let mut sha1 = Sha1::new();
    let mut sha512 = Sha512::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        sha1.update(&buffer[..read]);
        sha512.update(&buffer[..read]);
    }
    Ok((hex::encode(sha1.finalize()), hex::encode(sha512.finalize())))
}

/// Ask Modrinth which local mods are identifiable. Network failures are not
/// fatal: every mod falls back to `overrides/`, so the exported pack still
/// reproduces the instance exactly.
async fn resolve_mods(
    http: &reqwest::Client,
    mods: &[HashedMod],
    warnings: &mut Vec<String>,
) -> (Vec<ResolvedMod>, Vec<HashedMod>) {
    let enabled: Vec<&HashedMod> = mods.iter().filter(|m| m.enabled).collect();
    let mut bundled: Vec<HashedMod> = mods.iter().filter(|m| !m.enabled).cloned().collect();
    let mut resolved = Vec::new();
    if enabled.is_empty() {
        return (resolved, bundled);
    }

    let mut found: HashMap<String, ApiVersion> = HashMap::new();
    let mut had_error = false;
    for batch in enabled.chunks(HASH_BATCH) {
        let hashes: Vec<String> = batch.iter().map(|m| m.sha1.clone()).collect();
        match lookup_versions(http, &hashes).await {
            Ok(map) => found.extend(map),
            Err(error) => {
                had_error = true;
                tracing::warn!(error = %error, "modrinth hash lookup failed");
                break;
            }
        }
    }
    if had_error {
        warnings.push(
            "无法连接 Modrinth，所有模组已放入 overrides/；整合包仍然完整，但体积会更大".into(),
        );
    }

    for item in enabled {
        let Some(version) = found.get(&item.sha1) else {
            bundled.push(item.clone());
            continue;
        };
        let Some(file) = version.files.iter().find(|f| {
            f.hashes
                .get("sha1")
                .is_some_and(|h| h.eq_ignore_ascii_case(&item.sha1))
        }) else {
            bundled.push(item.clone());
            continue;
        };
        if file.size != 0 && file.size != item.size {
            bundled.push(item.clone());
            continue;
        }
        let Some(api_sha512) = file.hashes.get("sha512") else {
            bundled.push(item.clone());
            continue;
        };
        if !api_sha512.eq_ignore_ascii_case(&item.sha512) {
            bundled.push(item.clone());
            continue;
        }
        if ensure_trusted_url(&file.url, false).is_err() {
            bundled.push(item.clone());
            continue;
        }
        resolved.push(ResolvedMod {
            mod_info: item.clone(),
            url: file.url.clone(),
        });
    }

    if !resolved.is_empty() && !had_error {
        let unresolved = bundled.iter().filter(|m| m.enabled).count();
        if unresolved > 0 {
            tracing::info!(unresolved, "mods exported as overrides");
        }
    }
    (resolved, bundled)
}

async fn lookup_versions(
    http: &reqwest::Client,
    hashes: &[String],
) -> Result<HashMap<String, ApiVersion>> {
    let response = http
        .post(format!("{MODRINTH_API}/version_files"))
        .header("Accept", "application/json")
        .json(&serde_json::json!({ "hashes": hashes, "algorithm": "sha1" }))
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| AppError::network("无法连接 Modrinth").with_detail(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::new(
            ErrorCode::Http,
            format!("Modrinth 返回 HTTP {}", response.status().as_u16()),
        ));
    }
    response
        .json::<HashMap<String, ApiVersion>>()
        .await
        .map_err(|e| {
            AppError::new(ErrorCode::Http, "无法解析 Modrinth 响应").with_detail(e.to_string())
        })
}

fn write_mrpack(
    instance_dir: &Path,
    dest: &Path,
    index_bytes: &[u8],
    bundled: &[HashedMod],
    overwrite: bool,
) -> Result<(u32, u64)> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    for mod_info in bundled {
        let path = instance_dir.join("mods").join(&mod_info.file_name);
        let is_real_file = std::fs::symlink_metadata(&path)
            .map(|meta| meta.is_file() && !meta.file_type().is_symlink())
            .unwrap_or(false);
        if is_real_file {
            entries.push((mod_info.relative.clone(), path));
        }
    }
    for dir in OVERRIDE_DIRS {
        let root = instance_dir.join(dir);
        let is_real_dir = std::fs::symlink_metadata(&root)
            .map(|meta| meta.is_dir() && !meta.file_type().is_symlink())
            .unwrap_or(false);
        if is_real_dir {
            collect_tree(instance_dir, &root, &mut entries)?;
        }
    }
    for file in OVERRIDE_FILES {
        let path = instance_dir.join(file);
        let is_real_file = std::fs::symlink_metadata(&path)
            .map(|meta| meta.is_file() && !meta.file_type().is_symlink())
            .unwrap_or(false);
        if is_real_file {
            entries.push((file.to_string(), path));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    if entries.len() > MAX_ENTRIES {
        return Err(AppError::new(
            ErrorCode::ZipUnsafe,
            format!("导出文件数量超过 {MAX_ENTRIES} 个"),
        ));
    }
    let mut total = 0u64;
    for (_, path) in &entries {
        total += std::fs::metadata(path)?.len();
        if total > MAX_TOTAL_BYTES {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                "整合包导出体积超过 4GB",
            ));
        }
    }

    let tmp = dest.with_file_name(format!(
        ".{}.tmp-{}",
        dest.file_name().unwrap_or_default().to_string_lossy(),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    let result = (|| -> Result<()> {
        let file = std::fs::File::create(&tmp)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        zip.start_file("modrinth.index.json", options)
            .map_err(|e| AppError::internal(format!("写入整合包失败：{e}")))?;
        zip.write_all(index_bytes)?;
        for (rel, path) in &entries {
            let name = format!("overrides/{rel}");
            zip.start_file(name, options)
                .map_err(|e| AppError::internal(format!("写入整合包失败：{e}")))?;
            let mut src = std::fs::File::open(path)?;
            std::io::copy(&mut src, &mut zip)?;
        }
        zip.finish()
            .map_err(|e| AppError::internal(format!("完成整合包失败：{e}")))?;
        if dest.exists() {
            if !overwrite {
                return Err(AppError::new(ErrorCode::AlreadyExists, "导出文件已存在"));
            }
            std::fs::remove_file(dest)?;
        }
        std::fs::rename(&tmp, dest)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    Ok((entries.len() as u32, total))
}

fn collect_tree(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let rd = std::fs::read_dir(dir)?;
    for entry in rd.flatten() {
        let path = entry.path();
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            collect_tree(root, &path, out)?;
            continue;
        }
        if !meta.is_file() || meta.len() > MAX_ENTRY_BYTES {
            continue;
        }
        let name = match path.strip_prefix(root).ok().and_then(path_to_zip_name) {
            Some(name) => name,
            None => continue,
        };
        if name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        {
            continue;
        }
        if name.ends_with(".DS_Store") {
            continue;
        }
        out.push((name, path));
        if out.len() > MAX_ENTRIES {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("导出文件数量超过 {MAX_ENTRIES} 个"),
            ));
        }
    }
    Ok(())
}

fn path_to_zip_name(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let text = value.to_str()?;
                if text.contains('\\') || text.contains('\0') {
                    return None;
                }
                parts.push(text.to_string());
            }
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_loader_versions() {
        assert_eq!(
            parse_minecraft_version("fabric-loader-0.16.9-1.21.1").as_deref(),
            Some("1.21.1")
        );
        assert_eq!(
            parse_minecraft_version("quilt-loader-0.27.0-1.20.6").as_deref(),
            Some("1.20.6")
        );
        assert_eq!(
            parse_minecraft_version("1.21.1-forge-52.0.1").as_deref(),
            Some("1.21.1")
        );
        assert_eq!(
            parse_minecraft_version("neoforge-21.1.77").as_deref(),
            Some("1.21.1")
        );
        assert_eq!(parse_minecraft_version("1.12.2").as_deref(), Some("1.12.2"));
    }

    #[test]
    fn loader_dependency_maps_kinds() {
        let loader = ModLoaderInfo {
            kind: "Fabric".into(),
            version: "0.16.9".into(),
            installed: true,
        };
        assert_eq!(
            loader_dependency(Some(&loader)),
            Some(("fabric-loader".into(), "0.16.9".into()))
        );
        let unknown = ModLoaderInfo {
            kind: "Litelo".into(),
            version: "1".into(),
            installed: true,
        };
        assert_eq!(loader_dependency(Some(&unknown)), None);
    }

    #[test]
    fn writes_expected_archive_layout() {
        let dir = tempfile::tempdir().unwrap();
        let instance = dir.path().join("inst");
        std::fs::create_dir_all(instance.join("mods")).unwrap();
        std::fs::create_dir_all(instance.join("config")).unwrap();
        std::fs::write(instance.join("mods/manual.jar"), b"jar").unwrap();
        std::fs::write(instance.join("config/test.toml"), b"x = 1").unwrap();
        let bundled = vec![HashedMod {
            file_name: "manual.jar".into(),
            relative: "mods/manual.jar".into(),
            size: 3,
            sha1: "a".into(),
            sha512: "b".into(),
            enabled: true,
        }];
        let dest = dir.path().join("pack.mrpack");
        let index = br#"{"formatVersion":1}"#;
        let (count, _) = write_mrpack(&instance, &dest, index, &bundled, false).unwrap();
        assert_eq!(count, 2);
        let file = std::fs::File::open(&dest).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        assert!(zip.by_name("modrinth.index.json").is_ok());
        assert!(zip.by_name("overrides/mods/manual.jar").is_ok());
        assert!(zip.by_name("overrides/config/test.toml").is_ok());
    }
}
