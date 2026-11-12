//! Deterministic recursive dependency resolution.

use super::installed::InstalledIndex;
use super::model::*;
use crate::curseforge::{
    curseforge_relation, split_versions, CurseForgeRelation, CurseForgeService,
};
use crate::error::{AppError, Result};
use crate::modrinth::{ModrinthService, ResourceProject, ResourceVersion};
use crate::util;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ResolveLimits {
    pub max_depth: u32,
    pub max_requests: usize,
    pub max_nodes: usize,
}

impl Default for ResolveLimits {
    fn default() -> Self {
        Self {
            max_depth: 16,
            max_requests: 120,
            max_nodes: 200,
        }
    }
}

#[async_trait]
pub trait Adapter: Send + Sync {
    fn source(&self) -> DependencySource;
    async fn root(&self, request: &DependencyRequest) -> Result<ResolvedItem>;
    async fn fetch(
        &self,
        project_id: &str,
        version_id: Option<&str>,
        game_version: &str,
        loader: &str,
    ) -> Result<ResolvedItem>;
}

pub enum AdapterRef<'a> {
    Modrinth(ModrinthAdapter<'a>),
    CurseForge(CurseForgeAdapter<'a>),
}

impl AdapterRef<'_> {
    pub fn as_adapter(&self) -> &dyn Adapter {
        match self {
            Self::Modrinth(adapter) => adapter,
            Self::CurseForge(adapter) => adapter,
        }
    }
}

pub fn adapter_for<'a>(
    request: &DependencyRequest,
    modrinth: &'a ModrinthService,
    curseforge: Option<&'a CurseForgeService>,
) -> Result<AdapterRef<'a>> {
    let source = DependencySource::parse(&request.source)
        .ok_or_else(|| AppError::invalid_input("不支持的依赖来源"))?;
    match source {
        DependencySource::Modrinth => Ok(AdapterRef::Modrinth(ModrinthAdapter::new(modrinth))),
        DependencySource::CurseForge => {
            let service = curseforge
                .ok_or_else(|| AppError::not_configured("CurseForge API 不可用，无法分析依赖"))?;
            Ok(AdapterRef::CurseForge(CurseForgeAdapter::new(service)))
        }
    }
}

