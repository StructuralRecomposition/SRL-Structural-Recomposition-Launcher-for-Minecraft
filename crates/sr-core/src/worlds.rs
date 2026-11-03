//! Read-only view of an instance's single-player worlds (`saves/`).
//!
//! The launcher deliberately does not edit or delete worlds here. It exposes
//! folder names, sizes, region counts and the last save time, and lets the user
//! open the folder in the system file manager.

use crate::error::Result;
use crate::instance::Instance;
use serde::Serialize;
use std::path::{Path, PathBuf};

const MAX_SCANNED_FILES: u64 = 50_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorldEntry {
    /// Directory name under `saves/` (the level id).
    pub folder: String,
    /// Display name shown by the launcher. Currently the folder name; the
    /// `level.dat` NBT is intentionally not parsed here.
    pub name: String,
    pub size_bytes: u64,
    pub level_dat_bytes: u64,
    pub region_files: u32,
    pub modified_at: Option<String>,
}

pub fn saves_dir(instance: &Instance) -> PathBuf {
    instance.game_dir.join("saves")
}

async fn dir_stats(root: &Path) -> (u64, u32) {
    let mut bytes = 0u64;
    let mut region_files = 0u32;
    let mut files = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut rd = match tokio::fs::read_dir(&dir).await {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        while let Ok(Some(entry)) = rd.next_entry().await {
            if files >= MAX_SCANNED_FILES {
                return (bytes, region_files);
            }
            let path = entry.path();
            let Ok(meta) = tokio::fs::symlink_metadata(&path).await else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                bytes = bytes.saturating_add(meta.len());
                files += 1;
                if path.extension().and_then(|e| e.to_str()) == Some("mca") {
                    region_files += 1;
                }
            }
        }
    }
    (bytes, region_files)
}

fn validate_folder(folder: &str) -> Result<()> {
    if folder.is_empty()
        || folder.len() > 255
        || folder.contains('/')
        || folder.contains('\\')
        || folder.contains('\0')
        || folder == "."
        || folder == ".."
        || folder.chars().any(|c| c.is_control())
    {
        return Err(crate::error::AppError::path_unsafe("存档目录名非法"));
    }
    Ok(())
}

pub async fn list(instance: &Instance) -> Result<Vec<WorldEntry>> {
    let dir = saves_dir(instance);
    let mut out = Vec::new();
    let mut rd = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    while let Some(entry) = rd.next_entry().await? {
        let Ok(file_type) = entry.file_type().await else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().into_owned();
        if folder.starts_with('.') || validate_folder(&folder).is_err() {
            continue;
        }
        let world_dir = entry.path();
        let level_dat = world_dir.join("level.dat");
        let Ok(level_meta) = tokio::fs::symlink_metadata(&level_dat).await else {
            continue;
        };
        if level_meta.file_type().is_symlink() || !level_meta.is_file() {
            continue;
        }
        let (size_bytes, region_files) = dir_stats(&world_dir).await;
        let modified = level_meta
            .modified()
            .or_else(|_| std::fs::metadata(&world_dir).and_then(|m| m.modified()))
            .ok()
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339());
        out.push(WorldEntry {
            folder: folder.clone(),
            name: folder,
            size_bytes,
            level_dat_bytes: level_meta.len(),
            region_files,
            modified_at: modified,
        });
    }
    out.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(out)
}

/// Resolve a validated world folder for the open-folder command.
pub fn world_dir(instance: &Instance, folder: &str) -> Result<PathBuf> {
    validate_folder(folder)?;
    Ok(saves_dir(instance).join(folder))
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[tokio::test]
    async fn lists_only_real_worlds_and_counts_regions() {
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        let saves = saves_dir(&inst);
        tokio::fs::create_dir_all(saves.join("test/region"))
            .await
            .unwrap();
        tokio::fs::write(saves.join("test/level.dat"), vec![0u8; 128])
            .await
            .unwrap();
        tokio::fs::write(saves.join("test/region/r.0.0.mca"), vec![0u8; 4096])
            .await
            .unwrap();
        tokio::fs::write(saves.join("test/region/r.0.1.mca"), vec![0u8; 2048])
            .await
            .unwrap();
        tokio::fs::create_dir_all(saves.join("not-a-world"))
            .await
            .unwrap();
        tokio::fs::write(saves.join("loose-file.txt"), b"x")
            .await
            .unwrap();

        let worlds = list(&inst).await.unwrap();
        assert_eq!(worlds.len(), 1);
        assert_eq!(worlds[0].folder, "test");
        assert_eq!(worlds[0].region_files, 2);
        assert_eq!(worlds[0].level_dat_bytes, 128);
        assert!(worlds[0].size_bytes >= 128 + 4096 + 2048);
        assert!(worlds[0].modified_at.is_some());

        assert!(world_dir(&inst, "test").unwrap().ends_with("saves/test"));
        assert!(world_dir(&inst, "../escape").is_err());
    }

    #[tokio::test]
    async fn missing_saves_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let inst = test_instance(dir.path());
        assert!(list(&inst).await.unwrap().is_empty());
    }

    #[test]
    fn rejects_path_like_folders() {
        assert!(validate_folder("ok world_1").is_ok());
        assert!(validate_folder("../evil").is_err());
        assert!(validate_folder("a/b").is_err());
        assert!(validate_folder("").is_err());
    }
}
