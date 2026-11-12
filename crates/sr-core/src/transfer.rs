//! Safe instance export/import.
//!
//! Archives are plain ZIP files with an `sr-instance.json` manifest. Import
//! validates every entry before writing: no absolute paths, no `..`, no
//! backslashes, no symlinks, no duplicates, bounded entry count/size/ratio.
//! Tokens are never part of the manifest or archive.

use crate::error::{AppError, ErrorCode, Result};
use crate::instance::{Instance, ModLoaderInfo};
use crate::paths::GamePaths;
use crate::util;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const EXPORT_FORMAT: &str = "sr-instance-1";
const MANIFEST_NAME: &str = "sr-instance.json";
const MAX_ENTRIES: usize = 20_000;
const MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_ENTRY_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_RATIO: u64 = 1_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceArchiveManifest {
    pub format: String,
    pub exported_at: String,
    pub app_version: String,
    pub name: String,
    pub version_id: String,
    pub memory_mb: u32,
    #[serde(default)]
    pub jvm_args: Vec<String>,
    #[serde(default)]
    pub game_args: Vec<String>,
    #[serde(default)]
    pub resolution_width: Option<u32>,
    #[serde(default)]
    pub resolution_height: Option<u32>,
    #[serde(default)]
    pub mod_loader: Option<ModLoaderInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub path: PathBuf,
    pub files: u32,
    pub bytes: u64,
    pub skipped: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub manifest: InstanceArchiveManifest,
    pub file_count: u32,
    pub total_bytes: u64,
    pub warnings: Vec<String>,
}

fn validate_relative(name: &str) -> Result<String> {
    if name.is_empty() || name.len() > 1024 {
        return Err(AppError::new(ErrorCode::ZipUnsafe, "归档包含非法空路径"));
    }
    if name.contains('\0') || name.contains('\\') {
        return Err(AppError::new(
            ErrorCode::ZipUnsafe,
            format!("归档路径包含非法字符：{name}"),
        ));
    }
    let path = Path::new(name);
    if path.is_absolute() {
        return Err(AppError::new(
            ErrorCode::ZipUnsafe,
            format!("归档包含绝对路径：{name}"),
        ));
    }
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let text = part.to_string_lossy();
                if text.contains(':') {
                    return Err(AppError::new(
                        ErrorCode::ZipUnsafe,
                        format!("归档路径包含盘符：{name}"),
                    ));
                }
                if text.ends_with('.') || text.ends_with(' ') {
                    return Err(AppError::new(
                        ErrorCode::ZipUnsafe,
                        format!("归档路径包含 Windows 非法结尾：{name}"),
                    ));
                }
                let stem = text.split('.').next().unwrap_or("").to_ascii_uppercase();
                let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                    || (stem.len() == 4
                        && (stem.starts_with("COM") || stem.starts_with("LPT"))
                        && stem.as_bytes()[3].is_ascii_digit());
                if reserved {
                    return Err(AppError::new(
                        ErrorCode::ZipUnsafe,
                        format!("归档路径使用 Windows 保留名：{name}"),
                    ));
                }
            }
            Component::CurDir => {}
            _ => {
                return Err(AppError::new(
                    ErrorCode::ZipUnsafe,
                    format!("归档路径包含向上引用：{name}"),
                ))
            }
        }
    }
    Ok(name.trim_start_matches("./").to_string())
}

fn excluded(relative: &str) -> bool {
    let first = relative.split('/').next().unwrap_or("");
    matches!(
        first,
        "logs" | "crash-reports" | ".mixin.out" | "instance.json"
    ) || first.starts_with(".sr")
}