pub async fn analyze(
    request: &DependencyRequest,
    adapter: &dyn Adapter,
    installed: &InstalledIndex,
    limits: &ResolveLimits,
) -> Result<DependencyAnalysis> {
    if request.game_version.trim().is_empty() {
        return Err(AppError::invalid_input("请选择目标 Minecraft 版本"));
    }
    if request.loader.trim().is_empty() {
        return Err(AppError::invalid_input("请选择目标模组加载器"));
    }
    let game_version = request.game_version.trim();
    let loader = request.loader.trim();

    let root = adapter.root(request).await?;
    let mut warnings: Vec<String> = Vec::new();
    let mut blockers: Vec<String> = Vec::new();
    let mut nodes: Vec<DependencyNode> = Vec::new();
    let mut edges: Vec<DependencyEdge> = Vec::new();
    let mut edge_keys: HashSet<(String, String, String)> = HashSet::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut requests = 1usize;
    let mut complete = true;

    if !root.supports(game_version, loader) {
        complete = false;
        blockers.push(format!(
            "根模组 {} {} 不支持 Minecraft {} / {}",
            root.project_title, root.version_number, game_version, loader
        ));
    }

    let root_id = dependency_node_id(
        root.source.as_str(),
        &root.project_id,
        Some(root.version_id.as_str()),
    );
    let root_candidates =
        installed.matches(root.source.as_str(), &root.project_id, &root.project_slug);
    let (root_state, root_installed) = evaluate_installed(&root_candidates, &root);
    let root_installable = root
        .file
        .as_ref()
        .is_some_and(|file| file.sha1.as_deref().is_some_and(|value| !value.is_empty()));
    let root_node = make_node(
        &root,
        DependencyRelation::Required,
        0,
        None,
        Vec::new(),
        root.supports(game_version, loader),
        root_installable,
        true,
        root_installed,
        root_state,
    );
    index.insert(root_id.clone(), nodes.len());
    nodes.push(root_node);

    // Stack entries: (parent id, title, depth, relation, dependency, active)
    // `active` is false below an unselected optional dependency, so its
    // required children are not auto-installed.
    let mut stack: Vec<(
        String,
        String,
        u32,
        DependencyRelation,
        ResolvedDependency,
        bool,
    )> = Vec::new();
    for dependency in root.dependencies.iter().rev() {
        stack.push((
            root_id.clone(),
            root.project_title.clone(),
            1,
            dependency.relation,
            dependency.clone(),
            true,
        ));
    }

    while let Some((parent_id, parent_title, depth, relation, dependency, active)) = stack.pop() {
        let Some(project_id) = dependency.project_id.clone().filter(|v| !v.is_empty()) else {
            complete = false;
            let message = format!(
                "{} 的 {} 依赖缺少项目 ID，无法解析；原始类型 {}",
                parent_title,
                relation.as_str(),
                dependency.raw_type
            );
            if relation == DependencyRelation::Required {
                blockers.push(message.clone());
            }
            warnings.push(message);
            continue;
        };
        let node_id = dependency_node_id(
            adapter.source().as_str(),
            &project_id,
            dependency.version_id.as_deref(),
        );
        let existing = index.get(&node_id).copied().or_else(|| {
            if dependency.version_id.is_some() {
                return None;
            }
            nodes.iter().position(|node| {
                node.source == adapter.source().as_str() && node.project_id == project_id
            })
        });
        if let Some(existing) = existing {
            let existing_id = nodes[existing].id.clone();
            push_edge(
                &mut edges,
                &mut edge_keys,
                &parent_id,
                &existing_id,
                relation,
                &dependency.raw_type,
            );
            if relation == DependencyRelation::Incompatible {
                let title = nodes[existing].title.clone();
                blockers.push(format!(
                    "{} 与目标模组不兼容（{} 官方元数据）",
                    title,
                    adapter.source().as_str()
                ));
            }
            if relation == DependencyRelation::Required {
                let node = &mut nodes[existing];
                node.relation = DependencyRelation::Required;
                node.selected_by_default = active && node.compatible && node.installable;
                if !node.required_by.iter().any(|value| value == &parent_title) {
                    node.required_by.push(parent_title.clone());
                }
            }
            continue;
        }
        if depth > limits.max_depth {
            complete = false;
            let message = format!(
                "依赖层级超过 {} 层，未继续解析 {}",
                limits.max_depth, project_id
            );
            if relation == DependencyRelation::Required {
                blockers.push(message.clone());
            }
            warnings.push(message);
            continue;
        }
        if requests >= limits.max_requests || nodes.len() >= limits.max_nodes {
            complete = false;
            let message = format!("依赖请求或节点数量达到上限，{} 未解析", project_id);
            if relation == DependencyRelation::Required {
                blockers.push(message.clone());
            }
            warnings.push(message);
            continue;
        }
        requests += 1;
        let fetched = adapter
            .fetch(
                &project_id,
                dependency.version_id.as_deref(),
                game_version,
                loader,
            )
            .await;
        match fetched {
            Ok(item) => {
                let compatible = item.supports(game_version, loader);
                let installable = item.file.as_ref().is_some_and(|file| {
                    file.sha1.as_deref().is_some_and(|value| !value.is_empty())
                });
                let candidates =
                    installed.matches(item.source.as_str(), &item.project_id, &item.project_slug);
                let (installed_state, installed) = evaluate_installed(&candidates, &item);
                let node = make_node(
                    &item,
                    relation,
                    depth,
                    Some(parent_id.clone()),
                    vec![parent_title.clone()],
                    compatible,
                    installable,
                    active && relation == DependencyRelation::Required && compatible && installable,
                    installed,
                    installed_state,
                );
                let node_id = node.id.clone();
                index.insert(node_id.clone(), nodes.len());
                nodes.push(node);
                push_edge(
                    &mut edges,
                    &mut edge_keys,
                    &parent_id,
                    &node_id,
                    relation,
                    &dependency.raw_type,
                );
                if relation == DependencyRelation::Incompatible {
                    blockers.push(format!(
                        "{} 与目标模组不兼容（{} 官方元数据）",
                        item.project_title,
                        adapter.source().as_str()
                    ));
                }
                if relation == DependencyRelation::Unknown {
                    complete = false;
                    warnings.push(format!(
                        "{} 的关系类型未知（{}），不会自动安装",
                        item.project_title, dependency.raw_type
                    ));
                }
                if relation == DependencyRelation::Required && !compatible {
                    blockers.push(format!(
                        "必需依赖 {} {} 不支持 Minecraft {} / {}",
                        item.project_title, item.version_number, game_version, loader
                    ));
                }
                if relation == DependencyRelation::Required && !installable {
                    blockers.push(format!(
                        "必需依赖 {} 没有可校验的下载文件，已阻止自动安装",
                        item.project_title
                    ));
                }
                for note in &item.notes {
                    warnings.push(format!("{}：{}", item.project_title, note));
                }
                if relation.is_recursive() {
                    for child in item.dependencies.iter().rev() {
                        stack.push((
                            node_id.clone(),
                            item.project_title.clone(),
                            depth + 1,
                            child.relation,
                            child.clone(),
                            active && relation == DependencyRelation::Required,
                        ));
                    }
                }
            }
            Err(error) => {
                complete = false;
                let title = format!("未知依赖 {project_id}");
                let message = format!(
                    "{} 的 {} 依赖 {} 无法解析：{}",
                    parent_title,
                    relation.as_str(),
                    project_id,
                    error.message
                );
                if relation == DependencyRelation::Required {
                    blockers.push(message.clone());
                }
                warnings.push(message);
                let node_id = format!(
                    "{}:unresolved",
                    dependency_node_id(
                        adapter.source().as_str(),
                        &project_id,
                        dependency.version_id.as_deref()
                    )
                );
                if !index.contains_key(&node_id) && nodes.len() < limits.max_nodes {
                    nodes.push(DependencyNode {
                        id: node_id.clone(),
                        source: adapter.source().as_str().into(),
                        project_id: project_id.clone(),
                        title,
                        version_id: None,
                        version_number: None,
                        version_type: None,
                        file_name: None,
                        url: None,
                        sha1: None,
                        sha512: None,
                        size: 0,
                        has_checksum: false,
                        relation,
                        depth,
                        parent: Some(parent_id.clone()),
                        required_by: vec![parent_title.clone()],
                        installed: false,
                        installed_state: "not_installed".into(),
                        installed_file: None,
                        installed_version: None,
                        compatible: false,
                        installable: false,
                        selected_by_default: false,
                        notes: vec![error.message.clone()],
                    });
                    index.insert(node_id.clone(), nodes.len() - 1);
                }
                push_edge(
                    &mut edges,
                    &mut edge_keys,
                    &parent_id,
                    &node_id,
                    relation,
                    &dependency.raw_type,
                );
            }
        }
    }

    // Deduplicate messages while preserving order.
    dedup(&mut warnings);
    dedup(&mut blockers);

    // Final graph integrity check: every edge endpoint must resolve to exactly
    // one node. A broken graph must never reach the install step.
    let mut ids = HashSet::new();
    for node in &nodes {
        if !ids.insert(node.id.as_str()) {
            complete = false;
            blockers.push(format!("依赖图包含重复节点 ID：{}", node.id));
        }
    }
    for edge in &edges {
        if !ids.contains(edge.from.as_str()) || !ids.contains(edge.to.as_str()) {
            complete = false;
            blockers.push(format!("依赖边端点不存在：{} -> {}", edge.from, edge.to));
        }
    }

    let default_plan = nodes
        .iter()
        .filter(|node| {
            node.selected_by_default
                && node.compatible
                && node.installable
                && (node.depth == 0 || !node.installed)
                && node.relation != DependencyRelation::Incompatible
        })
        .map(|node| PlannedEntry {
            node_id: node.id.clone(),
            title: node.title.clone(),
            file_name: node.file_name.clone().unwrap_or_default(),
            size: node.size,
            relation: node.relation,
        })
        .collect::<Vec<_>>();

    let stats = DependencyStats {
        nodes: nodes.len(),
        required: nodes
            .iter()
            .filter(|node| node.relation == DependencyRelation::Required)
            .count(),
        optional: nodes
            .iter()
            .filter(|node| node.relation == DependencyRelation::Optional)
            .count(),
        incompatible: nodes
            .iter()
            .filter(|node| node.relation == DependencyRelation::Incompatible)
            .count(),
        installed: nodes.iter().filter(|node| node.installed).count(),
        to_install: default_plan.len(),
        total_bytes: default_plan.iter().map(|entry| entry.size).sum(),
    };

    Ok(DependencyAnalysis {
        id: format!("dep-{}", util::short_id()),
        created_at: util::now_rfc3339(),
        source: adapter.source().as_str().into(),
        root_project_id: root.project_id.clone(),
        root_version_id: Some(root.version_id.clone()),
        game_version: game_version.to_string(),
        loader: loader.to_string(),
        instance_id: request.instance_id.clone(),
        instance_name: None,
        nodes,
        edges,
        warnings,
        blockers,
        stats,
        default_plan,
        complete,
    })
}

