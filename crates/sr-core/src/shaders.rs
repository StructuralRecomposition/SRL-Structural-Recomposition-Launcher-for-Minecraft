//! Shader pack file management for an instance's `shaderpacks/` directory.
//!
//! The launcher only manages files. Whether shaders render depends on the
//! installed loader/mod: Iris or OptiFine is required for vanilla Minecraft.
//! This module never claims that a shader pack is "active" beyond its filename
//! state (`.zip` vs `.zip.disabled`).

use crate::error::{AppError, ErrorCode, Result};
use crate::instance::Instance;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const MAX_SHADER_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShaderEntry {
    pub file_name: String,
    pub size: u64,
    pub enabled: bool,
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShaderSupport {
    /// Enabled mod filename that looks like Iris, if any.
    pub iris: Option<String>,
    /// Enabled mod filename that looks like OptiFine, if any.
    pub optifine: Option<String>,
}

impl ShaderSupport {
    pub fn supported(&self) -> bool {
        self.iris.is_some() || self.optifine.is_some()
    }
}

pub fn shaderpacks_dir(instance: &Instance) -> PathBuf {
    instance.game_dir.join("shaderpacks")
}

fn validate_file_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 255 {
        return Err(AppError::invalid_input("光影包文件名长度非法"));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') || name.contains(':') {
        return Err(AppError::path_unsafe("光影包文件名不能包含路径分隔符"));
    }
    if name == "." || name == ".." {
        return Err(AppError::path_unsafe("光影包文件名非法"));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(AppError::invalid_input("光影包文件名包含控制字符"));
    }
    if !(name.ends_with(".zip") || name.ends_with(".zip.disabled")) {
        return Err(AppError::invalid_input(
            "只支持 .zip 或 .zip.disabled 光影包",
        ));
    }
    Ok(())
}

async fn entry_for(path: &Path) -> Result<ShaderEntry> {
    let meta = tokio::fs::symlink_metadata(path).await?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(AppError::path_unsafe("拒绝处理符号链接或非普通文件"));
    }
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::invalid_input("光影包文件名缺失"))?;
    let modified_at = meta
        .modified()
        .ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    Ok(ShaderEntry {
        enabled: file_name.ends_with(".zip"),
        file_name,
        size: meta.len(),
        modified_at,
    })
}

pub async fn list(instance: &Instance) -> Result<Vec<ShaderEntry>> {
    let dir = shaderpacks_dir(instance);
    let mut out = Vec::new();
    let mut rd = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    while let Some(entry) = rd.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.ends_with(".zip") || name.ends_with(".zip.disabled")) {
            continue;
        }
        let path = entry.path();
        let meta = match tokio::fs::symlink_metadata(&path).await {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }
        if let Ok(item) = entry_for(&path).await {
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

/// Enable or disable a shader pack by adding/removing the `.disabled` suffix.
pub async fn toggle(instance: &Instance, file_name: &str, enabled: bool) -> Result<ShaderEntry> {
    validate_file_name(file_name)?;
    let dir = shaderpacks_dir(instance);
    let source = dir.join(file_name);
    let meta = tokio::fs::symlink_metadata(&source).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("光影包不存在")
        } else {
            e.into()
        }
    })?;
    if meta.file_type().is_symlink() {
        return Err(AppError::path_unsafe("拒绝处理符号链接"));
    }
    let currently_enabled = file_name.ends_with(".zip");
    if currently_enabled == enabled {
        return entry_for(&source).await;
    }
    let target_name = if enabled {
        file_name.trim_end_matches(".disabled").to_string()
    } else {
        format!("{file_name}.disabled")
    };
    validate_file_name(&target_name)?;
    let target = dir.join(&target_name);
    if tokio::fs::try_exists(&target).await.unwrap_or(false) {
        return Err(AppError::new(
            ErrorCode::AlreadyExists,
            format!("目标文件已存在：{target_name}"),
        ));
    }
    tokio::fs::rename(&source, &target).await?;
    entry_for(&target).await
}

