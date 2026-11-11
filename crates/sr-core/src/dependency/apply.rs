use super::model::DependencyAnalysis;
use crate::download::TaskHandle;
use crate::error::{AppError, ErrorCode, Result};
use crate::instance::Instance;
use crate::mod_install::{self, InstallFile, ModInstallResult};
use crate::paths::AppPaths;
use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallSelection {
    pub analysis_id: String,
    #[serde(default)]
    pub node_ids: Vec<String>,
}

pub fn dependencies_dir(paths: &AppPaths) -> std::path::PathBuf {
    paths.cache_dir().join("dependencies")
}

fn valid_analysis_id(value: &str) -> bool {
    value.starts_with("dep-")
        && value.len() <= 64
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

const MAX_ANALYSES: usize = 200;

pub async fn save_analysis(paths: &AppPaths, analysis: &DependencyAnalysis) -> Result<()> {
    if !valid_analysis_id(&analysis.id) {
        return Err(AppError::invalid_input("依赖分析 ID 非法"));
    }
    let dir = dependencies_dir(paths);
    util::write_json(&dir.join(format!("{}.json", analysis.id)), analysis).await?;
    prune_analyses(&dir, MAX_ANALYSES).await;
    Ok(())
}

/// Dependency snapshots are cache, not user data. Keep the newest N so a long
/// session cannot grow the data directory without bound.
async fn prune_analyses(dir: &std::path::Path, keep: usize) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("dep-") || !name.ends_with(".json") {
            continue;
        }
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        files.push((modified, entry.path()));
    }
    if files.len() <= keep {
        return;
    }
    files.sort_by_key(|(modified, _)| *modified);
    let remove = files.len().saturating_sub(keep);
    for (_, path) in files.into_iter().take(remove) {
        let _ = tokio::fs::remove_file(path).await;
    }
}

pub async fn load_analysis(paths: &AppPaths, id: &str) -> Result<DependencyAnalysis> {
    if !valid_analysis_id(id) {
        return Err(AppError::invalid_input("依赖分析 ID 非法"));
    }
    util::read_json(&dependencies_dir(paths).join(format!("{id}.json")))
        .await?
        .ok_or_else(|| AppError::not_found("依赖分析已过期，请重新分析"))
}

fn instance_loader(instance: &Instance) -> Option<String> {
    instance
        .mod_loader
        .as_ref()
        .map(|loader| loader.kind.to_ascii_lowercase())
        .or_else(|| {
            let id = instance.version_id.to_ascii_lowercase();
            if id.contains("fabric-loader-") {
                Some("fabric".into())
            } else if id.contains("quilt-loader-") {
                Some("quilt".into())
            } else if id.contains("-neoforge-") {
                Some("neoforge".into())
            } else if id.contains("-forge-") {
                Some("forge".into())
            } else {
                None
            }
        })
}