#[allow(clippy::too_many_arguments)]
/// The single source of truth for dependency node identity.
pub fn dependency_node_id(source: &str, project_id: &str, version: Option<&str>) -> String {
    format!(
        "{}:{}:{}",
        source,
        project_id,
        version.filter(|value| !value.is_empty()).unwrap_or("any")
    )
}

fn evaluate_installed(
    candidates: &[&crate::dependency::InstalledMod],
    item: &ResolvedItem,
) -> (String, Option<crate::dependency::InstalledMod>) {
    if candidates.is_empty() {
        return ("not_installed".into(), None);
    }
    if candidates.len() > 1 {
        return ("conflict".into(), Some(candidates[0].clone()));
    }
    let installed = candidates[0];
    if !installed.enabled {
        return ("disabled".into(), Some(installed.clone()));
    }
    match installed.version.as_deref() {
        Some(version)
            if version.eq_ignore_ascii_case(&item.version_number)
                || version.eq_ignore_ascii_case(&item.version_id) =>
        {
            ("enabled_satisfied".into(), Some(installed.clone()))
        }
        Some(_) => ("version_mismatch".into(), Some(installed.clone())),
        None => ("version_unknown".into(), Some(installed.clone())),
    }
}

#[allow(clippy::too_many_arguments)]
fn make_node(
    item: &ResolvedItem,
    relation: DependencyRelation,
    depth: u32,
    parent: Option<String>,
    required_by: Vec<String>,
    compatible: bool,
    installable: bool,
    selected_by_default: bool,
    installed: Option<crate::dependency::InstalledMod>,
    installed_state: String,
) -> DependencyNode {
    let file = item.file.as_ref();
    let mut notes = item.notes.clone();
    if let Some(installed) = installed.as_ref() {
        let installed_version = installed.version.as_deref().unwrap_or("未知");
        notes.push(match installed_state.as_str() {
            "enabled_satisfied" => format!("本地已启用且版本匹配：{installed_version}"),
            "disabled" => {
                format!("本地版本 {installed_version} 已禁用（.disabled），不能视为满足依赖")
            }
            "version_mismatch" => format!(
                "本地版本 {installed_version} 与目标版本 {} 不匹配，需要更新",
                item.version_number
            ),
            "version_unknown" => "本地模组版本未知，不能视为满足依赖".into(),
            "conflict" => "本地存在多个同项目模组文件，请先处理冲突".into(),
            _ => format!("本地文件：{}", installed.file_name),
        });
        if !installed.enabled {
            notes.push("本地模组文件处于禁用状态（.disabled）".into());
        }
    }
    DependencyNode {
        id: dependency_node_id(
            item.source.as_str(),
            &item.project_id,
            Some(item.version_id.as_str()),
        ),
        source: item.source.as_str().into(),
        project_id: item.project_id.clone(),
        title: item.project_title.clone(),
        version_id: Some(item.version_id.clone()),
        version_number: Some(item.version_number.clone()),
        version_type: Some(item.version_type.clone()),
        file_name: file.map(|file| file.file_name.clone()),
        url: file.map(|file| file.url.clone()),
        sha1: file.and_then(|file| file.sha1.clone()),
        sha512: file.and_then(|file| file.sha512.clone()),
        size: file.map(|file| file.size).unwrap_or(0),
        has_checksum: file.is_some_and(|file| {
            file.sha1.as_deref().is_some_and(|value| !value.is_empty())
                || file
                    .sha512
                    .as_deref()
                    .is_some_and(|value| !value.is_empty())
        }),
        relation,
        depth,
        parent,
        required_by,
        installed: installed_state == "enabled_satisfied",
        installed_state,
        installed_file: installed.as_ref().map(|item| item.file_name.clone()),
        installed_version: installed.as_ref().and_then(|item| item.version.clone()),
        compatible,
        installable,
        selected_by_default,
        notes,
    }
}

