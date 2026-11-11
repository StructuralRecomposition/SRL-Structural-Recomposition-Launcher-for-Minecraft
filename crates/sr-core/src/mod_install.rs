//! Modrinth mod installation with dependency resolution, staging, verification,
//! backups and rollback.
//!
//! The model may request a plan, but it can never install anything: the UI must
//! show the plan and the user must confirm it. Downloads only accept URLs from
//! the official Modrinth CDN and require a SHA-1.

use crate::download::{self, DownloadSpec, TaskHandle, TaskStatus};
use crate::error::{AppError, ErrorCode, Result};
use crate::instance::Instance;
use crate::modrinth::{ModrinthService, ResourceVersion};
use crate::paths::AppPaths;
use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const MAX_ENTRIES: usize = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModPlanEntry {
    pub project_id: String,
    pub title: String,
    pub version_id: String,
    pub version_number: String,
    pub file_name: String,
    pub url: String,
    pub size: u64,
    pub sha1: String,
    pub required_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModInstallPlan {
    pub id: String,
    pub instance_id: String,
    pub instance_name: String,
    pub created_at: String,
    pub root_project: String,
    pub game_version: String,
    pub loader: String,
    pub entries: Vec<ModPlanEntry>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModInstallResult {
    pub token: String,
    pub backup_dir: String,
    pub installed: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    token: String,
    instance_id: String,
    created_at: String,
    installed: Vec<String>,
    backups: Vec<BackupFile>,
    rolled_back: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupFile {
    file_name: String,
    backup_name: String,
}

fn plans_dir(paths: &AppPaths) -> PathBuf {
    paths.base().join("assistant").join("mod-plans")
}

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn validate_file_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 255
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name == "."
        || name == ".."
        || name.ends_with(".disabled")
    {
        return Err(AppError::path_unsafe("模组文件名非法"));
    }
    Ok(())
}

fn primary_file(version: &ResourceVersion) -> Result<&crate::modrinth::ResourceFile> {
    version
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| AppError::not_found("该版本没有可下载文件"))
}

fn compatible(version: &ResourceVersion, game_version: &str, loader: &str) -> bool {
    let version_ok =
        version.game_versions.is_empty() || version.game_versions.iter().any(|v| v == game_version);
    let loader_ok = version.loaders.is_empty()
        || version
            .loaders
            .iter()
            .any(|l| l.eq_ignore_ascii_case(loader));
    version_ok && loader_ok
}

async fn pick_version(
    service: &ModrinthService,
    project_id: &str,
    version_id: Option<&str>,
    game_version: &str,
    loader: &str,
) -> Result<ResourceVersion> {
    if let Some(id) = version_id {
        let version = service.version(id, project_id).await?;
        if !compatible(&version, game_version, loader) {
            return Err(AppError::invalid_input(format!(
                "{} {} 不支持 Minecraft {} / {}",
                version.name, version.version_number, game_version, loader
            )));
        }
        return Ok(version);
    }
    let versions = service
        .versions(project_id, Some(game_version), Some(loader))
        .await?;
    versions
        .into_iter()
        .find(|v| compatible(v, game_version, loader))
        .ok_or_else(|| {
            AppError::not_found(format!(
                "没有找到支持 Minecraft {game_version} / {loader} 的版本"
            ))
        })
}

#[allow(clippy::too_many_arguments)]
async fn resolve(
    service: &ModrinthService,
    project_id: &str,
    version_id: Option<&str>,
    game_version: &str,
    loader: &str,
    required_by: Vec<String>,
    visited: &mut HashSet<String>,
    entries: &mut Vec<ModPlanEntry>,
    warnings: &mut Vec<String>,
    depth: usize,
) -> Result<()> {
    if depth > 8 || entries.len() >= MAX_ENTRIES {
        return Err(AppError::invalid_input("依赖层级或数量超过限制"));
    }
    if !visited.insert(project_id.to_string()) {
        return Ok(());
    }
    let version = pick_version(service, project_id, version_id, game_version, loader).await?;
    let file = primary_file(&version)?;
    let sha1 = file
        .sha1
        .clone()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| AppError::invalid_input("依赖文件缺少 SHA-1，已拒绝安装"))?;
    validate_file_name(&file.filename)?;
    download::ensure_trusted_url(&file.url, false)?;
    let project = service.project(project_id).await?;
    let project_title = project.title.clone();
    entries.push(ModPlanEntry {
        project_id: project_id.to_string(),
        title: project_title.clone(),
        version_id: version.id.clone(),
        version_number: version.version_number.clone(),
        file_name: file.filename.clone(),
        url: file.url.clone(),
        size: file.size,
        sha1,
        required_by: required_by.clone(),
    });

    let mut next = Vec::new();
    for dep in &version.dependencies {
        if dep.dependency_type != "required" {
            continue;
        }
        let Some(dep_project) = dep.project_id.as_deref() else {
            warnings.push(format!(
                "{} 有一个必需依赖缺少 project_id，已跳过，请人工确认",
                project_title
            ));
            continue;
        };
        next.push((dep_project.to_string(), dep.version_id.clone()));
    }
    for (dep_project, dep_version) in next {
        Box::pin(resolve(
            service,
            &dep_project,
            dep_version.as_deref(),
            game_version,
            loader,
            vec![project_title.clone()],
            visited,
            entries,
            warnings,
            depth + 1,
        ))
        .await?;
    }
    Ok(())
}

