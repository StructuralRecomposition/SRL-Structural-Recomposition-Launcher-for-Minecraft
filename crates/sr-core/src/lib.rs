//! SR Launcher core: framework-independent business logic.
//!
//! The Tauri layer must stay thin: every behaviour that touches files, network,
//! processes or credentials lives here and is testable without a GUI.

pub mod account;
pub mod assistant;
pub mod assistant_history;
pub mod crash;
pub mod curseforge;
pub mod diagnostics;
pub mod download;
pub mod error;
pub mod fabric;
pub mod install;
pub mod installer;
pub mod instance;
pub mod iris;
pub mod java;
pub mod java_runtime;
pub mod launch;
pub mod microsoft;
pub mod mod_install;
pub mod modrinth;
pub mod mods;
pub mod multiplayer;
pub mod paths;
pub mod recommend;
pub mod repair;
pub mod resourcepacks;
pub mod resources;
pub mod settings;
pub mod shaders;
pub mod skin;
pub mod transfer;
pub mod util;
pub mod version;
pub mod worlds;

pub use error::{AppError, ErrorCode, Result};

/// User agent used for every outbound request.
pub const USER_AGENT: &str = concat!("SRLauncher/", env!("CARGO_PKG_VERSION"));