fn is_symlink(meta: &std::fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

fn collect_files(root: &Path) -> Result<(Vec<(String, PathBuf)>, u32)> {
    let mut out = Vec::new();
    let mut skipped = 0u32;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                return Err(AppError::new(
                    ErrorCode::Io,
                    format!("无法读取实例目录：{}", dir.display()),
                )
                .with_detail(e.to_string()))
            }
        };
        for entry in entries {
            let entry = entry.map_err(|e| {
                AppError::new(ErrorCode::Io, "读取实例目录失败").with_detail(e.to_string())
            })?;
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path)?;
            if is_symlink(&meta) {
                skipped += 1;
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .map_err(|_| AppError::path_unsafe("实例路径越界"))?
                .to_string_lossy()
                .replace('\\', "/");
            if rel.is_empty() || excluded(&rel) {
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                if meta.len() > MAX_ENTRY_BYTES {
                    return Err(AppError::new(
                        ErrorCode::ZipUnsafe,
                        format!("文件过大，无法导出：{rel}"),
                    ));
                }
                out.push((rel, path));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok((out, skipped))
}

fn manifest_for(instance: &Instance) -> InstanceArchiveManifest {
    InstanceArchiveManifest {
        format: EXPORT_FORMAT.into(),
        exported_at: util::now_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        name: instance.name.clone(),
        version_id: instance.version_id.clone(),
        memory_mb: instance.memory_mb,
        jvm_args: instance.jvm_args.clone(),
        game_args: instance.game_args.clone(),
        resolution_width: instance.resolution_width,
        resolution_height: instance.resolution_height,
        mod_loader: instance.mod_loader.clone(),
    }
}

/// Export an instance (without logs/crash reports/tokens) to a zip. Writes to
/// a temporary archive first and renames on success.
pub async fn export_instance(
    game: &GamePaths,
    instance: &Instance,
    dest: &Path,
    overwrite: bool,
) -> Result<ExportSummary> {
    let dest = dest.to_path_buf();
    if dest.extension().and_then(|e| e.to_str()) != Some("zip") {
        return Err(AppError::invalid_input("导出文件必须以 .zip 结尾"));
    }
    if dest.starts_with(&instance.game_dir) {
        return Err(AppError::invalid_input("导出文件不能放在实例目录内部"));
    }
    if tokio::fs::try_exists(&dest).await.unwrap_or(false) && !overwrite {
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
    let _ = game;
    let dir = instance.game_dir.clone();
    let manifest = manifest_for(instance);
    tokio::task::spawn_blocking(move || export_blocking(&dir, &manifest, &dest, overwrite))
        .await
        .map_err(|e| AppError::internal(format!("导出任务失败：{e}")))?
}

fn export_blocking(
    dir: &Path,
    manifest: &InstanceArchiveManifest,
    dest: &Path,
    overwrite: bool,
) -> Result<ExportSummary> {
    let (files, skipped) = collect_files(dir)?;
    let mut total: u64 = 0;
    for (_, path) in &files {
        total += std::fs::metadata(path)?.len();
    }
    if total > MAX_TOTAL_BYTES {
        return Err(AppError::new(
            ErrorCode::ZipUnsafe,
            "实例体积超过 8GB，当前版本不支持导出",
        ));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
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
        zip.start_file(MANIFEST_NAME, options)
            .map_err(|e| AppError::internal(format!("写入归档失败：{e}")))?;
        zip.write_all(&serde_json::to_vec_pretty(manifest)?)?;
        for (rel, path) in &files {
            zip.start_file(rel, options)
                .map_err(|e| AppError::internal(format!("写入归档失败：{e}")))?;
            let mut src = std::fs::File::open(path)?;
            std::io::copy(&mut src, &mut zip)?;
        }
        zip.finish()
            .map_err(|e| AppError::internal(format!("完成归档失败：{e}")))?;
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
    Ok(ExportSummary {
        path: dest.to_path_buf(),
        files: files.len() as u32,
        bytes: total,
        skipped,
    })
}

fn open_archive(path: &Path) -> Result<zip::ZipArchive<std::fs::File>> {
    let file = std::fs::File::open(path).map_err(|e| {
        AppError::new(ErrorCode::Io, format!("无法打开归档：{}", path.display()))
            .with_detail(e.to_string())
    })?;
    zip::ZipArchive::new(file).map_err(|e| {
        AppError::new(ErrorCode::ZipUnsafe, "归档不是有效的 ZIP 文件").with_detail(e.to_string())
    })
}

fn read_manifest(zip: &mut zip::ZipArchive<std::fs::File>) -> Result<InstanceArchiveManifest> {
    let entry = zip.by_name(MANIFEST_NAME).map_err(|_| {
        AppError::new(
            ErrorCode::ZipUnsafe,
            "缺少 sr-instance.json，无法识别实例归档",
        )
    })?;
    if entry.size() > 1024 * 1024 {
        return Err(AppError::new(ErrorCode::ZipUnsafe, "归档清单异常过大"));
    }
    let mut bytes = Vec::new();
    entry
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| {
            AppError::new(ErrorCode::ZipUnsafe, "读取归档清单失败").with_detail(e.to_string())
        })?;
    let manifest: InstanceArchiveManifest = serde_json::from_slice(&bytes).map_err(|e| {
        AppError::new(ErrorCode::ZipUnsafe, "归档清单格式无效").with_detail(e.to_string())
    })?;
    if manifest.format != EXPORT_FORMAT {
        return Err(AppError::new(
            ErrorCode::Unsupported,
            format!("不支持的实例归档格式：{}", manifest.format),
        ));
    }
    if manifest.version_id.trim().is_empty() {
        return Err(AppError::new(
            ErrorCode::ZipUnsafe,
            "归档缺少 Minecraft 版本",
        ));
    }
    Ok(manifest)
}

/// Inspect an archive without writing anything.
pub async fn inspect_archive(path: &Path) -> Result<ImportPreview> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || inspect_blocking(&path))
        .await
        .map_err(|e| AppError::internal(format!("检查任务失败：{e}")))?
}

