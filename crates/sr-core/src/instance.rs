use crate::error::{AppError, Result};
use crate::paths::GamePaths;
use crate::util;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModLoaderInfo {
    pub kind: String,
    pub version: String,
    /// Only true after a verified installation; never optimistically set.
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub version_id: String,
    pub game_dir: PathBuf,
    #[serde(default)]
    pub java_path: Option<PathBuf>,
    #[serde(default = "default_memory")]
    pub memory_mb: u32,
    #[serde(default)]
    pub jvm_args: Vec<String>,
    #[serde(default)]
    pub game_args: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub last_launched_at: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub resolution_width: Option<u32>,
    #[serde(default)]
    pub resolution_height: Option<u32>,
    #[serde(default)]
    pub mod_loader: Option<ModLoaderInfo>,
}

fn default_memory() -> u32 {
    4096
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstanceRequest {
    pub name: String,
    pub version_id: String,
    #[serde(default)]
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub mod_loader: Option<ModLoaderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstanceRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub java_path: Option<String>,
    #[serde(default)]
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub jvm_args: Option<Vec<String>>,
    #[serde(default)]
    pub game_args: Option<Vec<String>>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub resolution_width: Option<u32>,
    #[serde(default)]
    pub resolution_height: Option<u32>,
}

#[derive(Clone)]
pub struct InstanceManager {
    game: GamePaths,
}

impl InstanceManager {
    pub fn new(game: GamePaths) -> Self {
        Self { game }
    }

    pub fn game(&self) -> &GamePaths {
        &self.game
    }

    pub fn instance_dir(&self, id: &str) -> PathBuf {
        self.game.instances_dir().join(id)
    }

    pub fn config_path(&self, id: &str) -> PathBuf {
        self.instance_dir(id).join("instance.json")
    }

    pub async fn list(&self) -> Result<Vec<Instance>> {
        let dir = self.game.instances_dir();
        let mut out = Vec::new();
        let mut rd = match tokio::fs::read_dir(&dir).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = rd.next_entry().await? {
            if !entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let path = entry.path().join("instance.json");
            if let Ok(Some(instance)) = crate::util::read_json::<Instance>(&path).await {
                if util::safe_component(&instance.id, "") == instance.id && !instance.id.is_empty()
                {
                    out.push(instance);
                } else {
                    tracing::warn!(path = %path.display(), "ignoring instance with unsafe id");
                }
            }
        }
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(out)
    }

    pub async fn get(&self, id: &str) -> Result<Instance> {
        if util::safe_component(id, "") != id {
            return Err(AppError::invalid_input("实例 ID 非法"));
        }
        let path = self.config_path(id);
        let mut instance = crate::util::read_json::<Instance>(&path)
            .await?
            .ok_or_else(|| AppError::not_found(format!("实例 {id} 不存在")))?;
        if instance.mod_loader.is_none() {
            instance.mod_loader = Self::detect_loader(&instance.version_id);
        }
        Ok(instance)
    }

    pub async fn create(&self, req: CreateInstanceRequest) -> Result<Instance> {
        let name = req.name.trim();
        if name.is_empty() {
            return Err(AppError::invalid_input("实例名称不能为空"));
        }
        if name.chars().count() > 64 {
            return Err(AppError::invalid_input("实例名称不能超过 64 个字符"));
        }
        if util::safe_component(&req.version_id, "") != req.version_id {
            return Err(AppError::invalid_input("版本 ID 非法"));
        }
        if !tokio::fs::try_exists(self.game.version_json(&req.version_id))
            .await
            .unwrap_or(false)
        {
            return Err(AppError::not_found(format!(
                "版本 {} 尚未安装，无法创建实例",
                req.version_id
            )));
        }
        let memory = req.memory_mb.unwrap_or(default_memory());
        validate_memory(memory)?;

        let id = util::short_id();
        let dir = self.instance_dir(&id);
        util::ensure_dir(&dir).await?;
        let detected_loader = Self::detect_loader(&req.version_id);
        let instance = Instance {
            id,
            name: name.to_string(),
            version_id: req.version_id,
            game_dir: dir.clone(),
            java_path: None,
            memory_mb: memory,
            jvm_args: Vec::new(),
            game_args: Vec::new(),
            created_at: util::now_rfc3339(),
            last_launched_at: None,
            account_id: None,
            resolution_width: None,
            resolution_height: None,
            mod_loader: req.mod_loader.clone().or(detected_loader),
        };
        self.save(&instance).await?;
        Ok(instance)
    }

    fn detect_loader(version_id: &str) -> Option<ModLoaderInfo> {
        if let Some(rest) = version_id.strip_prefix("fabric-loader-") {
            let version = rest.split('-').next().unwrap_or(rest).to_string();
            return Some(ModLoaderInfo {
                kind: "Fabric".into(),
                version,
                installed: true,
            });
        }
        if let Some(rest) = version_id.strip_prefix("quilt-loader-") {
            let version = rest.split('-').next().unwrap_or(rest).to_string();
            return Some(ModLoaderInfo {
                kind: "Quilt".into(),
                version,
                installed: true,
            });
        }
        if let Some((_, loader)) = version_id.rsplit_once("-forge-") {
            return Some(ModLoaderInfo {
                kind: "Forge".into(),
                version: loader.to_string(),
                installed: true,
            });
        }
        if let Some(rest) = version_id.strip_prefix("neoforge-") {
            return Some(ModLoaderInfo {
                kind: "NeoForge".into(),
                version: rest.to_string(),
                installed: true,
            });
        }
        None
    }

    pub async fn save(&self, instance: &Instance) -> Result<()> {
        if util::safe_component(&instance.id, "") != instance.id || instance.id.is_empty() {
            return Err(AppError::invalid_input("实例 ID 非法"));
        }
        if instance.name.trim().is_empty() {
            return Err(AppError::invalid_input("实例名称不能为空"));
        }
        validate_memory(instance.memory_mb)?;
        let path = self.config_path(&instance.id);
        util::write_json(&path, instance).await
    }

    /// Point an existing instance at another installed version (for example
    /// after installing Fabric/Forge for it). Mods and saves stay untouched.
    pub async fn set_version(&self, id: &str, version_id: &str) -> Result<Instance> {
        if util::safe_component(version_id, "") != version_id || version_id.is_empty() {
            return Err(AppError::invalid_input("版本 ID 非法"));
        }
        if !tokio::fs::try_exists(self.game.version_json(version_id))
            .await
            .unwrap_or(false)
        {
            return Err(AppError::not_found(format!("版本 {version_id} 尚未安装")));
        }
        let mut instance = self.get(id).await?;
        instance.version_id = version_id.to_string();
        instance.mod_loader = Self::detect_loader(version_id);
        self.save(&instance).await?;
        Ok(instance)
    }

    pub async fn update(&self, id: &str, req: UpdateInstanceRequest) -> Result<Instance> {
        let mut instance = self.get(id).await?;
        if let Some(name) = req.name {
            let trimmed = name.trim().to_string();
            if trimmed.is_empty() {
                return Err(AppError::invalid_input("实例名称不能为空"));
            }
            instance.name = trimmed;
        }
        if let Some(java) = req.java_path {
            instance.java_path = if java.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(java))
            };
        }
        if let Some(memory) = req.memory_mb {
            validate_memory(memory)?;
            instance.memory_mb = memory;
        }
        if let Some(args) = req.jvm_args {
            instance.jvm_args = args;
        }
        if let Some(args) = req.game_args {
            instance.game_args = args;
        }
        if let Some(account) = req.account_id {
            instance.account_id = if account.trim().is_empty() {
                None
            } else {
                Some(account)
            };
        }
        if req.resolution_width.is_some() || req.resolution_height.is_some() {
            let width = req.resolution_width.unwrap_or(0);
            let height = req.resolution_height.unwrap_or(0);
            if width == 0 && height == 0 {
                instance.resolution_width = None;
                instance.resolution_height = None;
            } else {
                validate_resolution(width, height)?;
                instance.resolution_width = Some(width);
                instance.resolution_height = Some(height);
            }
        }
        self.save(&instance).await?;
        Ok(instance)
    }

    /// Delete an instance. `delete_files` must be explicitly true to remove the
    /// instance directory (save data, mods, configs). Otherwise only the
    /// launcher-side instance.json is removed and the directory is preserved.
    pub async fn delete(&self, id: &str, delete_files: bool) -> Result<()> {
        let instance = self.get(id).await?;
        if delete_files {
            let dir = self.instance_dir(id);
            tokio::fs::remove_dir_all(&dir).await.map_err(|e| {
                AppError::new(crate::error::ErrorCode::Io, "删除实例目录失败")
                    .with_detail(e.to_string())
            })?;
        } else {
            tokio::fs::remove_file(self.config_path(&instance.id)).await?;
        }
        Ok(())
    }

    pub async fn duplicate(&self, id: &str, new_name: Option<String>) -> Result<Instance> {
        let source = self.get(id).await?;
        let new_id = util::short_id();
        let dir = self.instance_dir(&new_id);
        copy_dir_recursive(&source.game_dir, &dir).await?;
        let mut copy = source.clone();
        copy.id = new_id;
        copy.name = new_name.unwrap_or_else(|| format!("{} 副本", source.name));
        copy.game_dir = dir;
        copy.created_at = util::now_rfc3339();
        copy.last_launched_at = None;
        self.save(&copy).await?;
        Ok(copy)
    }

    pub async fn touch_launched(&self, id: &str) -> Result<()> {
        let mut instance = self.get(id).await?;
        instance.last_launched_at = Some(util::now_rfc3339());
        self.save(&instance).await
    }
}