pub async fn delete(instance: &Instance, file_name: &str) -> Result<()> {
    validate_file_name(file_name)?;
    let path = shaderpacks_dir(instance).join(file_name);
    let meta = tokio::fs::symlink_metadata(&path).await?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(AppError::path_unsafe("拒绝删除非常规文件"));
    }
    tokio::fs::remove_file(&path).await?;
    Ok(())
}

pub async fn import(instance: &Instance, source: &Path, overwrite: bool) -> Result<ShaderEntry> {
    let source_meta = tokio::fs::symlink_metadata(source).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("源光影包不存在")
        } else {
            e.into()
        }
    })?;
    if source_meta.file_type().is_symlink() || !source_meta.is_file() {
        return Err(AppError::path_unsafe(
            "源文件必须是普通文件，不能是符号链接",
        ));
    }
    if source_meta.len() > MAX_SHADER_BYTES {
        return Err(AppError::invalid_input("光影包超过 2GB"));
    }
    let name = source
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::invalid_input("源文件名缺失"))?;
    validate_file_name(&name)?;
    if source.extension().and_then(|e| e.to_str()) != Some("zip") {
        return Err(AppError::invalid_input("只能导入 .zip 光影包"));
    }
    let dir = shaderpacks_dir(instance);
    tokio::fs::create_dir_all(&dir).await?;
    let dest = dir.join(&name);
    if tokio::fs::try_exists(&dest).await.unwrap_or(false) && !overwrite {
        return Err(AppError::new(
            ErrorCode::AlreadyExists,
            format!("光影包已存在：{name}"),
        ));
    }
    let tmp = dir.join(format!(
        ".{name}.tmp-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    if let Err(e) = tokio::fs::copy(source, &tmp).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e.into());
    }
    #[cfg(windows)]
    {
        if tokio::fs::try_exists(&dest).await.unwrap_or(false) {
            tokio::fs::remove_file(&dest).await?;
        }
    }
    if let Err(e) = tokio::fs::rename(&tmp, &dest).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e.into());
    }
    entry_for(&dest).await
}

pub async fn import_bytes(
    instance: &Instance,
    file_name: &str,
    bytes: &[u8],
    overwrite: bool,
) -> Result<ShaderEntry> {
    validate_file_name(file_name)?;
    if !file_name.ends_with(".zip") {
        return Err(AppError::invalid_input("只能导入 .zip 光影包"));
    }
    if bytes.len() as u64 > MAX_SHADER_BYTES {
        return Err(AppError::invalid_input("光影包超过 2GB"));
    }
    let dir = shaderpacks_dir(instance);
    tokio::fs::create_dir_all(&dir).await?;
    let dest = dir.join(file_name);
    if let Ok(meta) = tokio::fs::symlink_metadata(&dest).await {
        if meta.file_type().is_symlink() {
            return Err(AppError::path_unsafe("拒绝覆盖符号链接"));
        }
        if !overwrite {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                format!("光影包已存在：{file_name}"),
            ));
        }
    }
    crate::util::atomic_write(&dest, bytes).await?;
    entry_for(&dest).await
}