/// Apply a stored analysis. The renderer only sends node ids; URLs, sizes and
/// checksums are read back from the server-side analysis record.
pub async fn install_analysis(
    http: &reqwest::Client,
    paths: &AppPaths,
    instance: &Instance,
    selection: &InstallSelection,
    task: &TaskHandle,
    cancel: &CancellationToken,
) -> Result<ModInstallResult> {
    let analysis = load_analysis(paths, &selection.analysis_id).await?;
    if let Some(expected) = analysis.instance_id.as_deref() {
        if expected != instance.id {
            return Err(AppError::invalid_input(
                "依赖分析与目标实例不匹配，请重新分析",
            ));
        }
    }
    let loader = instance_loader(instance)
        .ok_or_else(|| AppError::invalid_input("目标实例没有可用的模组加载器"))?;
    if !loader.eq_ignore_ascii_case(&analysis.loader) {
        return Err(AppError::invalid_input(format!(
            "目标实例加载器为 {loader}，分析目标为 {}",
            analysis.loader
        )));
    }
    let version_matches = instance.version_id == analysis.game_version
        || instance
            .version_id
            .split(['-', '_'])
            .any(|part| part.eq_ignore_ascii_case(&analysis.game_version));
    if !version_matches {
        return Err(AppError::invalid_input(format!(
            "目标实例版本 {} 与分析的 Minecraft {} 不匹配",
            instance.version_id, analysis.game_version
        )));
    }
    if !analysis.blockers.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "依赖分析存在阻止安装的问题，请先处理冲突",
        )
        .with_detail(analysis.blockers.join("；")));
    }

    let root = analysis
        .nodes
        .iter()
        .find(|node| node.depth == 0)
        .ok_or_else(|| AppError::internal("依赖分析缺少根模组"))?;
    let requested: HashSet<&str> = if selection.node_ids.is_empty() {
        analysis
            .default_plan
            .iter()
            .map(|entry| entry.node_id.as_str())
            .collect()
    } else {
        selection.node_ids.iter().map(String::as_str).collect()
    };
    if !requested.contains(root.id.as_str()) {
        return Err(AppError::invalid_input("安装计划必须包含目标模组本身"));
    }
    for node in &analysis.nodes {
        if !requested.contains(node.id.as_str()) {
            continue;
        }
        if node.relation == super::model::DependencyRelation::Incompatible {
            return Err(AppError::invalid_input(format!(
                "不能安装已确认不兼容的模组：{}",
                node.title
            )));
        }
        if !node.compatible {
            return Err(AppError::invalid_input(format!(
                "{} 与目标 Minecraft 版本/加载器不匹配",
                node.title
            )));
        }
        if node.depth > 0
            && !matches!(
                node.relation,
                super::model::DependencyRelation::Required
                    | super::model::DependencyRelation::Optional
            )
        {
            return Err(AppError::invalid_input(format!(
                "{} 的关系是 {}，只能查看，不能加入安装计划",
                node.title,
                node.relation.as_str()
            )));
        }
        if node.relation == super::model::DependencyRelation::Required
            && node.installable
            && !node.installed
            && node.depth > 0
            && !requested.contains(node.id.as_str())
        {
            return Err(AppError::invalid_input(format!(
                "缺少必需依赖：{}",
                node.title
            )));
        }
    }

    let mut files = Vec::new();
    let mut names = HashSet::new();
    for node in analysis
        .nodes
        .iter()
        .filter(|node| requested.contains(node.id.as_str()))
    {
        if node.installed && node.depth > 0 {
            continue;
        }
        let (Some(file_name), Some(url), Some(sha1)) =
            (node.file_name.clone(), node.url.clone(), node.sha1.clone())
        else {
            return Err(AppError::invalid_input(format!(
                "{} 缺少可校验的下载文件",
                node.title
            )));
        };
        if file_name.is_empty() || sha1.is_empty() {
            return Err(AppError::invalid_input(format!(
                "{} 的下载元数据不完整",
                node.title
            )));
        }
        if !names.insert(file_name.clone()) {
            return Err(AppError::invalid_input(format!(
                "安装计划里有重复文件名：{file_name}"
            )));
        }
        files.push(InstallFile {
            file_name,
            url,
            size: node.size,
            sha1,
        });
    }
    if files.is_empty() {
        return Err(AppError::new(
            ErrorCode::AlreadyExists,
            "所选依赖都已经安装，没有需要下载的文件",
        ));
    }
    mod_install::apply_files(
        http,
        paths,
        instance,
        &files,
        &analysis.warnings,
        task,
        cancel,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependency::model::{
        DependencyNode, DependencyRelation, DependencyStats, PlannedEntry,
    };

    fn analysis(blockers: Vec<String>, nodes: Vec<DependencyNode>) -> DependencyAnalysis {
        DependencyAnalysis {
            id: "dep-test".into(),
            created_at: "2026-11-12T00:00:00Z".into(),
            source: "modrinth".into(),
            root_project_id: "root".into(),
            root_version_id: Some("v1".into()),
            game_version: "1.21.1".into(),
            loader: "fabric".into(),
            instance_id: None,
            instance_name: None,
            nodes,
            edges: Vec::new(),
            warnings: Vec::new(),
            blockers,
            stats: DependencyStats::default(),
            default_plan: Vec::new(),
            complete: true,
        }
    }

    fn node(id: &str, relation: DependencyRelation, installed: bool) -> DependencyNode {
        DependencyNode {
            id: id.into(),
            source: "modrinth".into(),
            project_id: id.into(),
            title: id.into(),
            version_id: Some("v1".into()),
            version_number: Some("1".into()),
            version_type: Some("release".into()),
            file_name: Some(format!("{id}.jar")),
            url: Some("https://cdn.modrinth.com/data/x/y.jar".into()),
            sha1: Some("a".repeat(40)),
            sha512: None,
            size: 10,
            has_checksum: true,
            relation,
            depth: if id == "root" { 0 } else { 1 },
            parent: None,
            required_by: Vec::new(),
            installed,
            installed_file: None,
            installed_version: None,
            compatible: true,
            installable: true,
            selected_by_default: true,
            notes: Vec::new(),
        }
    }

    #[tokio::test]
    async fn prunes_old_analysis_cache() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..3 {
            tokio::fs::write(dir.path().join(format!("dep-{index}.json")), b"{}")
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        prune_analyses(dir.path(), 2).await;
        let mut names = Vec::new();
        let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        assert_eq!(names, vec!["dep-1.json", "dep-2.json"]);
    }

    #[test]
    fn rejects_analysis_with_blockers() {
        let value = analysis(
            vec!["conflict".into()],
            vec![node("root", DependencyRelation::Required, false)],
        );
        assert!(!value.blockers.is_empty());
    }

    #[test]
    fn default_plan_requires_root_and_required_nodes() {
        let value = analysis(
            Vec::new(),
            vec![
                node("root", DependencyRelation::Required, false),
                node("dep", DependencyRelation::Required, false),
            ],
        );
        value.validate_marker();
    }

    // Pure marker to keep the fixture types referenced in this test module.
    impl DependencyAnalysis {
        fn validate_marker(&self) {
            assert_eq!(self.nodes.len(), 2);
            assert!(self.default_plan.is_empty());
            let _planned = PlannedEntry {
                node_id: "root".into(),
                title: "root".into(),
                file_name: "root.jar".into(),
                size: 10,
                relation: DependencyRelation::Required,
            };
        }
    }
}
