//! Local mod file management for an instance's `mods/` directory.
//! This only manages files; whether a mod actually loads depends on the
//! installed loader and is never claimed otherwise.

use crate::error::{AppError, ErrorCode, Result};
use crate::instance::Instance;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const MAX_MOD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModEntry {
    pub file_name: String,
    pub size: u64,
    pub enabled: bool,
    pub modified_at: Option<String>,
}

pub fn mods_dir(instance: &Instance) -> PathBuf {
    instance.game_dir.join("mods")
}

fn validate_file_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 255 {
        return Err(AppError::invalid_input("模组文件名长度非法"));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') || name.contains(':') {
        return Err(AppError::path_unsafe("模组文件名不能包含路径分隔符"));
    }
    if name == "." || name == ".." {
        return Err(AppError::path_unsafe("模组文件名非法"));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(AppError::invalid_input("模组文件名包含控制字符"));
    }
    if !(name.ends_with(".jar") || name.ends_with(".jar.disabled")) {
        return Err(AppError::invalid_input("只支持 .jar 或 .jar.disabled 文件"));
    }
    Ok(())
}

async fn entry_for(path: &Path) -> Result<ModEntry> {
    let meta = tokio::fs::symlink_metadata(path).await?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(AppError::path_unsafe("拒绝处理符号链接或非普通文件"));
    }
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::invalid_input("模组文件名缺失"))?;
    let modified_at = meta
        .modified()
        .ok()
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
    Ok(ModEntry {
        enabled: file_name.ends_with(".jar"),
        file_name,
        size: meta.len(),
        modified_at,
    })
}

pub async fn list(instance: &Instance) -> Result<Vec<ModEntry>> {
    let dir = mods_dir(instance);
    let mut out = Vec::new();
    let mut rd = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    while let Some(entry) = rd.next_entry().await? {
        let path = entry.path();
        let meta = match tokio::fs::symlink_metadata(&path).await {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.ends_with(".jar") || name.ends_with(".jar.disabled")) {
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

/// Enable or disable a mod by adding/removing the `.disabled` suffix.
pub async fn toggle(instance: &Instance, file_name: &str, enabled: bool) -> Result<ModEntry> {
    validate_file_name(file_name)?;
    let dir = mods_dir(instance);
    let source = dir.join(file_name);
    let meta = tokio::fs::symlink_metadata(&source).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("模组文件不存在")
        } else {
            e.into()
        }
    })?;
    if meta.file_type().is_symlink() {
        return Err(AppError::path_unsafe("拒绝处理符号链接"));
    }
    let currently_enabled = file_name.ends_with(".jar");
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
    let path = mods_dir(instance).join(file_name);
    let meta = tokio::fs::symlink_metadata(&path).await?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(AppError::path_unsafe("拒绝删除非常规文件"));
    }
    tokio::fs::remove_file(&path).await?;
    Ok(())
}

/// Write a mod from memory (for example a file chosen in the webview or a
/// verified download) into the instance's mods directory. Uses the same atomic
/// write path as other launcher files, so an interrupted write never leaves a
/// partial jar behind.
pub async fn import_bytes(
    instance: &Instance,
    file_name: &str,
    bytes: &[u8],
    overwrite: bool,
) -> Result<ModEntry> {
    validate_file_name(file_name)?;
    if !file_name.ends_with(".jar") {
        return Err(AppError::invalid_input("只能导入 .jar 模组"));
    }
    if bytes.len() as u64 > MAX_MOD_BYTES {
        return Err(AppError::invalid_input("模组文件超过 2GB"));
    }
    let dir = mods_dir(instance);
    tokio::fs::create_dir_all(&dir).await?;
    let dest = dir.join(file_name);
    if let Ok(meta) = tokio::fs::symlink_metadata(&dest).await {
        if meta.file_type().is_symlink() {
            return Err(AppError::path_unsafe("拒绝覆盖符号链接"));
        }
        if !overwrite {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                format!("模组已存在：{file_name}"),
            ));
        }
    }
    crate::util::atomic_write(&dest, bytes).await?;
    entry_for(&dest).await
}