/// Detect enabled shader-capable mods by filename. This is a heuristic and is
/// labelled as such in the UI; it never modifies mod files.
pub async fn support(instance: &Instance) -> Result<ShaderSupport> {
    let mods = crate::mods::list(instance).await?;
    let mut iris = None;
    let mut optifine = None;
    for entry in mods.into_iter().filter(|m| m.enabled) {
        let lower = entry.file_name.to_lowercase();
        if iris.is_none() && lower.contains("iris") {
            iris = Some(entry.file_name.clone());
        }
        if optifine.is_none() && lower.contains("optifine") {
            optifine = Some(entry.file_name);
        }
    }
    Ok(ShaderSupport { iris, optifine })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util;

    fn instance(dir: &tempfile::TempDir) -> Instance {
        let game_dir = dir.path().join("instances/i");
        std::fs::create_dir_all(&game_dir).unwrap();
        Instance {
            id: "i".into(),
            name: "test".into(),
            version_id: "1.20.4".into(),
            game_dir,
            java_path: None,
            memory_mb: 2048,
            jvm_args: vec![],
            game_args: vec![],
            created_at: util::now_rfc3339(),
            last_launched_at: None,
            account_id: None,
            resolution_width: None,
            resolution_height: None,
            mod_loader: None,
        }
    }

    #[tokio::test]
    async fn lists_toggles_and_deletes_shaderpacks() {
        let dir = tempfile::tempdir().unwrap();
        let inst = instance(&dir);
        std::fs::create_dir_all(shaderpacks_dir(&inst)).unwrap();
        tokio::fs::write(shaderpacks_dir(&inst).join("BSL.zip"), b"aaa")
            .await
            .unwrap();
        tokio::fs::write(
            shaderpacks_dir(&inst).join("Complementary.zip.disabled"),
            b"bb",
        )
        .await
        .unwrap();
        tokio::fs::write(shaderpacks_dir(&inst).join("readme.txt"), b"x")
            .await
            .unwrap();

        let list = list(&inst).await.unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].enabled);

        let disabled = toggle(&inst, "BSL.zip", false).await.unwrap();
        assert_eq!(disabled.file_name, "BSL.zip.disabled");
        assert!(!disabled.enabled);

        let enabled = toggle(&inst, "BSL.zip.disabled", true).await.unwrap();
        assert!(enabled.enabled);
        delete(&inst, "BSL.zip").await.unwrap();
        assert!(!shaderpacks_dir(&inst).join("BSL.zip").exists());
    }

    #[tokio::test]
    async fn imports_and_rejects_unsafe_names() {
        let dir = tempfile::tempdir().unwrap();
        let inst = instance(&dir);
        let src = dir.path().join("shader.zip");
        tokio::fs::write(&src, b"zip-bytes").await.unwrap();
        let entry = import(&inst, &src, false).await.unwrap();
        assert_eq!(entry.file_name, "shader.zip");
        assert_eq!(entry.size, 9);

        let err = import(&inst, &src, false).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::AlreadyExists);
        import(&inst, &src, true).await.unwrap();

        let txt = dir.path().join("shader.txt");
        tokio::fs::write(&txt, b"x").await.unwrap();
        assert!(import(&inst, &txt, false).await.is_err());

        assert!(import_bytes(&inst, "../escape.zip", b"x", true)
            .await
            .is_err());
        assert!(import_bytes(&inst, "bad.jar", b"x", true).await.is_err());
    }

    #[tokio::test]
    async fn detects_shader_support_from_enabled_mods() {
        let dir = tempfile::tempdir().unwrap();
        let inst = instance(&dir);
        std::fs::create_dir_all(crate::mods::mods_dir(&inst)).unwrap();
        tokio::fs::write(crate::mods::mods_dir(&inst).join("iris-1.7.0.jar"), b"x")
            .await
            .unwrap();
        tokio::fs::write(
            crate::mods::mods_dir(&inst).join("sodium.jar.disabled"),
            b"x",
        )
        .await
        .unwrap();
        let support = support(&inst).await.unwrap();
        assert_eq!(support.iris.as_deref(), Some("iris-1.7.0.jar"));
        assert!(support.optifine.is_none());
        assert!(support.supported());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn refuses_symlinked_shaderpack() {
        let dir = tempfile::tempdir().unwrap();
        let inst = instance(&dir);
        std::fs::create_dir_all(shaderpacks_dir(&inst)).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", shaderpacks_dir(&inst).join("evil.zip")).unwrap();
        assert!(list(&inst).await.unwrap().is_empty());
        assert!(toggle(&inst, "evil.zip", false).await.is_err());
    }
}
