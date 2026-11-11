use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyRelation {
    Required,
    Optional,
    Incompatible,
    Embedded,
    Tool,
    Include,
    Unknown,
}

impl DependencyRelation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::Incompatible => "incompatible",
            Self::Embedded => "embedded",
            Self::Tool => "tool",
            Self::Include => "include",
            Self::Unknown => "unknown",
        }
    }

    /// Only required dependencies are installed automatically. Optional ones
    /// are resolved and shown but require an explicit user choice.
    pub fn auto_install(self) -> bool {
        self == Self::Required
    }

    /// Relations that make the current target impossible to install safely.
    pub fn blocks_install(self) -> bool {
        self == Self::Incompatible
    }

    pub fn is_recursive(self) -> bool {
        matches!(self, Self::Required | Self::Optional)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencySource {
    Modrinth,
    CurseForge,
}

impl DependencySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Modrinth => "modrinth",
            Self::CurseForge => "curseforge",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "modrinth" => Some(Self::Modrinth),
            "curseforge" => Some(Self::CurseForge),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyRequest {
    pub source: String,
    pub project_id: String,
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub instance_id: Option<String>,
    pub game_version: String,
    pub loader: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFile {
    pub file_name: String,
    pub url: String,
    pub size: u64,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDependency {
    /// Project id on the platform that produced this relation.
    pub project_id: Option<String>,
    /// Exact version id when the platform provides one. CurseForge relation
    /// entries only carry a mod id, so this stays `None` there.
    pub version_id: Option<String>,
    pub relation: DependencyRelation,
    /// Raw platform value (`required`, `embedded`, `3`, ...). Kept for audit.
    pub raw_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedItem {
    pub source: DependencySource,
    pub project_id: String,
    pub project_title: String,
    pub project_slug: String,
    pub version_id: String,
    pub version_number: String,
    pub version_type: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub file: Option<ResolvedFile>,
    pub dependencies: Vec<ResolvedDependency>,
    pub notes: Vec<String>,
}

impl ResolvedItem {
    pub fn supports(&self, game_version: &str, loader: &str) -> bool {
        let game_ok = self.game_versions.is_empty()
            || self
                .game_versions
                .iter()
                .any(|value| value.eq_ignore_ascii_case(game_version));
        let loader_ok = self.loaders.is_empty()
            || self
                .loaders
                .iter()
                .any(|value| value.eq_ignore_ascii_case(loader));
        game_ok && loader_ok
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyNode {
    /// Stable within one analysis: `<source>:<project>`.
    pub id: String,
    pub source: String,
    pub project_id: String,
    pub title: String,
    pub version_id: Option<String>,
    pub version_number: Option<String>,
    pub version_type: Option<String>,
    pub file_name: Option<String>,
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub size: u64,
    pub has_checksum: bool,
    pub relation: DependencyRelation,
    pub depth: u32,
    pub parent: Option<String>,
    pub required_by: Vec<String>,
    pub installed: bool,
    pub installed_file: Option<String>,
    pub installed_version: Option<String>,
    pub compatible: bool,
    pub installable: bool,
    pub selected_by_default: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub relation: DependencyRelation,
    pub raw_type: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyStats {
    pub nodes: usize,
    pub required: usize,
    pub optional: usize,
    pub incompatible: usize,
    pub installed: usize,
    pub to_install: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedEntry {
    pub node_id: String,
    pub title: String,
    pub file_name: String,
    pub size: u64,
    pub relation: DependencyRelation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyAnalysis {
    pub id: String,
    pub created_at: String,
    pub source: String,
    pub root_project_id: String,
    pub root_version_id: Option<String>,
    pub game_version: String,
    pub loader: String,
    pub instance_id: Option<String>,
    pub instance_name: Option<String>,
    pub nodes: Vec<DependencyNode>,
    pub edges: Vec<DependencyEdge>,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
    pub stats: DependencyStats,
    /// Files that would be downloaded for the default selection. Optional
    /// nodes remain visible but are not included until the user ticks them.
    pub default_plan: Vec<PlannedEntry>,
    /// Analysis honesty marker: true when every relation came from an API
    /// response, false when part of the graph could not be read.
    pub complete: bool,
}