fn inspect_blocking(path: &Path) -> Result<ImportPreview> {
    let mut zip = open_archive(path)?;
    let manifest = read_manifest(&mut zip)?;
    let mut names = std::collections::HashSet::new();
    let mut file_count = 0u32;
    let mut total_bytes = 0u64;
    let mut warnings = Vec::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| {
            AppError::new(ErrorCode::ZipUnsafe, "无法读取归档条目").with_detail(e.to_string())
        })?;
        if entry.name()? == MANIFEST_NAME || entry.is_dir() {
            continue;
        }
        let name = validate_relative(&entry.name()?)?;
        if !names.insert(name.clone()) {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("归档包含重复路径：{name}"),
            ));
        }
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("归档条目过大：{name}"),
            ));
        }
        if entry.compressed_size() > 0
            && entry.size() > 50 * 1024 * 1024
            && entry.size() / entry.compressed_size() > MAX_RATIO
        {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("归档条目压缩比异常：{name}"),
            ));
        }
        file_count += 1;
        total_bytes = total_bytes.saturating_add(entry.size());
        if file_count > MAX_ENTRIES as u32 {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "归档文件数量过多"));
        }
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "归档解压后超过 8GB"));
        }
        if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("归档包含符号链接：{name}"),
            ));
        }
    }
    if file_count == 0 {
        warnings.push("归档只包含清单，没有游戏文件".to_string());
    }
    Ok(ImportPreview {
        manifest,
        file_count,
        total_bytes,
        warnings,
    })
}