pub async fn plan(
    service: &ModrinthService,
    instance: &Instance,
    project_id: &str,
    version_id: Option<&str>,
    game_version: &str,
    loader: &str,
) -> Result<ModInstallPlan> {
    if loader.is_empty() {
        return Err(AppError::invalid_input("实例没有可用的模组加载器"));
    }
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    resolve(
        service,
        project_id,
        version_id,
        game_version,
        loader,
        Vec::new(),
        &mut HashSet::new(),
        &mut entries,
        &mut warnings,
        0,
    )
    .await?;
    if entries.is_empty() {
        return Err(AppError::not_found("没有解析到可安装文件"));
    }
    warnings.push(
        "如果 mods/ 中已经存在同名文件，安装会先备份再覆盖；不同版本的同名模组可能仍需要手动清理。"
            .into(),
    );
    Ok(ModInstallPlan {
        id: util::short_id(),
        instance_id: instance.id.clone(),
        instance_name: instance.name.clone(),
        created_at: util::now_rfc3339(),
        root_project: project_id.to_string(),
        game_version: game_version.to_string(),
        loader: loader.to_string(),
        entries,
        warnings,
        notes: vec![
            "下载先写入临时目录并校验 SHA-1；安装前备份同名文件；失败会清理并回滚。".into(),
        ],
    })
}

/// Build one combined plan for several root projects (a private modpack).
pub async fn plan_many(
    service: &ModrinthService,
    instance: &Instance,
    project_ids: &[String],
    game_version: &str,
    loader: &str,
) -> Result<ModInstallPlan> {
    if loader.is_empty() {
        return Err(AppError::invalid_input("实例没有可用的模组加载器"));
    }
    if project_ids.is_empty() || project_ids.len() > 20 {
        return Err(AppError::invalid_input("整合包模组数量必须在 1 到 20 之间"));
    }
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let mut visited = HashSet::new();
    for project in project_ids {
        resolve(
            service,
            project,
            None,
            game_version,
            loader,
            Vec::new(),
            &mut visited,
            &mut entries,
            &mut warnings,
            0,
        )
        .await?;
        if entries.len() >= MAX_ENTRIES {
            warnings.push(format!(
                "已达到 {MAX_ENTRIES} 个文件上限，后续模组未加入计划"
            ));
            break;
        }
    }
    if entries.is_empty() {
        return Err(AppError::not_found("没有解析到可安装文件"));
    }
    warnings.push(
        "这是可审查的安装计划，不会自动创建新实例或修改配置；同名文件会先备份再覆盖。".into(),
    );
    Ok(ModInstallPlan {
        id: util::short_id(),
        instance_id: instance.id.clone(),
        instance_name: instance.name.clone(),
        created_at: util::now_rfc3339(),
        root_project: "modpack".into(),
        game_version: game_version.to_string(),
        loader: loader.to_string(),
        entries,
        warnings,
        notes: vec!["安装顺序按主模组与依赖解析结果；下载到临时目录并逐个校验 SHA-1。".into()],
    })
}

pub async fn save_plan(paths: &AppPaths, plan: &ModInstallPlan) -> Result<()> {
    if !safe_id(&plan.id) {
        return Err(AppError::invalid_input("安装计划 ID 非法"));
    }
    util::write_json(&plans_dir(paths).join(format!("{}.json", plan.id)), plan).await
}

pub async fn load_plan(paths: &AppPaths, id: &str) -> Result<ModInstallPlan> {
    if !safe_id(id) {
        return Err(AppError::invalid_input("安装计划 ID 非法"));
    }
    util::read_json(&plans_dir(paths).join(format!("{id}.json")))
        .await?
        .ok_or_else(|| AppError::not_found("安装计划不存在或已过期"))
}

