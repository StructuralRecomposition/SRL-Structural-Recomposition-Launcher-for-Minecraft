//! Safe, whitelisted repair plans with backups and rollback.
//!
//! Only three low-risk operations are allowed:
//! - change instance memory (instance.json only);
//! - disable a mod by renaming `*.jar` to `*.jar.disabled`;
//! - (rollback restores the previous state).
//!
//! Nothing here executes shell commands, downloads files, or touches saves.

use crate::assistant::LocalReport;
use crate::error::{AppError, ErrorCode, Result};
use crate::instance::{Instance, InstanceManager};
use crate::paths::AppPaths;
use crate::util;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairAction {
    /// `set_memory` | `disable_mod`
    pub kind: String,
    pub description: String,
    pub target: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub evidence: Vec<String>,
    pub risk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairPlan {
    pub id: String,
    pub instance_id: String,
    pub instance_name: String,
    pub created_at: String,
    pub actions: Vec<RepairAction>,
    pub notes: Vec<String>,
    pub rollback: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairResult {
    pub token: String,
    pub backup_dir: String,
    pub applied: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupManifest {
    token: String,
    instance_id: String,
    created_at: String,
    instance_json: Option<String>,
    mod_renames: Vec<ModRename>,
    rolled_back: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModRename {
    original: String,
    disabled: String,
}

fn plans_dir(paths: &AppPaths) -> PathBuf {
    paths.base().join("assistant").join("plans")
}

fn backups_root(paths: &AppPaths) -> PathBuf {
    paths.base().join("backups").join("assistant")
}

fn safe_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn plan_from_report(report: &LocalReport, instance: &Instance) -> RepairPlan {
    let mut actions = Vec::new();
    let mut notes = Vec::new();

    let mut oom_line = None;
    if let Some(crash) = &report.crash {
        if let Some(item) = crash
            .exceptions
            .iter()
            .find(|e| e.kind.contains("OutOfMemoryError"))
        {
            oom_line = Some(format!("{}:L{}", crash.file_name, item.line));
        }
    }
    if oom_line.is_none() {
        if let Some(log) = &report.log {
            if let Some(item) = log
                .error_lines
                .iter()
                .find(|l| l.text.contains("OutOfMemoryError"))
            {
                oom_line = Some(format!("{}:L{}", log.file_name, item.line));
            }
        }
    }
    if let Some(evidence) = oom_line {
        if instance.memory_mb < 16384 {
            let target = (instance.memory_mb + 2048).min(16384);
            actions.push(RepairAction {
                kind: "set_memory".into(),
                description: format!(
                    "将实例最大内存从 {} MB 提高到 {} MB",
                    instance.memory_mb, target
                ),
                target: instance.id.clone(),
                from: Some(instance.memory_mb.to_string()),
                to: Some(target.to_string()),
                evidence: vec![evidence],
                risk: "低：只修改实例 instance.json，可回滚；不会修改存档".into(),
            });
        } else {
            notes.push(
                "内存已达到 16384 MB，未自动继续提高；请检查是否有内存泄漏或过多模组。".into(),
            );
        }
    }

    if let Some(crash) = &report.crash {
        for suspect in crash.suspected_mods.iter().take(3) {
            let matched = report.mods.iter().find(|file| {
                let name = suspect.name.to_ascii_lowercase().replace('.', "");
                let file_lower = file.to_ascii_lowercase().replace('.', "");
                file_lower.contains(&name)
            });
            match matched {
                Some(file) if file.ends_with(".jar") => actions.push(RepairAction {
                    kind: "disable_mod".into(),
                    description: format!("暂时禁用模组 {file}"),
                    target: instance.id.clone(),
                    from: Some(file.clone()),
                    to: Some(format!("{file}.disabled")),
                    evidence: vec![format!(
                        "{}:L{}",
                        crash.file_name,
                        suspect.evidence_line.unwrap_or(0)
                    )],
                    risk: "低：仅重命名 mods/ 中的文件，可回滚；不会删除模组".into(),
                }),
                Some(_) => notes.push(format!(
                    "疑似模组 {} 对应的文件不是 .jar，未生成禁用操作",
                    suspect.name
                )),
                None => notes.push(format!(
                    "疑似模组 {} 没有在 mods/ 目录中找到对应文件，未生成操作",
                    suspect.name
                )),
            }
        }
    }

    if actions.is_empty() {
        notes.push(
            "本地证据不足以生成可执行的低风险修复；请人工检查日志或在对话中补充信息。".into(),
        );
    }
    notes.push(
        "执行前会备份 instance.json 和涉及的文件；任何失败都会自动回滚。不会触碰 saves/ 或玩家数据。"
            .into(),
    );

    RepairPlan {
        id: util::short_id(),
        instance_id: instance.id.clone(),
        instance_name: instance.name.clone(),
        created_at: util::now_rfc3339(),
        actions,
        notes,
        rollback: "使用生成的回滚令牌可恢复 instance.json 与原模组文件名。".into(),
    }
}

pub async fn save_plan(paths: &AppPaths, plan: &RepairPlan) -> Result<()> {
    if !safe_id(&plan.id) {
        return Err(AppError::invalid_input("修复计划 ID 非法"));
    }
    util::write_json(&plans_dir(paths).join(format!("{}.json", plan.id)), plan).await
}

pub async fn load_plan(paths: &AppPaths, id: &str) -> Result<RepairPlan> {
    if !safe_id(id) {
        return Err(AppError::invalid_input("修复计划 ID 非法"));
    }
    let path = plans_dir(paths).join(format!("{id}.json"));
    util::read_json::<RepairPlan>(&path)
        .await?
        .ok_or_else(|| AppError::not_found("修复计划不存在或已过期"))
}

fn validate_mod_file(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 255
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name == "."
        || name == ".."
    {
        return Err(AppError::path_unsafe("模组文件名非法"));
    }
    Ok(())
}

pub async fn apply(
    paths: &AppPaths,
    manager: &InstanceManager,
    instance: &Instance,
    plan: &RepairPlan,
) -> Result<RepairResult> {
    if plan.instance_id != instance.id {
        return Err(AppError::invalid_input("修复计划与实例不匹配"));
    }
    if plan.actions.is_empty() {
        return Err(AppError::invalid_input("修复计划没有可执行操作"));
    }
    if plan.actions.len() > 8 {
        return Err(AppError::invalid_input("修复计划操作过多"));
    }

    let token = format!("repair-{}", util::short_id());
    let backup_dir = backups_root(paths).join(&token);
    util::ensure_dir(&backup_dir).await?;
    let instance_json = manager.config_path(&instance.id);
    if tokio::fs::try_exists(&instance_json).await.unwrap_or(false) {
        tokio::fs::copy(&instance_json, backup_dir.join("instance.json")).await?;
    }
    let mut manifest = BackupManifest {
        token: token.clone(),
        instance_id: instance.id.clone(),
        created_at: util::now_rfc3339(),
        instance_json: Some("instance.json".into()),
        mod_renames: Vec::new(),
        rolled_back: false,
    };
    util::write_json(&backup_dir.join("manifest.json"), &manifest).await?;

    let mut applied = Vec::new();
    let mut warnings = Vec::new();
    let result = async {
        for action in &plan.actions {
            match action.kind.as_str() {
                "set_memory" => {
                    let target: u32 = action
                        .to
                        .as_deref()
                        .and_then(|v| v.parse().ok())
                        .ok_or_else(|| AppError::invalid_input("内存目标值非法"))?;
                    if !(512..=65536).contains(&target) {
                        return Err(AppError::invalid_input("内存目标值超出范围"));
                    }
                    let mut updated = instance.clone();
                    updated.memory_mb = target;
                    manager.save(&updated).await?;
                    applied.push(format!("内存已设为 {target} MB"));
                }
                "disable_mod" => {
                    let from = action
                        .from
                        .as_deref()
                        .ok_or_else(|| AppError::invalid_input("缺少模组文件名"))?;
                    validate_mod_file(from)?;
                    let mods = instance.game_dir.join("mods");
                    let source = mods.join(from);
                    let target_name = format!("{from}.disabled");
                    let target = mods.join(&target_name);
                    let canonical_mods = tokio::fs::canonicalize(&mods)
                        .await
                        .unwrap_or_else(|_| mods.clone());
                    let canonical_source = tokio::fs::canonicalize(&source)
                        .await
                        .map_err(|_| AppError::not_found(format!("模组不存在：{from}")))?;
                    if !canonical_source.starts_with(&canonical_mods) {
                        return Err(AppError::path_unsafe("拒绝操作 mods/ 之外的文件"));
                    }
                    if tokio::fs::try_exists(&target).await.unwrap_or(false) {
                        return Err(AppError::new(
                            ErrorCode::AlreadyExists,
                            format!("目标文件已存在：{target_name}"),
                        ));
                    }
                    tokio::fs::rename(&source, &target).await?;
                    manifest.mod_renames.push(ModRename {
                        original: from.to_string(),
                        disabled: target_name.clone(),
                    });
                    util::write_json(&backup_dir.join("manifest.json"), &manifest).await?;
                    applied.push(format!("已禁用 {from} → {target_name}"));
                }
                other => {
                    return Err(AppError::invalid_input(format!(
                        "不在白名单内的修复操作：{other}"
                    )))
                }
            }
        }
        Ok::<_, AppError>(())
    }
    .await;

    if let Err(error) = result {
        warnings.push(format!("执行失败，正在回滚：{}", error.message));
        let _ = rollback_with_manifest(paths, manager, instance, &manifest).await;
        return Err(error.with_detail(warnings.join("；")));
    }

    Ok(RepairResult {
        token,
        backup_dir: backup_dir.display().to_string(),
        applied,
        warnings,
    })
}

async fn rollback_with_manifest(
    _paths: &AppPaths,
    _manager: &InstanceManager,
    instance: &Instance,
    manifest: &BackupManifest,
) -> Result<()> {
    let backup_dir = backups_root(_paths).join(&manifest.token);
    if let Some(name) = &manifest.instance_json {
        let backup = backup_dir.join(name);
        if tokio::fs::try_exists(&backup).await.unwrap_or(false) {
            let bytes = tokio::fs::read(&backup).await?;
            util::atomic_write(&_manager.config_path(&instance.id), &bytes).await?;
        }
    }
    for rename in &manifest.mod_renames {
        let mods = instance.game_dir.join("mods");
        let disabled = mods.join(&rename.disabled);
        let original = mods.join(&rename.original);
        if tokio::fs::try_exists(&disabled).await.unwrap_or(false)
            && !tokio::fs::try_exists(&original).await.unwrap_or(false)
        {
            tokio::fs::rename(&disabled, &original).await?;
        }
    }
    Ok(())
}

pub async fn rollback(
    paths: &AppPaths,
    manager: &InstanceManager,
    instance: &Instance,
    token: &str,
) -> Result<()> {
    if !token.starts_with("repair-")
        || token.len() > 80
        || !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(AppError::invalid_input("回滚令牌非法"));
    }
    let backup_dir = backups_root(paths).join(token);
    let manifest_path = backup_dir.join("manifest.json");
    let mut manifest = util::read_json::<BackupManifest>(&manifest_path)
        .await?
        .ok_or_else(|| AppError::not_found("找不到该修复的备份记录"))?;
    if manifest.instance_id != instance.id {
        return Err(AppError::invalid_input("回滚记录与实例不匹配"));
    }
    if manifest.rolled_back {
        return Err(AppError::new(ErrorCode::AlreadyExists, "该修复已经回滚过"));
    }
    rollback_with_manifest(paths, manager, instance, &manifest).await?;
    manifest.rolled_back = true;
    util::write_json(&manifest_path, &manifest).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::LocalReport;
    use std::path::Path;

    fn instance(dir: &Path) -> Instance {
        let game_dir = dir.join("inst");
        Instance {
            id: "abc123".into(),
            name: "测试实例".into(),
            version_id: "1.21.1".into(),
            game_dir: game_dir.clone(),
            java_path: None,
            memory_mb: 2048,
            jvm_args: Vec::new(),
            game_args: Vec::new(),
            created_at: util::now_rfc3339(),
            last_launched_at: None,
            account_id: None,
            resolution_width: None,
            resolution_height: None,
            mod_loader: None,
        }
    }

    fn report(instance: &Instance) -> LocalReport {
        LocalReport {
            instance_id: instance.id.clone(),
            instance_name: instance.name.clone(),
            version_id: instance.version_id.clone(),
            loader: None,
            memory_mb: instance.memory_mb,
            java_path: None,
            mods: vec!["examplemod-1.0.jar".into()],
            crash: None,
            log: None,
            signals: Vec::new(),
            facts: Vec::new(),
            limits: Vec::new(),
        }
    }

    #[tokio::test]
    async fn applies_memory_and_disables_mod_then_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        paths.ensure_layout().await.unwrap();
        let inst = instance(dir.path());
        tokio::fs::create_dir_all(inst.game_dir.join("mods"))
            .await
            .unwrap();
        tokio::fs::write(inst.game_dir.join("mods/examplemod-1.0.jar"), b"jar")
            .await
            .unwrap();
        let manager = InstanceManager::new(crate::paths::GamePaths::new(dir.path().join("mc")));
        manager.save(&inst).await.unwrap();

        let plan = RepairPlan {
            id: util::short_id(),
            instance_id: inst.id.clone(),
            instance_name: inst.name.clone(),
            created_at: util::now_rfc3339(),
            actions: vec![
                RepairAction {
                    kind: "set_memory".into(),
                    description: "memory".into(),
                    target: inst.id.clone(),
                    from: Some("2048".into()),
                    to: Some("4096".into()),
                    evidence: vec!["crash.txt:L10".into()],
                    risk: "低".into(),
                },
                RepairAction {
                    kind: "disable_mod".into(),
                    description: "mod".into(),
                    target: inst.id.clone(),
                    from: Some("examplemod-1.0.jar".into()),
                    to: Some("examplemod-1.0.jar.disabled".into()),
                    evidence: vec!["crash.txt:L12".into()],
                    risk: "低".into(),
                },
            ],
            notes: Vec::new(),
            rollback: String::new(),
        };
        let result = apply(&paths, &manager, &inst, &plan).await.unwrap();
        let updated = manager.get(&inst.id).await.unwrap();
        assert_eq!(updated.memory_mb, 4096);
        assert!(inst
            .game_dir
            .join("mods/examplemod-1.0.jar.disabled")
            .exists());
        rollback(&paths, &manager, &updated, &result.token)
            .await
            .unwrap();
        let restored = manager.get(&inst.id).await.unwrap();
        assert_eq!(restored.memory_mb, 2048);
        assert!(inst.game_dir.join("mods/examplemod-1.0.jar").exists());
    }

    #[test]
    fn plan_is_humble_without_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let inst = instance(dir.path());
        let plan = plan_from_report(&report(&inst), &inst);
        assert!(plan.actions.is_empty());
        assert!(plan.notes.iter().any(|n| n.contains("证据不足")));
    }
}
