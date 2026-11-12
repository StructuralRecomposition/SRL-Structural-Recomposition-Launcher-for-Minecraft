//! Resource pack (材质包) management for an instance's `resourcepacks/` folder.
//!
//! Minecraft enables packs through the `resourcePacks` JSON array in
//! `options.txt`; the files themselves are never renamed. The launcher mirrors
//! that behaviour and only touches that one line. The offline skin pack
//! (`SRLocalSkin.zip`) is managed by the account/skin subsystem and is read-only
//! here.

use crate::error::{AppError, ErrorCode, Result};
use crate::instance::Instance;
use crate::util;
use serde::Serialize;
use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};

pub const MANAGED_PACK: &str = "SRLocalSkin.zip";
pub const MAX_RESOURCEPACK_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_MCMETA_BYTES: usize = 64 * 1024;
const MAX_UNCOMPRESSED_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_SCANNED_FILES: u64 = 50_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePackEntry {
    pub file_name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_at: Option<String>,
    /// Present in `options.txt`'s `resourcePacks` list as `file/<name>`.
    pub enabled: bool,
    /// `SRLocalSkin.zip` is generated from the account's skin; the UI must not
    /// toggle or delete it here.
    pub managed: bool,
}

pub fn resourcepacks_dir(instance: &Instance) -> PathBuf {
    instance.game_dir.join("resourcepacks")
}

fn options_path(instance: &Instance) -> PathBuf {
    instance.game_dir.join("options.txt")
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 255
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.contains(':')
        || name == "."
        || name == ".."
        || name.chars().any(|c| c.is_control())
    {
        return Err(AppError::path_unsafe("材质包名称非法"));
    }
    Ok(())
}

async fn dir_stats(root: &Path) -> u64 {
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut rd = match tokio::fs::read_dir(&dir).await {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        while let Ok(Some(entry)) = rd.next_entry().await {
            if files >= MAX_SCANNED_FILES {
                return bytes;
            }
            let Ok(meta) = tokio::fs::symlink_metadata(entry.path()).await else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                bytes = bytes.saturating_add(meta.len());
                files += 1;
            }
        }
    }
    bytes
}

async fn entry_for(dir: &Path, file_name: &str, enabled: bool) -> Result<ResourcePackEntry> {
    let path = dir.join(file_name);
    let meta = tokio::fs::symlink_metadata(&path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("材质包不存在")
        } else {
            e.into()
        }
    })?;
    if meta.file_type().is_symlink() {
        return Err(AppError::path_unsafe("拒绝处理符号链接"));
    }
    let is_dir = meta.is_dir();
    let size = if is_dir {
        dir_stats(&path).await
    } else {
        meta.len()
    };
    let modified_at = meta
        .modified()
        .ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    Ok(ResourcePackEntry {
        file_name: file_name.to_string(),
        is_dir,
        size,
        modified_at,
        enabled,
        managed: file_name == MANAGED_PACK,
    })
}

/// Parse `resourcePacks:[...]` from options.txt. `None` means the line exists
/// but is not a JSON array (callers must not rewrite it).
fn parse_packs(content: &str) -> Option<Vec<String>> {
    const KEY: &str = "resourcePacks:";
    for line in content.lines() {
        if let Some(raw) = line.strip_prefix(KEY) {
            let raw = raw.trim();
            if raw.is_empty() {
                return Some(Vec::new());
            }
            return serde_json::from_str::<Vec<String>>(raw).ok();
        }
    }
    Some(Vec::new())
}

async fn selected_packs(instance: &Instance) -> Option<Vec<String>> {
    match tokio::fs::read_to_string(options_path(instance)).await {
        Ok(content) => parse_packs(&content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Vec::new()),
        Err(_) => None,
    }
}

async fn mutate_packs(
    instance: &Instance,
    create_if_missing: bool,
    mutate: impl FnOnce(&mut Vec<String>) -> bool,
) -> Result<bool> {
    let path = options_path(instance);
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if !create_if_missing {
                return Ok(false);
            }
            String::new()
        }
        Err(e) => return Err(e.into()),
    };
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    const KEY: &str = "resourcePacks:";
    let index = lines.iter().position(|l| l.starts_with(KEY));
    let mut packs = match index {
        Some(i) => {
            let raw = lines[i][KEY.len()..].trim();
            if raw.is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(raw).map_err(|_| {
                    AppError::internal(
                        "options.txt 中的 resourcePacks 不是合法 JSON，已保持原样（未改写）",
                    )
                })?
            }
        }
        None => Vec::new(),
    };
    if !mutate(&mut packs) {
        return Ok(false);
    }
    let line = format!("{KEY}{}", serde_json::to_string(&packs)?);
    match index {
        Some(i) => lines[i] = line,
        None => lines.push(line),
    }
    let mut text = lines.join("\n");
    text.push('\n');
    util::atomic_write(&path, text.as_bytes()).await?;
    Ok(true)
}