/// Copy a local jar into the instance's mods directory. The target is written
/// to a temporary file first and renamed, so a failed copy cannot leave a
/// half-written mod.
pub async fn import(instance: &Instance, source: &Path, overwrite: bool) -> Result<ModEntry> {
    let source_meta = tokio::fs::symlink_metadata(source).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("源模组文件不存在")
        } else {
            e.into()
        }
    })?;
    if source_meta.file_type().is_symlink() || !source_meta.is_file() {
        return Err(AppError::path_unsafe(
            "源文件必须是普通文件，不能是符号链接",
        ));
    }
    if source_meta.len() > MAX_MOD_BYTES {
        return Err(AppError::invalid_input("模组文件超过 2GB"));
    }
    let name = source
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::invalid_input("源文件名缺失"))?;
    validate_file_name(&name)?;
    if source.extension().and_then(|e| e.to_str()) != Some("jar") {
        return Err(AppError::invalid_input("只能导入 .jar 模组"));
    }
    let dir = mods_dir(instance);
    tokio::fs::create_dir_all(&dir).await?;
    let dest = dir.join(&name);
    if tokio::fs::try_exists(&dest).await.unwrap_or(false) && !overwrite {
        return Err(AppError::new(
            ErrorCode::AlreadyExists,
            format!("模组已存在：{name}"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::Instance;
    use crate::util;

    fn instance(dir: &tempfile::TempDir) -> Instance {
        let game_dir = dir.path().join("inst");
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
    async fn lists_toggles_and_deletes_mods() {
        let dir = tempfile::tempdir().unwrap();
        let inst = instance(&dir);
        std::fs::create_dir_all(mods_dir(&inst)).unwrap();
        tokio::fs::write(mods_dir(&inst).join("a.jar"), b"aaa")
            .await
            .unwrap();
        tokio::fs::write(mods_dir(&inst).join("b.jar.disabled"), b"bb")
            .await
            .unwrap();
        tokio::fs::write(mods_dir(&inst).join("notes.txt"), b"x")
            .await
            .unwrap();

        let list = list(&inst).await.unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].enabled);
        assert_eq!(list[1].file_name, "b.jar.disabled");

        let disabled = toggle(&inst, "a.jar", false).await.unwrap();
        assert_eq!(disabled.file_name, "a.jar.disabled");
        assert!(!disabled.enabled);
        assert!(!mods_dir(&inst).join("a.jar").exists());

        let enabled = toggle(&inst, "a.jar.disabled", true).await.unwrap();
        assert_eq!(enabled.file_name, "a.jar");
        assert!(enabled.enabled);

        delete(&inst, "a.jar").await.unwrap();
        assert!(!mods_dir(&inst).join("a.jar").exists());
    }

    #[tokio::test]
    async fn rejects_traversal_and_unsupported_names() {
        let dir = tempfile::tempdir().unwrap();
        let inst = instance(&dir);
        std::fs::create_dir_all(mods_dir(&inst)).unwrap();
        for bad in ["../x.jar", "a/b.jar", "a\\b.jar", "x.txt", "..", ""] {
            assert!(validate_file_name(bad).is_err(), "{bad}");
        }
        assert!(toggle(&inst, "../x.jar", true).await.is_err());
        assert!(delete(&inst, "../x.jar").await.is_err());
    }

    #[tokio::test]
    async fn imports_jar_atomically_and_refuses_overwrite_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let inst = instance(&dir);
        let src = dir.path().join("cool.jar");
        tokio::fs::write(&src, b"mod-bytes").await.unwrap();
        let entry = import(&inst, &src, false).await.unwrap();
        assert_eq!(entry.file_name, "cool.jar");
        assert_eq!(entry.size, 9);
        assert!(entry.enabled);

        let err = import(&inst, &src, false).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::AlreadyExists);
        let replaced = import(&inst, &src, true).await.unwrap();
        assert_eq!(replaced.size, 9);

        let txt = dir.path().join("notamod.txt");
        tokio::fs::write(&txt, b"x").await.unwrap();
        assert!(import(&inst, &txt, false).await.is_err());
    }

    #[tokio::test]
    async fn imports_bytes_with_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        let inst = instance(&dir);
        let entry = import_bytes(&inst, "from-picker.jar", b"bytes", false)
            .await
            .unwrap();
        assert_eq!(entry.file_name, "from-picker.jar");
        assert_eq!(entry.size, 5);
        assert!(entry.enabled);

        let err = import_bytes(&inst, "from-picker.jar", b"new", false)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::AlreadyExists);
        let replaced = import_bytes(&inst, "from-picker.jar", b"new", true)
            .await
            .unwrap();
        assert_eq!(replaced.size, 3);

        assert!(import_bytes(&inst, "bad.txt", b"x", true).await.is_err());
        assert!(import_bytes(&inst, "../escape.jar", b"x", true)
            .await
            .is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn refuses_symlinked_mod() {
        let dir = tempfile::tempdir().unwrap();
        let inst = instance(&dir);
        std::fs::create_dir_all(mods_dir(&inst)).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", mods_dir(&inst).join("evil.jar")).unwrap();
        let list = list(&inst).await.unwrap();
        assert!(list.is_empty());
        assert!(toggle(&inst, "evil.jar", false).await.is_err());
    }
}