fn push_edge(
    edges: &mut Vec<DependencyEdge>,
    keys: &mut HashSet<(String, String, String)>,
    from: &str,
    to: &str,
    relation: DependencyRelation,
    raw_type: &str,
) {
    let key = (from.to_string(), to.to_string(), relation.as_str().into());
    if keys.insert(key) {
        edges.push(DependencyEdge {
            from: from.to_string(),
            to: to.to_string(),
            relation,
            raw_type: raw_type.to_string(),
        });
    }
}

fn dedup(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

// ---------------------------------------------------------------------------
// Modrinth
// ---------------------------------------------------------------------------

pub struct ModrinthAdapter<'a> {
    service: &'a ModrinthService,
    projects: Mutex<HashMap<String, ResourceProject>>,
}

impl<'a> ModrinthAdapter<'a> {
    pub fn new(service: &'a ModrinthService) -> Self {
        Self {
            service,
            projects: Mutex::new(HashMap::new()),
        }
    }

    async fn project(&self, id: &str) -> Result<ResourceProject> {
        if let Some(project) = self
            .projects
            .lock()
            .ok()
            .and_then(|map| map.get(id).cloned())
        {
            return Ok(project);
        }
        let project = self.service.project(id).await?;
        if let Ok(mut map) = self.projects.lock() {
            map.insert(id.to_string(), project.clone());
        }
        Ok(project)
    }