pub async fn list(instance: &Instance) -> Result<Vec<ResourcePackEntry>> {
    let dir = resourcepacks_dir(instance);
    let selected = selected_packs(instance).await;
    if selected.is_none() {
        tracing::warn!(instance = %instance.id, "options.txt resourcePacks is malformed");
    }
    let selected = selected.unwrap_or_default();
    let mut out = Vec::new();
    let mut rd = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    while let Some(entry) = rd.next_entry().await? {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with('.') || validate_name(&file_name).is_err() {
            continue;
        }
        let Ok(meta) = tokio::fs::symlink_metadata(entry.path()).await else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let is_zip = meta.is_file() && file_name.to_ascii_lowercase().ends_with(".zip");
        if !meta.is_dir() && !is_zip {
            continue;
        }
        let key = format!("file/{file_name}");
        let enabled = selected.iter().any(|p| p == &key);
        if let Ok(item) = entry_for(&dir, &file_name, enabled).await {
            out.push(item);
        }
    }
    out.sort_by(|a, b| {
        b.enabled
            .cmp(&a.enabled)
            .then_with(|| a.file_name.cmp(&b.file_name))
    });
    Ok(out)
}

pub async fn set_enabled(
    instance: &Instance,
    file_name: &str,
    enabled: bool,
) -> Result<ResourcePackEntry> {
    validate_name(file_name)?;
    if file_name == MANAGED_PACK {
        return Err(AppError::unsupported(
            "SRLocalSkin.zip 由账号皮肤自动管理，不能在这里启用或禁用",
        ));
    }
    let dir = resourcepacks_dir(instance);
    entry_for(&dir, file_name, false).await?;
    let key = format!("file/{file_name}");
    mutate_packs(instance, true, |packs| {
        let has = packs.iter().any(|p| p == &key);
        if enabled && !has {
            packs.push(key.clone());
            true
        } else if !enabled && has {
            packs.retain(|p| p != &key);
            true
        } else {
            false
        }
    })
    .await?;
    entry_for(&dir, file_name, enabled).await
}

pub async fn delete(instance: &Instance, file_name: &str) -> Result<()> {
    validate_name(file_name)?;
    if file_name == MANAGED_PACK {
        return Err(AppError::unsupported(
            "SRLocalSkin.zip 由账号皮肤自动管理，不能在材质管理中删除",
        ));
    }
    let dir = resourcepacks_dir(instance);
    let path = dir.join(file_name);
    let meta = tokio::fs::symlink_metadata(&path).await?;
    if meta.file_type().is_symlink() {
        return Err(AppError::path_unsafe("拒绝删除符号链接"));
    }
    let key = format!("file/{file_name}");
    mutate_packs(instance, false, |packs| {
        let before = packs.len();
        packs.retain(|p| p != &key);
        packs.len() != before
    })
    .await?;
    if meta.is_dir() {
        tokio::fs::remove_dir_all(&path).await?;
    } else if meta.is_file() {
        tokio::fs::remove_file(&path).await?;
    } else {
        return Err(AppError::path_unsafe("拒绝删除非常规文件"));
    }
    Ok(())
}

fn validate_zip<R: Read + Seek>(reader: R) -> Result<()> {
    let mut zip = zip::ZipArchive::new(reader)
        .map_err(|e| AppError::invalid_input(format!("不是有效的 ZIP 文件：{e}")))?;
    let mut total_uncompressed = 0u64;
    let mut total_compressed = 0u64;
    let mut has_meta = false;
    for i in 0..zip.len() {
        let entry = match zip.by_index(i) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let name = entry.name()?.to_string();
        if name.starts_with('/') || name.contains('\\') || name.split('/').any(|part| part == "..")
        {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("材质包含不安全的路径：{name}"),
            ));
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                "材质包包含符号链接，已拒绝",
            ));
        }
        total_uncompressed = total_uncompressed.saturating_add(entry.size());
        total_compressed = total_compressed.saturating_add(entry.compressed_size());
        if total_uncompressed > MAX_UNCOMPRESSED_BYTES {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                "材质包解压后超过 4GB，已拒绝",
            ));
        }
        if entry.name()? == "pack.mcmeta" {
            has_meta = true;
            let mut meta = Vec::new();
            entry
                .take(MAX_MCMETA_BYTES as u64)
                .read_to_end(&mut meta)
                .map_err(|e| AppError::invalid_input(format!("无法读取 pack.mcmeta：{e}")))?;
            let value: serde_json::Value = serde_json::from_slice(&meta)
                .map_err(|e| AppError::invalid_input(format!("pack.mcmeta 不是合法 JSON：{e}")))?;
            let pack = value
                .get("pack")
                .ok_or_else(|| AppError::invalid_input("pack.mcmeta 缺少 pack 对象"))?;
            if pack.get("pack_format").is_none() && pack.get("supported_formats").is_none() {
                return Err(AppError::invalid_input(
                    "pack.mcmeta 缺少 pack_format / supported_formats",
                ));
            }
        }
    }
    if !has_meta {
        return Err(AppError::invalid_input(
            "ZIP 根目录缺少 pack.mcmeta，不是有效的 Minecraft 材质包",
        ));
    }
    if total_compressed > 0
        && total_uncompressed > 10 * 1024 * 1024
        && total_uncompressed / total_compressed > 1000
    {
        return Err(AppError::new(
            ErrorCode::ZipUnsafe,
            "材质包压缩比异常，已拒绝",
        ));
    }
    Ok(())
}