/// Import an inspected archive as a new instance. Requires the referenced
/// Minecraft version to be installed. Extraction goes to a staging directory
/// and is only promoted after every entry succeeds.
pub async fn import_instance(
    game: &GamePaths,
    archive: &Path,
    new_name: Option<String>,
) -> Result<Instance> {
    let preview = inspect_archive(archive).await?;
    if !tokio::fs::try_exists(game.version_json(&preview.manifest.version_id))
        .await
        .unwrap_or(false)
    {
        return Err(AppError::not_found(format!(
            "请先安装 Minecraft {} 再导入该实例",
            preview.manifest.version_id
        )));
    }
    let id = util::short_id();
    let final_dir = game.instances_dir().join(&id);
    if tokio::fs::try_exists(&final_dir).await.unwrap_or(false) {
        return Err(AppError::new(ErrorCode::AlreadyExists, "实例目录冲突"));
    }
    let staging = game.instances_dir().join(format!(
        ".import-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    ));
    util::ensure_dir(&staging).await?;

    let archive = archive.to_path_buf();
    let staging_for_task = staging.clone();
    let extract =
        tokio::task::spawn_blocking(move || extract_blocking(&archive, &staging_for_task))
            .await
            .map_err(|e| AppError::internal(format!("导入任务失败：{e}")))?;
    if let Err(e) = extract {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(e);
    }

    let name = new_name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| preview.manifest.name.clone());
    let name: String = name.chars().take(64).collect();
    let instance = Instance {
        id: id.clone(),
        name,
        version_id: preview.manifest.version_id.clone(),
        game_dir: final_dir.clone(),
        java_path: None,
        memory_mb: preview.manifest.memory_mb.clamp(512, 65_536),
        jvm_args: preview.manifest.jvm_args.clone(),
        game_args: preview.manifest.game_args.clone(),
        created_at: util::now_rfc3339(),
        last_launched_at: None,
        account_id: None,
        resolution_width: None,
        resolution_height: None,
        mod_loader: preview.manifest.mod_loader.clone(),
    };
    util::write_json(&staging.join("instance.json"), &instance).await?;

    if let Err(e) = tokio::fs::rename(&staging, &final_dir).await {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return Err(e.into());
    }
    Ok(instance)
}

fn extract_blocking(archive: &Path, staging: &Path) -> Result<()> {
    let mut zip = open_archive(archive)?;
    let manifest = read_manifest(&mut zip)?;
    if manifest.format != EXPORT_FORMAT {
        return Err(AppError::new(ErrorCode::Unsupported, "不支持的归档格式"));
    }
    let mut seen = std::collections::HashSet::new();
    let mut total_written: u64 = 0;
    let mut file_count: u32 = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| {
            AppError::new(ErrorCode::ZipUnsafe, "无法读取归档条目").with_detail(e.to_string())
        })?;
        if entry.name()? == MANIFEST_NAME || entry.is_dir() {
            continue;
        }
        let name = validate_relative(&entry.name()?)?;
        if !seen.insert(name.clone()) {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("归档包含重复路径：{name}"),
            ));
        }
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "归档条目过大"));
        }
        if entry.compressed_size() > 0
            && entry.size() > 50 * 1024 * 1024
            && entry.size() / entry.compressed_size() > MAX_RATIO
        {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("归档条目压缩比异常：{name}"),
            ));
        }
        file_count += 1;
        if file_count > MAX_ENTRIES as u32 {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "归档文件数量过多"));
        }
        let dest = util::safe_join(staging, &name)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&dest)?;
        let written = std::io::copy(&mut entry, &mut out)?;
        if written != entry.size() {
            return Err(AppError::new(
                ErrorCode::ZipUnsafe,
                format!("归档条目解压大小不一致：{name}"),
            ));
        }
        total_written = total_written.saturating_add(written);
        if total_written > MAX_TOTAL_BYTES {
            return Err(AppError::new(ErrorCode::ZipUnsafe, "归档解压后超过 8GB"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::Instance;
    use std::io::Write;

    fn setup_game() -> (tempfile::TempDir, GamePaths, Instance) {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path().join("mc"));
        std::fs::create_dir_all(game.version_dir("1.20.4")).unwrap();
        std::fs::write(game.version_json("1.20.4"), br#"{"id":"1.20.4"}"#).unwrap();
        let game_dir = game.instances_dir().join("src");
        std::fs::create_dir_all(game_dir.join("saves/world")).unwrap();
        std::fs::create_dir_all(game_dir.join("mods")).unwrap();
        std::fs::create_dir_all(game_dir.join("logs")).unwrap();
        std::fs::write(game_dir.join("options.txt"), "lang:zh_CN").unwrap();
        std::fs::write(game_dir.join("saves/world/level.dat"), b"level").unwrap();
        std::fs::write(game_dir.join("mods/example.jar"), b"jar").unwrap();
        std::fs::write(game_dir.join("logs/latest.log"), b"secret log").unwrap();
        let instance = Instance {
            id: "src".into(),
            name: "生存".into(),
            version_id: "1.20.4".into(),
            game_dir,
            java_path: None,
            memory_mb: 4096,
            jvm_args: vec!["-Dfoo=bar".into()],
            game_args: vec![],
            created_at: util::now_rfc3339(),
            last_launched_at: None,
            account_id: None,
            resolution_width: None,
            resolution_height: None,
            mod_loader: None,
        };
        (dir, game, instance)
    }

    #[tokio::test]
    async fn exports_and_imports_roundtrip_without_logs() {
        let (dir, game, instance) = setup_game();
        let archive = dir.path().join("instance.zip");
        let summary = export_instance(&game, &instance, &archive, false)
            .await
            .unwrap();
        assert_eq!(summary.files, 3, "options + save + mod");
        assert!(archive.exists());
        let names = archive_names(&archive);
        assert!(!names.iter().any(|n| n.starts_with("logs/")));
        assert!(names.iter().any(|n| n == "saves/world/level.dat"));

        let preview = inspect_archive(&archive).await.unwrap();
        assert_eq!(preview.manifest.name, "生存");
        assert_eq!(preview.file_count, 3);

        let imported = import_instance(&game, &archive, Some("导入实例".into()))
            .await
            .unwrap();
        assert_eq!(imported.name, "导入实例");
        assert_eq!(imported.memory_mb, 4096);
        assert_eq!(
            std::fs::read_to_string(imported.game_dir.join("saves/world/level.dat")).unwrap(),
            "level"
        );
        assert!(!imported.game_dir.join("logs").exists());
    }

    fn archive_names(path: &Path) -> Vec<String> {
        let file = std::fs::File::open(path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().unwrap().to_string())
            .collect()
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[tokio::test]
    async fn rejects_traversal_and_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.zip");
        let manifest = serde_json::to_vec(&serde_json::json!({
            "format": EXPORT_FORMAT, "exportedAt": "x", "appVersion": "0",
            "name": "x", "versionId": "1.20.4", "memoryMb": 1024
        }))
        .unwrap();
        write_zip(&bad, &[(MANIFEST_NAME, &manifest), ("../evil.txt", b"x")]);
        assert!(inspect_archive(&bad).await.is_err());

        let dup = dir.path().join("dup.zip");
        // Distinct zip names that normalize to the same relative path.
        write_zip(
            &dup,
            &[
                (MANIFEST_NAME, &manifest),
                ("a.txt", b"1"),
                ("./a.txt", b"2"),
            ],
        );
        assert!(inspect_archive(&dup).await.is_err());
    }

    #[test]
    fn rejects_windows_reserved_and_dot_paths() {
        for bad in ["CON/x", "a/NUL.txt", "dir./file", "trail /x", "C:/x"] {
            assert!(validate_relative(bad).is_err(), "{bad}");
        }
        assert!(validate_relative("saves/world/level.dat").is_ok());
        assert!(validate_relative("./options.txt").is_ok());
    }

    #[tokio::test]
    async fn rejects_missing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("plain.zip");
        write_zip(&zip, &[("a.txt", b"x")]);
        let err = inspect_archive(&zip).await.unwrap_err();
        assert!(err.message.contains("sr-instance.json"));
    }

    #[tokio::test]
    async fn import_requires_installed_version() {
        let (dir, game, instance) = setup_game();
        let archive = dir.path().join("i.zip");
        export_instance(&game, &instance, &archive, false)
            .await
            .unwrap();
        let mut preview = inspect_archive(&archive).await.unwrap();
        preview.manifest.version_id = "9.9.9".into();
        // Rewrite manifest with a missing version.
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(MANIFEST_NAME, options).unwrap();
        zip.write_all(&serde_json::to_vec(&preview.manifest).unwrap())
            .unwrap();
        zip.finish().unwrap();
        let err = import_instance(&game, &archive, None).await.unwrap_err();
        assert!(err.message.contains("请先安装"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn export_skips_symlinks() {
        let (dir, game, instance) = setup_game();
        std::os::unix::fs::symlink("/etc/passwd", instance.game_dir.join("link")).unwrap();
        let archive = dir.path().join("sym.zip");
        let summary = export_instance(&game, &instance, &archive, false)
            .await
            .unwrap();
        assert_eq!(summary.skipped, 1);
        let names = archive_names(&archive);
        assert!(!names.iter().any(|n| n == "link"));
    }
}