    fn item(&self, project: ResourceProject, version: ResourceVersion) -> ResolvedItem {
        let file = version
            .files
            .iter()
            .find(|file| file.primary)
            .or_else(|| version.files.first())
            .map(|file| ResolvedFile {
                file_name: file.filename.clone(),
                url: file.url.clone(),
                size: file.size,
                sha1: file.sha1.clone(),
                sha512: file.sha512.clone(),
            });
        let mut notes = Vec::new();
        if version.version_type != "release" {
            notes.push(format!(
                "所选版本类型为 {}，不是稳定版",
                version.version_type
            ));
        }
        if version.game_versions.is_empty() || version.loaders.is_empty() {
            notes.push("该版本未声明完整的 Minecraft/加载器范围，请人工确认".into());
        }
        ResolvedItem {
            source: DependencySource::Modrinth,
            project_id: version.project_id.clone(),
            project_title: project.title,
            project_slug: project.slug,
            version_id: version.id,
            version_number: version.version_number,
            version_type: version.version_type,
            game_versions: version.game_versions,
            loaders: version.loaders,
            file,
            dependencies: version
                .dependencies
                .into_iter()
                .map(|dep| ResolvedDependency {
                    project_id: dep.project_id,
                    version_id: dep.version_id,
                    relation: modrinth_relation(&dep.dependency_type),
                    raw_type: dep.dependency_type,
                })
                .collect(),
            notes,
        }
    }
}

pub fn modrinth_relation(value: &str) -> DependencyRelation {
    match value.trim().to_ascii_lowercase().as_str() {
        "required" => DependencyRelation::Required,
        "optional" => DependencyRelation::Optional,
        "incompatible" => DependencyRelation::Incompatible,
        "embedded" => DependencyRelation::Embedded,
        _ => DependencyRelation::Unknown,
    }
}

#[async_trait]
impl Adapter for ModrinthAdapter<'_> {
    fn source(&self) -> DependencySource {
        DependencySource::Modrinth
    }

    async fn root(&self, request: &DependencyRequest) -> Result<ResolvedItem> {
        let project = self.project(&request.project_id).await?;
        let version = match request.version_id.as_deref() {
            Some(version_id) => {
                self.service
                    .version(version_id, &request.project_id)
                    .await?
            }
            None => {
                let versions = self
                    .service
                    .versions(
                        &request.project_id,
                        Some(&request.game_version),
                        Some(&request.loader),
                    )
                    .await?;
                pick_modrinth_version(versions, &request.game_version, &request.loader)?
            }
        };
        Ok(self.item(project, version))
    }

    async fn fetch(
        &self,
        project_id: &str,
        version_id: Option<&str>,
        game_version: &str,
        loader: &str,
    ) -> Result<ResolvedItem> {
        let project = self.project(project_id).await?;
        let version = match version_id {
            Some(version_id) => self.service.version(version_id, project_id).await?,
            None => {
                let mut versions = self
                    .service
                    .versions(project_id, Some(game_version), Some(loader))
                    .await?;
                if versions.is_empty() {
                    // Some universal mods do not declare a loader; retry
                    // without the loader facet rather than claiming missing.
                    versions = self
                        .service
                        .versions(project_id, Some(game_version), None)
                        .await?;
                }
                pick_modrinth_version(versions, game_version, loader)?
            }
        };
        Ok(self.item(project, version))
    }
}

fn pick_modrinth_version(
    versions: Vec<ResourceVersion>,
    game_version: &str,
    loader: &str,
) -> Result<ResourceVersion> {
    let compatible = |version: &ResourceVersion| {
        let game_ok = version.game_versions.is_empty()
            || version
                .game_versions
                .iter()
                .any(|value| value.eq_ignore_ascii_case(game_version));
        let loader_ok = version.loaders.is_empty()
            || version
                .loaders
                .iter()
                .any(|value| value.eq_ignore_ascii_case(loader));
        game_ok && loader_ok
    };
    let mut compatible = versions.into_iter().filter(compatible).collect::<Vec<_>>();
    compatible.sort_by(|a, b| b.date_published.cmp(&a.date_published));
    compatible
        .iter()
        .find(|version| version.version_type == "release")
        .cloned()
        .or_else(|| compatible.first().cloned())
        .ok_or_else(|| {
            AppError::not_found(format!(
                "没有找到支持 Minecraft {game_version} / {loader} 的版本"
            ))
        })
}

// ---------------------------------------------------------------------------
// CurseForge
// ---------------------------------------------------------------------------

pub struct CurseForgeAdapter<'a> {
    service: &'a CurseForgeService,
    projects: Mutex<HashMap<u64, crate::curseforge::CurseForgeMod>>,
}

impl<'a> CurseForgeAdapter<'a> {
    pub fn new(service: &'a CurseForgeService) -> Self {
        Self {
            service,
            projects: Mutex::new(HashMap::new()),
        }
    }