async fn validate_zip_path(path: &Path) -> Result<()> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&path)?;
        validate_zip(std::io::BufReader::new(file))
    })
    .await
    .map_err(|e| AppError::internal(format!("材质包校验任务失败：{e}")))?
}

async fn write_bytes(
    instance: &Instance,
    file_name: &str,
    bytes: &[u8],
    overwrite: bool,
) -> Result<ResourcePackEntry> {
    validate_name(file_name)?;
    if file_name == MANAGED_PACK {
        return Err(AppError::unsupported(
            "SRLocalSkin.zip 由账号皮肤自动管理，不能作为普通材质包导入",
        ));
    }
    if !file_name.to_ascii_lowercase().ends_with(".zip") {
        return Err(AppError::invalid_input("只能导入 .zip 材质包"));
    }
    if bytes.len() as u64 > MAX_RESOURCEPACK_BYTES {
        return Err(AppError::invalid_input("材质包超过 2GB"));
    }
    validate_zip(Cursor::new(bytes))?;
    let dir = resourcepacks_dir(instance);
    util::ensure_dir(&dir).await?;
    let dest = dir.join(file_name);
    if let Ok(meta) = tokio::fs::symlink_metadata(&dest).await {
        if meta.file_type().is_symlink() {
            return Err(AppError::path_unsafe("拒绝覆盖符号链接"));
        }
        if meta.is_dir() {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                format!("同名目录已存在：{file_name}"),
            ));
        }
        if !overwrite {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                format!("材质包已存在：{file_name}"),
            ));
        }
    }
    util::atomic_write(&dest, bytes).await?;
    entry_for(&dir, file_name, false).await
}

pub async fn import(
    instance: &Instance,
    source: &Path,
    overwrite: bool,
) -> Result<ResourcePackEntry> {
    let meta = tokio::fs::symlink_metadata(source).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("源材质包不存在")
        } else {
            e.into()
        }
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(AppError::path_unsafe(
            "源文件必须是普通文件，不能是符号链接",
        ));
    }
    if meta.len() > MAX_RESOURCEPACK_BYTES {
        return Err(AppError::invalid_input("材质包超过 2GB"));
    }
    let name = source
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::invalid_input("源文件名缺失"))?;
    validate_name(&name)?;
    if name == MANAGED_PACK {
        return Err(AppError::unsupported(
            "SRLocalSkin.zip 由账号皮肤自动管理，不能作为普通材质包导入",
        ));
    }
    if !name.to_ascii_lowercase().ends_with(".zip") {
        return Err(AppError::invalid_input("只能导入 .zip 材质包"));
    }
    validate_zip_path(source).await?;
    let dir = resourcepacks_dir(instance);
    util::ensure_dir(&dir).await?;
    let dest = dir.join(&name);
    if let Ok(dest_meta) = tokio::fs::symlink_metadata(&dest).await {
        if dest_meta.file_type().is_symlink() {
            return Err(AppError::path_unsafe("拒绝覆盖符号链接"));
        }
        if dest_meta.is_dir() {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                format!("同名目录已存在：{name}"),
            ));
        }
        if !overwrite {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                format!("材质包已存在：{name}"),
            ));
        }
    }
    let tmp = dir.join(format!(
        ".{name}.tmp-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    if let Err(e) = tokio::fs::copy(source, &tmp).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e.into());
    }
    if let Err(e) = crate::util::replace_file_async(&tmp, &dest).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    entry_for(&dir, &name, false).await
}

