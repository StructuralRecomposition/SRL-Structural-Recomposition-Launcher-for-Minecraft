//! Deterministic mod dependency analysis.
//!
//! All data comes from the official Modrinth and CurseForge APIs plus locally
//! installed JAR metadata. No model is called here, and unknown relations stay
//! unknown instead of being promoted to required dependencies.

pub mod apply;
pub mod installed;
pub mod model;
pub mod resolve;

pub use apply::{install_analysis, InstallSelection};
pub use installed::{scan_installed, InstalledIndex, InstalledMod};
pub use model::{
    DependencyAnalysis, DependencyEdge, DependencyNode, DependencyRelation, DependencyRequest,
    DependencySource, DependencyStats, PlannedEntry, ResolvedDependency, ResolvedFile,
    ResolvedItem,
};
pub use resolve::{analyze, Adapter, ResolveLimits};