    async fn project(&self, id: u64) -> Result<crate::curseforge::CurseForgeMod> {
        if let Some(project) = self
            .projects
            .lock()
            .ok()
            .and_then(|map| map.get(&id).cloned())
        {
            return Ok(project);
        }
        let project = self.service.project(id).await?;
        if let Ok(mut map) = self.projects.lock() {
            map.insert(id, project.clone());
        }
        Ok(project)
    }

    fn item(
        &self,
        project: crate::curseforge::CurseForgeMod,
        file: crate::curseforge::CurseForgeFile,
    ) -> ResolvedItem {
        let (game_versions, loaders) = split_versions(&file.game_versions);
        let mut notes = Vec::new();
        if file.release_type != 1 {
            notes.push(format!(
                "所选文件发布类型为 {}，不是稳定版",
                release_label(file.release_type)
            ));
        }
        if game_versions.is_empty() {
            notes.push("该文件没有声明 Minecraft 版本，请人工确认".into());
        }
        ResolvedItem {
            source: DependencySource::CurseForge,
            project_id: project.id.to_string(),
            project_title: project.name,
            project_slug: project.slug,
            version_id: file.id.to_string(),
            version_number: file.file_name.clone(),
            version_type: release_label(file.release_type).into(),
            game_versions,
            loaders,
            file: Some(ResolvedFile {
                file_name: file.file_name.clone(),
                url: file.download_url.clone(),
                size: file.size,
                sha1: file.sha1.clone(),
                sha512: file.sha512.clone(),
            }),
            dependencies: file
                .dependencies
                .into_iter()
                .map(|dep| ResolvedDependency {
                    project_id: Some(dep.mod_id.to_string()),
                    version_id: None,
                    relation: map_curseforge_relation(dep.relation_type),
                    raw_type: dep.relation_type.to_string(),
                })
                .collect(),
            notes,
        }
    }
}

pub fn map_curseforge_relation(value: u32) -> DependencyRelation {
    match curseforge_relation(value) {
        CurseForgeRelation::EmbeddedLibrary => DependencyRelation::Embedded,
        CurseForgeRelation::OptionalDependency => DependencyRelation::Optional,
        CurseForgeRelation::RequiredDependency => DependencyRelation::Required,
        CurseForgeRelation::Tool => DependencyRelation::Tool,
        CurseForgeRelation::Incompatible => DependencyRelation::Incompatible,
        CurseForgeRelation::Include => DependencyRelation::Include,
        CurseForgeRelation::Unknown => DependencyRelation::Unknown,
    }
}

fn release_label(value: u32) -> &'static str {
    match value {
        1 => "release",
        2 => "beta",
        3 => "alpha",
        _ => "unknown",
    }
}