struct Ticker {
    value: Arc<AtomicU64>,
    handle: tokio::task::JoinHandle<()>,
}

impl Ticker {
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
    fn set(&self, value: u64) {
        self.value.store(value, Ordering::Relaxed);
    }
    fn stop(self) {
        self.handle.abort();
    }
}

#[derive(Debug, Clone)]
pub struct InstallFile {
    pub file_name: String,
    pub url: String,
    pub size: u64,
    pub sha1: String,
}

pub async fn apply_with_http(
    http: &reqwest::Client,
    paths: &AppPaths,
    instance: &Instance,
    plan: &ModInstallPlan,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<ModInstallResult> {
    let files = plan
        .entries
        .iter()
        .map(|entry| InstallFile {
            file_name: entry.file_name.clone(),
            url: entry.url.clone(),
            size: entry.size,
            sha1: entry.sha1.clone(),
        })
        .collect::<Vec<_>>();
    apply_files(http, paths, instance, &files, &plan.warnings, task, cancel).await
}

/// Shared staged download + backup + atomic-replace implementation used by the
/// resource page and the deterministic dependency installer. Every file needs
/// a SHA-1; callers must build this list from verified API metadata only.
pub async fn apply_files(
    http: &reqwest::Client,
    paths: &AppPaths,
    instance: &Instance,
    files: &[InstallFile],
    warnings: &[String],
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<ModInstallResult> {
    let token = format!("mod-{}", util::short_id());
    let staging = paths
        .cache_dir()
        .join("assistant")
        .join("staging")
        .join(&token);
    let backup_dir = paths.base().join("backups").join("assistant").join(&token);
    util::ensure_dir(&staging).await?;
    util::ensure_dir(&backup_dir).await?;
    let mods_dir = instance.game_dir.join("mods");
    util::ensure_dir(&mods_dir).await?;
    let canonical_mods = tokio::fs::canonicalize(&mods_dir)
        .await
        .unwrap_or_else(|_| mods_dir.clone());
    let total: u64 = files.iter().map(|file| file.size).sum();
    task.set_total_files(files.len() as u32).await;
    task.set_total_bytes(total).await;
    task.set_status(TaskStatus::Running).await;

    let mut manifest = Manifest {
        token: token.clone(),
        instance_id: instance.id.clone(),
        created_at: util::now_rfc3339(),
        installed: Vec::new(),
        backups: Vec::new(),
        rolled_back: false,
    };
    util::write_json(&backup_dir.join("manifest.json"), &manifest).await?;

    let mut staged: Vec<(InstallFile, PathBuf)> = Vec::new();
    let result = async {
        let ticker = Ticker::start(task);
        let mut downloaded = 0u64;
        for file in files {
            if cancel.is_cancelled() {
                return Err(AppError::cancelled());
            }
            validate_file_name(&file.file_name)?;
            download::ensure_trusted_url(&file.url, false)?;
            let dest = staging.join(&file.file_name);
            task.set_current_file(&file.file_name).await;
            let spec = DownloadSpec {
                url: &file.url,
                dest: &dest,
                sha1: Some(&file.sha1),
                size: Some(file.size),
            };
            let base = downloaded;
            download::download_file(http, &spec, cancel, false, |done| {
                ticker.set(base.saturating_add(done))
            })
            .await?;
            downloaded = downloaded.saturating_add(file.size);
            staged.push((file.clone(), dest));
        }
        ticker.stop();

        for (file, staged_path) in &staged {
            let target = mods_dir.join(&file.file_name);
            if tokio::fs::try_exists(&target).await.unwrap_or(false) {
                let canonical = tokio::fs::canonicalize(&target)
                    .await
                    .map_err(|_| AppError::path_unsafe("无法校验目标文件"))?;
                if !canonical.starts_with(&canonical_mods) {
                    return Err(AppError::path_unsafe("拒绝覆盖 mods/ 之外的文件"));
                }
                let backup_name = format!("{}-{}", manifest.backups.len(), file.file_name);
                tokio::fs::copy(&target, backup_dir.join(&backup_name)).await?;
                manifest.backups.push(BackupFile {
                    file_name: file.file_name.clone(),
                    backup_name,
                });
            }
            let tmp = mods_dir.join(format!(".{}.tmp-{}", file.file_name, util::short_id()));
            tokio::fs::copy(staged_path, &tmp).await?;
            tokio::fs::rename(&tmp, &target).await?;
            manifest.installed.push(file.file_name.clone());
            util::write_json(&backup_dir.join("manifest.json"), &manifest).await?;
            task.file_finished().await;
        }
        Ok::<_, AppError>(())
    }
    .await;

    if let Err(error) = result {
        let _ = rollback_with_manifest(instance, &manifest, &backup_dir).await;
        let _ = tokio::fs::remove_dir_all(&staging).await;
        if error.code == ErrorCode::Cancelled {
            task.cancelled().await;
        } else {
            task.fail(error.clone()).await;
        }
        return Err(error);
    }

    let _ = tokio::fs::remove_dir_all(&staging).await;
    task.complete().await;
    Ok(ModInstallResult {
        token,
        backup_dir: backup_dir.display().to_string(),
        installed: manifest.installed.clone(),
        warnings: warnings.to_vec(),
    })
}

async fn rollback_with_manifest(
    instance: &Instance,
    manifest: &Manifest,
    backup_dir: &Path,
) -> Result<()> {
    let mods = instance.game_dir.join("mods");
    for file in &manifest.installed {
        let _ = tokio::fs::remove_file(mods.join(file)).await;
    }
    for backup in &manifest.backups {
        let source = backup_dir.join(&backup.backup_name);
        if tokio::fs::try_exists(&source).await.unwrap_or(false) {
            tokio::fs::copy(&source, mods.join(&backup.file_name)).await?;
        }
    }
    Ok(())
}

pub async fn rollback(paths: &AppPaths, instance: &Instance, token: &str) -> Result<()> {
    if !token.starts_with("mod-")
        || token.len() > 80
        || !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(AppError::invalid_input("回滚令牌非法"));
    }
    let backup_dir = paths.base().join("backups").join("assistant").join(token);
    let manifest_path = backup_dir.join("manifest.json");
    let mut manifest: Manifest = util::read_json(&manifest_path)
        .await?
        .ok_or_else(|| AppError::not_found("找不到该安装的备份记录"))?;
    if manifest.instance_id != instance.id {
        return Err(AppError::invalid_input("回滚记录与实例不匹配"));
    }
    if manifest.rolled_back {
        return Err(AppError::new(ErrorCode::AlreadyExists, "该安装已经回滚过"));
    }
    rollback_with_manifest(instance, &manifest, &backup_dir).await?;
    manifest.rolled_back = true;
    util::write_json(&manifest_path, &manifest).await?;
    Ok(())
}

#[cfg(test)]
mod install_tests {
    use super::*;

    fn instance(root: &Path) -> Instance {
        Instance {
            id: "inst".into(),
            name: "Test".into(),
            version_id: "1.21.1".into(),
            game_dir: root.join("game"),
            java_path: None,
            memory_mb: 2048,
            jvm_args: Vec::new(),
            game_args: Vec::new(),
            created_at: util::now_rfc3339(),
            last_launched_at: None,
            account_id: None,
            resolution_width: None,
            resolution_height: None,
            mod_loader: Some(crate::instance::ModLoaderInfo {
                kind: "fabric".into(),
                version: "0.16".into(),
                installed: true,
            }),
        }
    }

    #[tokio::test]
    async fn rollback_restores_backup_and_removes_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let instance = instance(dir.path());
        let mods = instance.game_dir.join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("kept.jar"), b"keep").unwrap();
        std::fs::write(mods.join("new.jar"), b"new").unwrap();
        // A previous version of new.jar was backed up before replacement.
        let backup_dir = dir.path().join("backup");
        std::fs::create_dir_all(&backup_dir).unwrap();
        std::fs::write(backup_dir.join("0-new.jar"), b"old").unwrap();
        let manifest = Manifest {
            token: "mod-test".into(),
            instance_id: instance.id.clone(),
            created_at: util::now_rfc3339(),
            installed: vec!["new.jar".into()],
            backups: vec![BackupFile {
                file_name: "new.jar".into(),
                backup_name: "0-new.jar".into(),
            }],
            rolled_back: false,
        };
        rollback_with_manifest(&instance, &manifest, &backup_dir)
            .await
            .unwrap();
        assert_eq!(std::fs::read(mods.join("new.jar")).unwrap(), b"old");
        assert_eq!(std::fs::read(mods.join("kept.jar")).unwrap(), b"keep");
    }

    #[test]
    fn rejects_file_names_with_path_separators() {
        assert!(validate_file_name("../evil.jar").is_err());
        assert!(validate_file_name("a/b.jar").is_err());
        assert!(validate_file_name("a.jar.disabled").is_err());
        assert!(validate_file_name("ok-1.0.jar").is_ok());
    }
}