pub async fn import_bytes(
    instance: &Instance,
    file_name: &str,
    bytes: &[u8],
    overwrite: bool,
) -> Result<ResourcePackEntry> {
    write_bytes(instance, file_name, bytes, overwrite).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_instance(game_dir: &Path) -> Instance {
        Instance {
            id: "inst".into(),
            name: "test".into(),
            version_id: "1.21.1".into(),
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
            mod_loader: None,
        }
    }

    fn pack_zip(with_meta: bool) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
            let options = zip::write::SimpleFileOptions::default();
            if with_meta {
                zip.start_file("pack.mcmeta", options).unwrap();
                zip.write_all(br#"{"pack":{"pack_format":15,"description":"test"}}"#)
                    .unwrap();
            }
            zip.start_file("assets/minecraft/textures/test.txt", options)
                .unwrap();
            zip.write_all(b"x").unwrap();
            zip.finish().unwrap();
        }
        out
    }

    #[tokio::test]
    async fn lists_toggles_and_preserves_options() {
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        let rp = resourcepacks_dir(&inst);
        tokio::fs::create_dir_all(&rp).await.unwrap();
        tokio::fs::write(rp.join("Pack.zip"), pack_zip(true))
            .await
            .unwrap();
        tokio::fs::write(
            inst.game_dir.join("options.txt"),
            "fov:0.5\nresourcePacks:[\"vanilla\"]\nlang:zh_cn\n",
        )
        .await
        .unwrap();

        let list = list(&inst).await.unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].enabled);

        let entry = set_enabled(&inst, "Pack.zip", true).await.unwrap();
        assert!(entry.enabled);
        let options = tokio::fs::read_to_string(inst.game_dir.join("options.txt"))
            .await
            .unwrap();
        assert!(options.contains("\"vanilla\""));
        assert!(options.contains("\"file/Pack.zip\""));
        assert!(options.contains("fov:0.5"));
        assert!(options.contains("lang:zh_cn"));

        let entry = set_enabled(&inst, "Pack.zip", false).await.unwrap();
        assert!(!entry.enabled);
        let options = tokio::fs::read_to_string(inst.game_dir.join("options.txt"))
            .await
            .unwrap();
        assert!(!options.contains("file/Pack.zip"));
        assert!(options.contains("\"vanilla\""));
    }

    #[tokio::test]
    async fn imports_and_deletes_a_pack() {
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        let source = dir.path().join("source.zip");
        tokio::fs::write(&source, pack_zip(true)).await.unwrap();
        let entry = import(&inst, &source, false).await.unwrap();
        assert_eq!(entry.file_name, "source.zip");
        assert!(resourcepacks_dir(&inst).join("source.zip").exists());
        assert!(import(&inst, &source, false).await.is_err());
        let bytes = pack_zip(true);
        let entry = import_bytes(&inst, "from-bytes.zip", &bytes, false)
            .await
            .unwrap();
        assert_eq!(entry.file_name, "from-bytes.zip");
        delete(&inst, "source.zip").await.unwrap();
        assert!(!resourcepacks_dir(&inst).join("source.zip").exists());
    }

    #[tokio::test]
    async fn rejects_invalid_or_unsafe_zips() {
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        let err = import_bytes(&inst, "nometa.zip", &pack_zip(false), false)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(err.message.contains("pack.mcmeta"));

        let mut out = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("pack.mcmeta", options).unwrap();
            zip.write_all(br#"{"pack":{"pack_format":15}}"#).unwrap();
            zip.start_file("../evil.txt", options).unwrap();
            zip.write_all(b"x").unwrap();
            zip.finish().unwrap();
        }
        let err = import_bytes(&inst, "unsafe.zip", &out, false)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ZipUnsafe);
    }

    #[tokio::test]
    async fn managed_skin_pack_is_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        let rp = resourcepacks_dir(&inst);
        tokio::fs::create_dir_all(&rp).await.unwrap();
        tokio::fs::write(rp.join(MANAGED_PACK), pack_zip(true))
            .await
            .unwrap();
        let list = list(&inst).await.unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].managed);
        assert!(set_enabled(&inst, MANAGED_PACK, true).await.is_err());
        assert!(delete(&inst, MANAGED_PACK).await.is_err());
        assert!(import_bytes(&inst, MANAGED_PACK, &pack_zip(true), true)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn malformed_options_are_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        let rp = resourcepacks_dir(&inst);
        tokio::fs::create_dir_all(&rp).await.unwrap();
        tokio::fs::write(rp.join("Pack.zip"), pack_zip(true))
            .await
            .unwrap();
        tokio::fs::write(
            inst.game_dir.join("options.txt"),
            "resourcePacks:not-json\n",
        )
        .await
        .unwrap();
        let err = set_enabled(&inst, "Pack.zip", true).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
        let raw = tokio::fs::read_to_string(inst.game_dir.join("options.txt"))
            .await
            .unwrap();
        assert_eq!(raw, "resourcePacks:not-json\n");
    }
}