fn validate_resolution(width: u32, height: u32) -> Result<()> {
    if !(320..=7680).contains(&width) || !(240..=4320).contains(&height) {
        return Err(AppError::invalid_input(
            "分辨率必须在 320x240 到 7680x4320 之间",
        ));
    }
    Ok(())
}

fn validate_memory(memory_mb: u32) -> Result<()> {
    if !(512..=65_536).contains(&memory_mb) {
        return Err(AppError::invalid_input("内存必须在 512MB 到 65536MB 之间"));
    }
    Ok(())
}

async fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((from, to)) = stack.pop() {
        util::ensure_dir(&to).await?;
        let mut rd = match tokio::fs::read_dir(&from).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = rd.next_entry().await? {
            let ft = entry.file_type().await?;
            let target = to.join(entry.file_name());
            if ft.is_dir() {
                stack.push((entry.path(), target));
            } else if ft.is_file() {
                tokio::fs::copy(entry.path(), &target).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup() -> (tempfile::TempDir, InstanceManager) {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        game.ensure_layout().await.unwrap();
        tokio::fs::create_dir_all(game.version_dir("1.20.4"))
            .await
            .unwrap();
        tokio::fs::write(game.version_json("1.20.4"), br#"{"id":"1.20.4"}"#)
            .await
            .unwrap();
        (dir, InstanceManager::new(game))
    }

    #[test]
    fn detects_loader_from_version_id() {
        assert_eq!(
            InstanceManager::detect_loader("fabric-loader-0.19.5-1.21.1")
                .unwrap()
                .kind,
            "Fabric"
        );
        assert_eq!(
            InstanceManager::detect_loader("quilt-loader-0.20.0-beta.9-1.21.1")
                .unwrap()
                .kind,
            "Quilt"
        );
        assert_eq!(
            InstanceManager::detect_loader("1.21.1-forge-52.1.20")
                .unwrap()
                .kind,
            "Forge"
        );
        assert_eq!(
            InstanceManager::detect_loader("neoforge-21.1.264")
                .unwrap()
                .kind,
            "NeoForge"
        );
        assert!(InstanceManager::detect_loader("1.21.1").is_none());
    }

    #[tokio::test]
    async fn create_list_update_delete() {
        let (_dir, mgr) = setup().await;
        let inst = mgr
            .create(CreateInstanceRequest {
                name: "生存".into(),
                version_id: "1.20.4".into(),
                memory_mb: Some(2048),
                mod_loader: None,
            })
            .await
            .unwrap();
        assert_eq!(inst.name, "生存");
        assert!(inst.game_dir.is_dir());
        let list = mgr.list().await.unwrap();
        assert_eq!(list.len(), 1);

        let updated = mgr
            .update(
                &inst.id,
                UpdateInstanceRequest {
                    name: Some("创造".into()),
                    memory_mb: Some(4096),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "创造");
        assert_eq!(updated.memory_mb, 4096);

        mgr.delete(&inst.id, false).await.unwrap();
        assert!(mgr.list().await.unwrap().is_empty());
        assert!(inst.game_dir.is_dir(), "files must be preserved");
    }

    #[tokio::test]
    async fn updates_account_and_resolution() {
        let (_dir, mgr) = setup().await;
        let inst = mgr
            .create(CreateInstanceRequest {
                name: "a".into(),
                version_id: "1.20.4".into(),
                memory_mb: None,
                mod_loader: None,
            })
            .await
            .unwrap();
        let updated = mgr
            .update(
                &inst.id,
                UpdateInstanceRequest {
                    account_id: Some("acc1".into()),
                    resolution_width: Some(1920),
                    resolution_height: Some(1080),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.account_id.as_deref(), Some("acc1"));
        assert_eq!(updated.resolution_width, Some(1920));

        let cleared = mgr
            .update(
                &inst.id,
                UpdateInstanceRequest {
                    account_id: Some(String::new()),
                    resolution_width: Some(0),
                    resolution_height: Some(0),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(cleared.account_id.is_none());
        assert!(cleared.resolution_width.is_none());

        let err = mgr
            .update(
                &inst.id,
                UpdateInstanceRequest {
                    resolution_width: Some(100),
                    resolution_height: Some(100),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(err.message.contains("分辨率"));
    }

    #[tokio::test]
    async fn rejects_uninstalled_version_and_bad_memory() {
        let (_dir, mgr) = setup().await;
        let err = mgr
            .create(CreateInstanceRequest {
                name: "x".into(),
                version_id: "9.9.9".into(),
                memory_mb: None,
                mod_loader: None,
            })
            .await
            .unwrap_err();
        assert!(err.message.contains("尚未安装"));
        let err = mgr
            .create(CreateInstanceRequest {
                name: "x".into(),
                version_id: "1.20.4".into(),
                memory_mb: Some(128),
                mod_loader: None,
            })
            .await
            .unwrap_err();
        assert!(err.message.contains("内存"));
    }

    #[tokio::test]
    async fn duplicate_copies_files_with_new_identity() {
        let (_dir, mgr) = setup().await;
        let inst = mgr
            .create(CreateInstanceRequest {
                name: "a".into(),
                version_id: "1.20.4".into(),
                memory_mb: None,
                mod_loader: None,
            })
            .await
            .unwrap();
        tokio::fs::write(inst.game_dir.join("options.txt"), b"lang:zh_CN")
            .await
            .unwrap();
        let copy = mgr.duplicate(&inst.id, None).await.unwrap();
        assert_ne!(copy.id, inst.id);
        assert_eq!(copy.name, "a 副本");
        assert_eq!(
            tokio::fs::read_to_string(copy.game_dir.join("options.txt"))
                .await
                .unwrap(),
            "lang:zh_CN"
        );
    }

    #[tokio::test]
    async fn delete_with_files_removes_directory() {
        let (_dir, mgr) = setup().await;
        let inst = mgr
            .create(CreateInstanceRequest {
                name: "b".into(),
                version_id: "1.20.4".into(),
                memory_mb: None,
                mod_loader: None,
            })
            .await
            .unwrap();
        mgr.delete(&inst.id, true).await.unwrap();
        assert!(!inst.game_dir.exists());
    }
}