#[async_trait]
impl Adapter for CurseForgeAdapter<'_> {
    fn source(&self) -> DependencySource {
        DependencySource::CurseForge
    }

    async fn root(&self, request: &DependencyRequest) -> Result<ResolvedItem> {
        let project_id = request
            .project_id
            .parse::<u64>()
            .map_err(|_| AppError::invalid_input("CurseForge 项目 ID 无效"))?;
        let file_id = request
            .file_id
            .as_deref()
            .ok_or_else(|| AppError::invalid_input("CurseForge 依赖分析需要具体文件 ID"))?
            .parse::<u64>()
            .map_err(|_| AppError::invalid_input("CurseForge 文件 ID 无效"))?;
        let file = self.service.file(project_id, file_id).await?;
        if file.mod_id != project_id {
            return Err(AppError::invalid_input("CurseForge 文件与项目不匹配"));
        }
        let project = self.project(project_id).await?;
        Ok(self.item(project, file))
    }

    async fn fetch(
        &self,
        project_id: &str,
        _version_id: Option<&str>,
        game_version: &str,
        loader: &str,
    ) -> Result<ResolvedItem> {
        let id = project_id
            .parse::<u64>()
            .map_err(|_| AppError::invalid_input("CurseForge 项目 ID 无效"))?;
        let loader_type = crate::curseforge::mod_loader_type(loader);
        let mut files = self
            .service
            .files(id, Some(game_version), loader_type)
            .await?;
        if files.is_empty() {
            files = self.service.files(id, Some(game_version), None).await?;
        }
        let mut files = files
            .into_iter()
            .filter(|file| {
                let (versions, loaders) = split_versions(&file.game_versions);
                let game_ok = versions.is_empty()
                    || versions
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(game_version));
                let loader_ok = loaders.is_empty()
                    || loaders
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(loader));
                game_ok && loader_ok
            })
            .collect::<Vec<_>>();
        files.sort_by(|a, b| b.file_date.cmp(&a.file_date));
        let file = files
            .iter()
            .find(|file| file.release_type == 1)
            .cloned()
            .or_else(|| files.first().cloned())
            .ok_or_else(|| {
                AppError::not_found(format!(
                    "CurseForge 没有找到支持 Minecraft {game_version} / {loader} 的文件"
                ))
            })?;
        let project = self.project(id).await?;
        Ok(self.item(project, file))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct MockAdapter {
        items: Mutex<HashMap<String, ResolvedItem>>,
        calls: Mutex<VecDeque<String>>,
    }

    #[async_trait]
    impl Adapter for MockAdapter {
        fn source(&self) -> DependencySource {
            DependencySource::Modrinth
        }
        async fn root(&self, request: &DependencyRequest) -> Result<ResolvedItem> {
            self.fetch(
                &request.project_id,
                request.version_id.as_deref(),
                "1.21.1",
                "fabric",
            )
            .await
        }
        async fn fetch(
            &self,
            project_id: &str,
            _version_id: Option<&str>,
            _game: &str,
            _loader: &str,
        ) -> Result<ResolvedItem> {
            self.calls.lock().unwrap().push_back(project_id.into());
            self.items
                .lock()
                .unwrap()
                .get(project_id)
                .cloned()
                .ok_or_else(|| AppError::not_found(format!("missing {project_id}")))
        }
    }

    fn item(id: &str, deps: Vec<(&str, DependencyRelation)>) -> ResolvedItem {
        ResolvedItem {
            source: DependencySource::Modrinth,
            project_id: id.into(),
            project_title: id.into(),
            project_slug: id.into(),
            version_id: format!("{id}-v1"),
            version_number: "1.0.0".into(),
            version_type: "release".into(),
            game_versions: vec!["1.21.1".into()],
            loaders: vec!["fabric".into()],
            file: Some(ResolvedFile {
                file_name: format!("{id}.jar"),
                url: format!("https://cdn.modrinth.com/data/{id}/x.jar"),
                size: 10,
                sha1: Some("a".repeat(40)),
                sha512: None,
            }),
            dependencies: deps
                .into_iter()
                .map(|(project, relation)| ResolvedDependency {
                    project_id: Some(project.into()),
                    version_id: None,
                    relation,
                    raw_type: relation.as_str().into(),
                })
                .collect(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn installed_states_do_not_treat_disabled_or_mismatched_as_satisfied() {
        let target = item("a", vec![]);
        let mk = |enabled: bool, version: Option<&str>| crate::dependency::InstalledMod {
            file_name: "a.jar".into(),
            mod_ids: vec!["a".into()],
            version: version.map(str::to_string),
            fingerprint: None,
            enabled,
        };
        let satisfied = mk(true, Some("1.0.0"));
        assert_eq!(
            evaluate_installed(&[&satisfied], &target).0,
            "enabled_satisfied"
        );
        let disabled = mk(false, Some("1.0.0"));
        assert_eq!(evaluate_installed(&[&disabled], &target).0, "disabled");
        let mismatch = mk(true, Some("0.9.0"));
        assert_eq!(
            evaluate_installed(&[&mismatch], &target).0,
            "version_mismatch"
        );
        let unknown = mk(true, None);
        assert_eq!(
            evaluate_installed(&[&unknown], &target).0,
            "version_unknown"
        );
        assert_eq!(evaluate_installed(&[], &target).0, "not_installed");
        let conflict = mk(true, Some("0.9.0"));
        assert_eq!(
            evaluate_installed(&[&satisfied, &conflict], &target).0,
            "conflict"
        );
    }

    #[tokio::test]
    async fn root_and_edges_use_the_canonical_node_id() {
        let adapter = adapter(vec![
            item("root", vec![("a", DependencyRelation::Required)]),
            item("a", vec![("b", DependencyRelation::Required)]),
            item("b", vec![]),
        ]);
        let analysis = analyze(
            &request("root"),
            &adapter,
            &InstalledIndex::default(),
            &ResolveLimits::default(),
        )
        .await
        .unwrap();
        let root = analysis.nodes.iter().find(|node| node.depth == 0).unwrap();
        assert_eq!(
            root.id,
            dependency_node_id("modrinth", "root", Some("root-v1"))
        );
        for edge in &analysis.edges {
            assert!(analysis.nodes.iter().any(|node| node.id == edge.from));
            assert!(analysis.nodes.iter().any(|node| node.id == edge.to));
        }
    }

    fn adapter(items: Vec<ResolvedItem>) -> MockAdapter {
        MockAdapter {
            items: Mutex::new(
                items
                    .into_iter()
                    .map(|item| (item.project_id.clone(), item))
                    .collect(),
            ),
            calls: Mutex::new(VecDeque::new()),
        }
    }

    fn request(root: &str) -> DependencyRequest {
        DependencyRequest {
            source: "modrinth".into(),
            project_id: root.into(),
            version_id: None,
            file_id: None,
            instance_id: None,
            game_version: "1.21.1".into(),
            loader: "fabric".into(),
        }
    }

    #[tokio::test]
    async fn resolves_required_and_optional_without_installing_optional() {
        let adapter = adapter(vec![
            item(
                "root",
                vec![
                    ("req", DependencyRelation::Required),
                    ("opt", DependencyRelation::Optional),
                ],
            ),
            item("req", vec![]),
            item("opt", vec![]),
        ]);
        let analysis = analyze(
            &request("root"),
            &adapter,
            &InstalledIndex::default(),
            &ResolveLimits::default(),
        )
        .await
        .unwrap();
        assert_eq!(analysis.nodes.len(), 3);
        assert_eq!(analysis.stats.required, 2);
        assert_eq!(analysis.stats.optional, 1);
        assert_eq!(analysis.default_plan.len(), 2);
        assert!(analysis
            .nodes
            .iter()
            .find(|node| node.project_id == "opt")
            .is_some_and(|node| !node.selected_by_default));
    }

    #[tokio::test]
    async fn detects_cycles_and_deduplicates_nodes() {
        let adapter = adapter(vec![
            item("root", vec![("a", DependencyRelation::Required)]),
            item("a", vec![("root", DependencyRelation::Required)]),
        ]);
        let analysis = analyze(
            &request("root"),
            &adapter,
            &InstalledIndex::default(),
            &ResolveLimits::default(),
        )
        .await
        .unwrap();
        assert_eq!(analysis.nodes.len(), 2);
        assert!(analysis.complete);
    }

    #[tokio::test]
    async fn missing_required_dependency_blocks_install() {
        let adapter = adapter(vec![item(
            "root",
            vec![("missing", DependencyRelation::Required)],
        )]);
        let analysis = analyze(
            &request("root"),
            &adapter,
            &InstalledIndex::default(),
            &ResolveLimits::default(),
        )
        .await
        .unwrap();
        assert!(!analysis.complete);
        assert!(!analysis.blockers.is_empty());
        assert!(analysis
            .nodes
            .iter()
            .any(|node| node.project_id == "missing"));
    }

    #[tokio::test]
    async fn incompatible_relation_blocks_but_stays_visible() {
        let adapter = adapter(vec![
            item("root", vec![("bad", DependencyRelation::Incompatible)]),
            item("bad", vec![]),
        ]);
        let analysis = analyze(
            &request("root"),
            &adapter,
            &InstalledIndex::default(),
            &ResolveLimits::default(),
        )
        .await
        .unwrap();
        assert_eq!(analysis.stats.incompatible, 1);
        assert!(!analysis.blockers.is_empty());
        assert!(
            !analysis
                .nodes
                .iter()
                .find(|node| node.project_id == "bad")
                .unwrap()
                .selected_by_default
        );
    }

    #[tokio::test]
    async fn stops_at_depth_limit() {
        let adapter = adapter(vec![
            item("root", vec![("a", DependencyRelation::Required)]),
            item("a", vec![("b", DependencyRelation::Required)]),
            item("b", vec![("c", DependencyRelation::Required)]),
            item("c", vec![]),
        ]);
        let limits = ResolveLimits {
            max_depth: 1,
            ..ResolveLimits::default()
        };
        let analysis = analyze(
            &request("root"),
            &adapter,
            &InstalledIndex::default(),
            &limits,
        )
        .await
        .unwrap();
        assert!(!analysis.complete);
        assert!(analysis.warnings.iter().any(|value| value.contains("层级")));
    }

    #[test]
    fn maps_curseforge_relations_1_to_6() {
        assert_eq!(map_curseforge_relation(1), DependencyRelation::Embedded);
        assert_eq!(map_curseforge_relation(2), DependencyRelation::Optional);
        assert_eq!(map_curseforge_relation(3), DependencyRelation::Required);
        assert_eq!(map_curseforge_relation(4), DependencyRelation::Tool);
        assert_eq!(map_curseforge_relation(5), DependencyRelation::Incompatible);
        assert_eq!(map_curseforge_relation(6), DependencyRelation::Include);
        assert_eq!(map_curseforge_relation(99), DependencyRelation::Unknown);
    }

    #[test]
    fn maps_modrinth_relations() {
        assert_eq!(modrinth_relation("required"), DependencyRelation::Required);
        assert_eq!(modrinth_relation("optional"), DependencyRelation::Optional);
        assert_eq!(
            modrinth_relation("incompatible"),
            DependencyRelation::Incompatible
        );
        assert_eq!(modrinth_relation("embedded"), DependencyRelation::Embedded);
        assert_eq!(modrinth_relation("unknown"), DependencyRelation::Unknown);
    }
}
