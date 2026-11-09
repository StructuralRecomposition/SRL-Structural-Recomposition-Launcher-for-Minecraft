use crate::state::AppState;
use serde::Serialize;
use sr_core::account::{Account, AccountView, KeyringStore};
use sr_core::assistant;
use sr_core::assistant_history;
use sr_core::crash::{self as crash_service, CrashAnalysis, CrashFile};
use sr_core::curseforge::{self as curseforge_service, CurseForgeFile, CurseForgeService};
use sr_core::diagnostics::{self, DiagnosticsReport, LogFileInfo};
use sr_core::download::TaskView;
use sr_core::error::{AppError, ErrorCode, Result};
use sr_core::fabric::{FabricLoader, FabricService, LoaderKind};
use sr_core::installer::{self as installer_service, InstallerKind};
use sr_core::instance::{CreateInstanceRequest, Instance, InstanceManager, UpdateInstanceRequest};
use sr_core::iris;
use sr_core::java::{self, JavaCheck, JavaInfo};
use sr_core::java_runtime;
use sr_core::launch::{preflight, LaunchResult};
use sr_core::microsoft::{
    load_tokens, player_from_tokens, save_tokens, DeviceCodeView, MicrosoftAuth, MicrosoftPoll,
};
use sr_core::mod_install;
use sr_core::modpack_export::{self, ModpackExportSummary};
use sr_core::modrinth::{
    ResourceHit, ResourceProject, ResourceSearchPage, ResourceSearchQuery, ResourceVersion,
};
use sr_core::mods as mods_service;
use sr_core::mods::ModEntry;
use sr_core::multiplayer::{
    AccountView as MultiplayerAccount, CreateRoomRequest, Edition, FrpcView, LocalServiceView,
    MultiplayerStatus, NodeView, OrphanTunnel, RoomView, StopRoomResult,
};
use sr_core::paths::GamePaths;
use sr_core::recommend::{self as recommend_service, Recommendations};
use sr_core::repair;
use sr_core::resourcepacks::{self as resourcepack_service, ResourcePackEntry};
use sr_core::resources::{self as resource_install, ResourceTarget};
use sr_core::settings::{self, Settings};
use sr_core::shaders::{self as shader_service, ShaderEntry, ShaderSupport};
use sr_core::skin::{self as skin_service, SkinModel, SkinView};
use sr_core::transfer::{self, ExportSummary, ImportPreview};
use sr_core::version::{
    ManifestResult, ManifestSource, ManifestVersion, RuleContext, VersionDeletePlan,
    VersionDeleteResult,
};
use sr_core::worlds::{self as world_service, WorldEntry};
use std::path::{Path, PathBuf};
use tauri::State;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureState {
    pub id: String,
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathsView {
    pub data_dir: PathBuf,
    pub game_dir: PathBuf,
    pub logs_dir: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub app_version: String,
    pub platform: String,
    pub settings: Settings,
    pub paths: PathsView,
    pub instances: Vec<Instance>,
    pub accounts: Vec<AccountView>,
    pub java: Vec<JavaInfo>,
    pub installed_versions: Vec<sr_core::version::InstalledVersion>,
    pub running_instances: Vec<String>,
    pub features: Vec<FeatureState>,
}

#[tauri::command]
pub async fn get_bootstrap(state: State<'_, AppState>) -> Result<Bootstrap> {
    let settings = state.settings_snapshot().await;
    let game = state.game_snapshot().await;
    let instances = InstanceManager::new(game.clone()).list().await?;
    let accounts = state.accounts.list().await?;
    let java = state.java.read().await.clone();
    let installed_versions = state.versions.installed_versions(&game).await?;
    let running_instances = state.launches.read().await.running_instances().await;
    Ok(Bootstrap {
        app_version: env!("CARGO_PKG_VERSION").into(),
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        settings: settings.clone(),
        paths: PathsView {
            data_dir: state.paths.base().to_path_buf(),
            game_dir: game.root().to_path_buf(),
            logs_dir: state.paths.logs_dir(),
        },
        instances,
        accounts,
        java,
        installed_versions,
        running_instances,
        features: vec![
            FeatureState {
                id: "offline_accounts".into(),
                available: true,
                reason: None,
            },
            FeatureState {
                id: "microsoft_login".into(),
                available: settings
                    .microsoft_client_id
                    .as_deref()
                    .is_some_and(|id| !id.trim().is_empty()),
                reason: if settings
                    .microsoft_client_id
                    .as_deref()
                    .is_some_and(|id| !id.trim().is_empty())
                {
                    None
                } else {
                    Some("在设置中填写 Azure 应用的公开 client id 后启用".into())
                },
            },
            FeatureState {
                id: "mod_loaders".into(),
                available: true,
                reason: None,
            },
            FeatureState {
                id: "multiplayer".into(),
                available: true,
                reason: None,
            },
            FeatureState {
                id: "modpacks".into(),
                available: false,
                reason: Some("安全导入将在阶段 6 实现".into()),
            },
            FeatureState {
                id: "auto_update".into(),
                available: false,
                reason: Some("更新签名公钥未配置".into()),
            },
        ],
    })
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings> {
    Ok(state.settings_snapshot().await)
}

#[tauri::command]
pub async fn update_settings(state: State<'_, AppState>, settings: Settings) -> Result<Settings> {
    settings.validate()?;
    let game = GamePaths::new(settings.game_dir.clone());
    game.ensure_layout().await?;
    settings::save(&state.paths, &settings).await?;
    *state.game.write().await = game;
    *state.settings.write().await = settings.clone();
    state.downloads.set_limit(settings.max_concurrent_downloads);
    state.rebuild_launch_manager().await;
    tracing::info!(game_dir = %settings.game_dir.display(), "settings updated");
    Ok(settings)
}

// ---------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionList {
    pub latest_release: String,
    pub latest_snapshot: String,
    pub versions: Vec<ManifestVersion>,
    pub installed: Vec<String>,
    pub source: ManifestSource,
    pub warning: Option<String>,
}

#[tauri::command]
pub async fn list_versions(state: State<'_, AppState>, refresh: bool) -> Result<VersionList> {
    let result: ManifestResult = state.versions.manifest(&state.paths, refresh).await?;
    let game = state.game_snapshot().await;
    let installed = state
        .versions
        .installed_versions(&game)
        .await?
        .into_iter()
        .map(|v| v.id)
        .collect();
    Ok(VersionList {
        latest_release: result.manifest.latest.release,
        latest_snapshot: result.manifest.latest.snapshot,
        versions: result.manifest.versions,
        installed,
        source: result.source,
        warning: result.warning,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallAccepted {
    pub task_id: String,
    pub version_id: String,
    pub include_assets: bool,
}

#[tauri::command]
pub async fn install_version(
    state: State<'_, AppState>,
    version_id: String,
    include_assets: Option<bool>,
) -> Result<InstallAccepted> {
    if version_id.is_empty() || version_id.len() > 64 {
        return Err(AppError::invalid_input("版本 ID 非法"));
    }
    {
        let mut installing = state.installing.lock().await;
        if !installing.insert(version_id.clone()) {
            return Err(AppError::new(ErrorCode::AlreadyExists, "该版本正在安装中"));
        }
    }

    let settings = state.settings_snapshot().await;
    let include_assets = include_assets.unwrap_or(settings.download_assets_on_install);
    let options = sr_core::install::InstallOptions {
        include_assets,
        concurrency: settings.max_concurrent_downloads,
        allow_custom_hosts: settings.allow_custom_hosts,
    };
    let task = state
        .downloads
        .create("install", &format!("安装 Minecraft {version_id}"), 0, 0)
        .await;
    let task_id = task.id.clone();
    let _ = state.downloads.save().await;

    let state = state.inner().clone();
    let version_id_for_task = version_id.clone();
    tauri::async_runtime::spawn(async move {
        let game = state.game_snapshot().await;
        let installer = sr_core::install::Installer {
            versions: &state.versions,
            http: &state.http,
            paths: &state.paths,
            game: &game,
            downloads: &state.downloads,
            prepared: None,
            options,
        };
        let result = installer.install(task, &version_id_for_task).await;
        if let Err(e) = &result {
            tracing::warn!(version = %version_id_for_task, error = %e, "install failed");
        }
        state.installing.lock().await.remove(&version_id_for_task);
    });

    Ok(InstallAccepted {
        task_id,
        version_id,
        include_assets,
    })
}

#[tauri::command]
pub async fn list_tasks(state: State<'_, AppState>) -> Result<Vec<TaskView>> {
    Ok(state.downloads.list().await)
}

#[tauri::command]
pub async fn cancel_task(state: State<'_, AppState>, task_id: String) -> Result<()> {
    state.downloads.cancel(&task_id).await?;
    let _ = state.downloads.save().await;
    Ok(())
}

#[tauri::command]
pub async fn clear_finished_tasks(state: State<'_, AppState>) -> Result<()> {
    state.downloads.clear_finished().await;
    let _ = state.downloads.save().await;
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionDetail {
    pub id: String,
    pub main_class: String,
    pub version_type: String,
    pub java_major: u32,
    pub library_count: usize,
    pub native_count: usize,
    pub legacy_arguments: bool,
    pub installed: bool,
    pub issues: Vec<String>,
    pub java_check: Option<JavaCheck>,
}

#[tauri::command]
pub async fn get_version_detail(
    state: State<'_, AppState>,
    version_id: String,
) -> Result<VersionDetail> {
    let game = state.game_snapshot().await;
    let resolved = state.versions.load_installed(&game, &version_id).await?;
    let ctx = RuleContext::default();
    let libs = resolved.effective_libraries(&ctx);
    let issues = preflight(&game, &state.versions, &resolved, &ctx).await?;
    let javas = state.java.read().await.clone();
    let java_check = java::select(&javas, resolved.java_major(), None)
        .map(|j| java::check_compatibility(resolved.java_major(), j.major));
    Ok(VersionDetail {
        id: resolved.id.clone(),
        main_class: resolved.main_class().unwrap_or("").to_string(),
        version_type: resolved
            .version_type
            .clone()
            .unwrap_or_else(|| "unknown".into()),
        java_major: resolved.java_major(),
        library_count: libs.len(),
        native_count: libs.iter().filter(|l| l.is_native).count(),
        legacy_arguments: resolved.uses_legacy_arguments(),
        installed: issues.is_empty(),
        issues,
        java_check,
    })
}

#[tauri::command]
pub async fn verify_version(
    state: State<'_, AppState>,
    version_id: String,
    deep_assets: Option<bool>,
) -> Result<sr_core::version::VerificationReport> {
    let game = state.game_snapshot().await;
    let resolved = state.versions.load_installed(&game, &version_id).await?;
    // Only evaluate asset objects if at least one asset was installed.
    let has_assets = match tokio::fs::read_dir(game.objects_dir()).await {
        Ok(mut rd) => rd.next_entry().await.ok().flatten().is_some(),
        Err(_) => false,
    };
    state
        .versions
        .verify_installed(
            &game,
            &resolved,
            has_assets && deep_assets.unwrap_or(false),
            has_assets,
            &RuleContext::default(),
        )
        .await
}

#[tauri::command]
pub async fn get_version_delete_plan(
    state: State<'_, AppState>,
    version_id: String,
) -> Result<VersionDeletePlan> {
    let game = state.game_snapshot().await;
    let instances = InstanceManager::new(game.clone()).list().await?;
    state
        .versions
        .delete_plan(&game, &instances, &version_id)
        .await
}

#[tauri::command]
pub async fn delete_version(
    state: State<'_, AppState>,
    version_id: String,
    force: Option<bool>,
) -> Result<VersionDeleteResult> {
    let game = state.game_snapshot().await;
    let instances = InstanceManager::new(game.clone()).list().await?;
    state
        .versions
        .delete_installed(&game, &instances, &version_id, force.unwrap_or(false))
        .await
}

/// Open a known launcher directory in the system file manager.
///
/// The frontend can only choose from this fixed list; it cannot pass an
/// arbitrary filesystem path to be opened.
#[tauri::command]
pub async fn open_launcher_path(
    state: State<'_, AppState>,
    target: String,
    id: Option<String>,
) -> Result<()> {
    let game = state.game_snapshot().await;
    let path = match target.as_str() {
        "gameDir" => game.root().to_path_buf(),
        "versionsDir" => game.versions_dir(),
        "versionDir" => {
            let version_id = id.ok_or_else(|| AppError::invalid_input("缺少版本 ID"))?;
            if sr_core::util::safe_component(&version_id, "") != version_id || version_id.is_empty()
            {
                return Err(AppError::invalid_input("版本 ID 非法"));
            }
            let dir = game.version_dir(&version_id);
            let meta = tokio::fs::symlink_metadata(&dir).await.map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    AppError::not_found(format!("版本 {version_id} 不在本地"))
                } else {
                    e.into()
                }
            })?;
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(AppError::path_unsafe(
                    "版本目录是符号链接或非目录，已拒绝打开",
                ));
            }
            dir
        }
        "instanceDir"
        | "instanceMods"
        | "instanceShaderpacks"
        | "instanceSaves"
        | "instanceResourcepacks" => {
            let instance_id = id.ok_or_else(|| AppError::invalid_input("缺少实例 ID"))?;
            let instance = InstanceManager::new(game.clone()).get(&instance_id).await?;
            let dir = match target.as_str() {
                "instanceDir" => instance.game_dir,
                "instanceMods" => instance.game_dir.join("mods"),
                "instanceShaderpacks" => instance.game_dir.join("shaderpacks"),
                "instanceResourcepacks" => instance.game_dir.join("resourcepacks"),
                _ => instance.game_dir.join("saves"),
            };
            sr_core::util::ensure_dir(&dir).await?;
            dir
        }
        _ => return Err(AppError::invalid_input("不支持打开的目录类型")),
    };
    sr_core::util::ensure_dir(&path).await?;
    open_directory(&path).await
}

async fn open_directory(path: &Path) -> Result<()> {
    if !tokio::fs::metadata(path)
        .await
        .map(|m| m.is_dir())
        .unwrap_or(false)
    {
        return Err(AppError::not_found("目录不存在"));
    }
    #[cfg(target_os = "macos")]
    let mut command = tokio::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = tokio::process::Command::new("explorer");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = tokio::process::Command::new("xdg-open");
    command.arg(path);
    let status = command.status().await.map_err(|e| {
        AppError::new(ErrorCode::Io, "无法调用系统文件管理器").with_detail(e.to_string())
    })?;
    // explorer.exe commonly returns 1 even when it successfully opens a folder.
    #[cfg(target_os = "windows")]
    let _ = status;
    #[cfg(not(target_os = "windows"))]
    if !status.success() {
        return Err(AppError::new(ErrorCode::Io, "系统文件管理器返回失败状态"));
    }
    Ok(())
}

#[tauri::command]
pub async fn list_installed_versions(
    state: State<'_, AppState>,
) -> Result<Vec<sr_core::version::InstalledVersion>> {
    let game = state.game_snapshot().await;
    state.versions.installed_versions(&game).await
}

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_instances(state: State<'_, AppState>) -> Result<Vec<Instance>> {
    let game = state.game_snapshot().await;
    InstanceManager::new(game).list().await
}

#[tauri::command]
pub async fn create_instance(
    state: State<'_, AppState>,
    request: CreateInstanceRequest,
) -> Result<Instance> {
    let game = state.game_snapshot().await;
    InstanceManager::new(game).create(request).await
}

#[tauri::command]
pub async fn update_instance(
    state: State<'_, AppState>,
    instance_id: String,
    request: UpdateInstanceRequest,
) -> Result<Instance> {
    let game = state.game_snapshot().await;
    let inst = InstanceManager::new(game)
        .update(&instance_id, request)
        .await?;
    Ok(inst)
}

#[tauri::command]
pub async fn delete_instance(
    state: State<'_, AppState>,
    instance_id: String,
    delete_files: bool,
) -> Result<()> {
    let game = state.game_snapshot().await;
    InstanceManager::new(game)
        .delete(&instance_id, delete_files)
        .await
}

#[tauri::command]
pub async fn duplicate_instance(
    state: State<'_, AppState>,
    instance_id: String,
    new_name: Option<String>,
) -> Result<Instance> {
    let game = state.game_snapshot().await;
    InstanceManager::new(game)
        .duplicate(&instance_id, new_name)
        .await
}

#[tauri::command]
pub async fn launch_instance(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<LaunchResult> {
    let game = state.game_snapshot().await;
    let manager = InstanceManager::new(game.clone());
    let instance = manager.get(&instance_id).await?;
    if state.launches.read().await.is_running(&instance_id).await {
        return Err(AppError::new(ErrorCode::AlreadyExists, "该实例已在运行"));
    }
    let account =
        match &instance.account_id {
            Some(id) => state.accounts.get(id).await.map_err(|_| {
                AppError::new(
                    ErrorCode::AuthenticationRequired,
                    "实例指定的账号已被移除，请重新选择账号",
                )
            })?,
            None => state.accounts.active().await?.ok_or_else(|| {
                AppError::new(ErrorCode::AuthenticationRequired, "请先选择一个账号")
            })?,
        };
    let resolved = state
        .versions
        .load_installed(&game, &instance.version_id)
        .await?;
    let required = resolved.java_major();
    let settings = state.settings_snapshot().await;
    let javas = state.java.read().await.clone();
    // Per-instance selection is explicit and always wins. The global default is
    // only a preference: java::select refuses to use it when it is too new for
    // the target version (e.g. Java 25 for a Java 21 Forge pack).
    let selected_java = match instance.java_path.clone() {
        Some(path) => java::probe(&path, "manual").await?,
        None => java::select_for_launch(&javas, required, settings.java_path.as_deref())
            .ok_or_else(|| {
                let max = java::max_supported_major(required);
                let installed = if javas.is_empty() {
                    "未检测到任何 Java".to_string()
                } else {
                    format!(
                        "已检测到：{}",
                        javas
                            .iter()
                            .map(|j| format!("Java {}（{}）", j.major, j.version_string))
                            .collect::<Vec<_>>()
                            .join("、")
                    )
                };
                AppError::new(
                    ErrorCode::JavaInvalid,
                    format!(
                        "该版本需要 Java {required}（兼容范围 {required}–{max}），但没有找到可安全使用的 Java。"
                    ),
                )
                .with_detail(format!(
                    "{installed}。请到「Java 环境」安装 Java {required}，或在实例设置中手动选择。"
                ))
            })?
            .clone(),
    };
    let player = match account.kind {
        sr_core::account::AccountKind::Offline => sr_core::account::offline_player(&account)?,
        sr_core::account::AccountKind::Microsoft => {
            let store = KeyringStore::new("sr-launcher");
            let mut tokens = load_tokens(&store, &account.uuid)?.ok_or_else(|| {
                AppError::new(
                    ErrorCode::AuthenticationRequired,
                    "该 Microsoft 账号没有可用令牌，请重新登录",
                )
            })?;
            if tokens.mc_expires_at <= chrono::Utc::now().timestamp() {
                let client_id = settings
                    .microsoft_client_id
                    .clone()
                    .filter(|id| !id.trim().is_empty())
                    .ok_or_else(|| {
                        AppError::new(
                            ErrorCode::NotConfigured,
                            "Microsoft 登录已过期，且未配置 client id，无法刷新",
                        )
                    })?;
                let session = MicrosoftAuth::new(state.http.clone(), client_id)
                    .refresh(&tokens.refresh_token)
                    .await?;
                tokens = save_tokens(&store, &account.uuid, &session)?;
            }
            player_from_tokens(&account, &tokens)
        }
    };
    // Offline skins are applied as a client-side resource pack. Microsoft
    // accounts keep their official skin (uploaded separately) and must not be
    // shadowed by the local pack.
    let mut skin_warning: Option<String> = None;
    let local_skin =
        if account.kind == sr_core::account::AccountKind::Offline && settings.apply_offline_skin {
            match account.skin_id.as_deref() {
                Some(id) => match state.skins.get(id).await {
                    Ok(skin) => match state.skins.compat_bytes(&skin).await {
                        Ok(bytes) => Some(bytes),
                        Err(e) => {
                            skin_warning = Some(format!("无法读取皮肤文件：{}", e.message));
                            None
                        }
                    },
                    Err(e) => {
                        skin_warning = Some(format!("账号引用的皮肤已不存在：{}", e.message));
                        None
                    }
                },
                None => None,
            }
        } else {
            None
        };
    let pack_format = skin_service::detect_pack_format(&game.version_jar(&resolved.jar_id()))
        .unwrap_or_else(|| skin_service::pack_format_for(&instance.version_id));
    if let Err(e) =
        skin_service::apply_local_skin(&instance.game_dir, local_skin.as_deref(), pack_format).await
    {
        skin_warning = Some(format!("本地皮肤资源包未能应用：{}", e.message));
    }

    let mut result = state
        .launches
        .read()
        .await
        .launch(&instance, &player, &selected_java)
        .await?;
    if let Some(warning) = skin_warning {
        result.warnings.push(warning);
    }
    manager.touch_launched(&instance_id).await?;
    Ok(result)
}

#[tauri::command]
pub async fn stop_instance(state: State<'_, AppState>, instance_id: String) -> Result<()> {
    state.launches.read().await.stop(&instance_id).await
}

#[tauri::command]
pub async fn running_instances(state: State<'_, AppState>) -> Result<Vec<String>> {
    Ok(state.launches.read().await.running_instances().await)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCheck {
    #[serde(flatten)]
    pub preview: ImportPreview,
    pub version_installed: bool,
}

#[tauri::command]
pub async fn export_instance_archive(
    state: State<'_, AppState>,
    instance_id: String,
    dest_path: String,
    overwrite: bool,
) -> Result<ExportSummary> {
    if dest_path.trim().is_empty() {
        return Err(AppError::invalid_input("请选择导出文件路径"));
    }
    let game = state.game_snapshot().await;
    let manager = InstanceManager::new(game.clone());
    let instance = manager.get(&instance_id).await?;
    transfer::export_instance(&game, &instance, &PathBuf::from(dest_path), overwrite).await
}

#[tauri::command]
pub async fn export_instance_modpack(
    state: State<'_, AppState>,
    instance_id: String,
    dest_path: String,
    overwrite: bool,
    name: Option<String>,
    version_id: Option<String>,
) -> Result<ModpackExportSummary> {
    if dest_path.trim().is_empty() {
        return Err(AppError::invalid_input("请选择导出文件路径"));
    }
    let game = state.game_snapshot().await;
    let manager = InstanceManager::new(game.clone());
    let instance = manager.get(&instance_id).await?;
    modpack_export::export_mrpack(
        &game,
        &instance,
        &PathBuf::from(dest_path),
        overwrite,
        name,
        version_id,
        &state.http,
    )
    .await
}

#[tauri::command]
pub async fn inspect_instance_archive(
    state: State<'_, AppState>,
    path: String,
) -> Result<ImportCheck> {
    let preview = transfer::inspect_archive(&PathBuf::from(path)).await?;
    let game = state.game_snapshot().await;
    let version_installed = tokio::fs::try_exists(game.version_json(&preview.manifest.version_id))
        .await
        .unwrap_or(false);
    Ok(ImportCheck {
        preview,
        version_installed,
    })
}

#[tauri::command]
pub async fn import_instance_archive(
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> Result<Instance> {
    let game = state.game_snapshot().await;
    let instance = transfer::import_instance(&game, &PathBuf::from(path), name).await?;
    tracing::info!(instance = %instance.id, version = %instance.version_id, "instance imported");
    Ok(instance)
}

#[tauri::command]
pub async fn list_mods(state: State<'_, AppState>, instance_id: String) -> Result<Vec<ModEntry>> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    mods_service::list(&instance).await
}

#[tauri::command]
pub async fn toggle_mod(
    state: State<'_, AppState>,
    instance_id: String,
    file_name: String,
    enabled: bool,
) -> Result<ModEntry> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    mods_service::toggle(&instance, &file_name, enabled).await
}

#[tauri::command]
pub async fn delete_mod(
    state: State<'_, AppState>,
    instance_id: String,
    file_name: String,
) -> Result<()> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    mods_service::delete(&instance, &file_name).await
}

#[tauri::command]
pub async fn import_mod(
    state: State<'_, AppState>,
    instance_id: String,
    source_path: String,
    overwrite: bool,
) -> Result<ModEntry> {
    if source_path.trim().is_empty() {
        return Err(AppError::invalid_input("请选择模组文件路径"));
    }
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    mods_service::import(&instance, &PathBuf::from(source_path), overwrite).await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub file_name: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// Import one or more local mod jars (used for native drag & drop, where Tauri
/// provides absolute paths).
#[tauri::command]
pub async fn import_mods(
    state: State<'_, AppState>,
    instance_id: String,
    source_paths: Vec<String>,
    overwrite: bool,
) -> Result<Vec<ImportOutcome>> {
    if source_paths.is_empty() {
        return Err(AppError::invalid_input("没有可导入的文件"));
    }
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    let mut out = Vec::new();
    for source in source_paths {
        let path = PathBuf::from(&source);
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| source.clone());
        match mods_service::import(&instance, &path, overwrite).await {
            Ok(entry) => out.push(ImportOutcome {
                file_name: entry.file_name,
                ok: true,
                error: None,
            }),
            Err(e) => out.push(ImportOutcome {
                file_name,
                ok: false,
                error: Some(e.message),
            }),
        }
    }
    Ok(out)
}

/// Import a mod chosen through the webview file picker. The bytes travel as a
/// raw IPC payload (not JSON/base64), with metadata in headers.
#[tauri::command]
pub async fn import_mod_bytes(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<ModEntry> {
    let instance_id = request_header(&request, "x-sr-instance")?.to_string();
    let file_name = decode_header_name(request_header(&request, "x-sr-name")?)?;
    let overwrite = request_header(&request, "x-sr-overwrite").unwrap_or("0") == "1";
    let bytes = request_bytes(&request)?;
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    mods_service::import_bytes(&instance, &file_name, bytes, overwrite).await
}

#[tauri::command]
pub async fn list_shaders(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<ShaderEntry>> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    shader_service::list(&instance).await
}

#[tauri::command]
pub async fn toggle_shader(
    state: State<'_, AppState>,
    instance_id: String,
    file_name: String,
    enabled: bool,
) -> Result<ShaderEntry> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    shader_service::toggle(&instance, &file_name, enabled).await
}

#[tauri::command]
pub async fn delete_shader(
    state: State<'_, AppState>,
    instance_id: String,
    file_name: String,
) -> Result<()> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    shader_service::delete(&instance, &file_name).await
}

#[tauri::command]
pub async fn import_shaders(
    state: State<'_, AppState>,
    instance_id: String,
    source_paths: Vec<String>,
    overwrite: bool,
) -> Result<Vec<ImportOutcome>> {
    if source_paths.is_empty() {
        return Err(AppError::invalid_input("没有可导入的文件"));
    }
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    let mut out = Vec::new();
    for source in source_paths {
        let path = PathBuf::from(&source);
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| source.clone());
        match shader_service::import(&instance, &path, overwrite).await {
            Ok(entry) => out.push(ImportOutcome {
                file_name: entry.file_name,
                ok: true,
                error: None,
            }),
            Err(e) => out.push(ImportOutcome {
                file_name,
                ok: false,
                error: Some(e.message),
            }),
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn import_shader_bytes(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<ShaderEntry> {
    let instance_id = request_header(&request, "x-sr-instance")?.to_string();
    let file_name = decode_header_name(request_header(&request, "x-sr-name")?)?;
    let overwrite = request_header(&request, "x-sr-overwrite").unwrap_or("0") == "1";
    let bytes = request_bytes(&request)?;
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    shader_service::import_bytes(&instance, &file_name, bytes, overwrite).await
}

#[tauri::command]
pub async fn shader_support(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<ShaderSupport> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    shader_service::support(&instance).await
}

/// Install the latest compatible Iris jar from Modrinth into the instance's
/// mods folder. Requires a Fabric/Quilt instance and a resolved base version.
#[tauri::command]
pub async fn install_iris(state: State<'_, AppState>, instance_id: String) -> Result<ModEntry> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game.clone()).get(&instance_id).await?;
    let loader = instance
        .mod_loader
        .as_ref()
        .map(|m| m.kind.to_ascii_lowercase())
        .or_else(|| {
            if instance.version_id.starts_with("fabric-loader-") {
                Some("fabric".into())
            } else if instance.version_id.starts_with("quilt-loader-") {
                Some("quilt".into())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::Unsupported,
                "Iris 需要 Fabric 或 Quilt 实例，请先安装模组加载器",
            )
        })?;
    let game_version = state
        .versions
        .base_version_of(&game, &instance.version_id)
        .await?
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::VersionInvalid,
                "无法从加载器版本解析基础 Minecraft 版本",
            )
        })?;
    iris::install_latest(&state.http, &instance, &game_version, &loader).await
}

/// Install OptiFine directly into a Forge instance's mods/ folder. OptiFine's
/// jar is a Forge mod and a GUI installer; this path never runs the installer.
fn forge_version_of(instance: &Instance) -> Option<String> {
    if let Some(loader) = &instance.mod_loader {
        if loader.kind.eq_ignore_ascii_case("forge") {
            return Some(loader.version.clone());
        }
    }
    instance
        .version_id
        .split_once("-forge-")
        .map(|(_, version)| version.to_string())
}

// ---------------------------------------------------------------------------
// Community resources (Modrinth)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceTaskAccepted {
    pub task_id: String,
    pub title: String,
}

#[tauri::command]
pub async fn search_resources(
    state: State<'_, AppState>,
    query: ResourceSearchQuery,
) -> Result<ResourceSearchPage> {
    state.modrinth.search(&query).await
}

#[tauri::command]
pub async fn resource_game_versions(
    state: State<'_, AppState>,
) -> Result<Vec<sr_core::modrinth::GameVersionInfo>> {
    state.modrinth.game_versions().await
}

#[tauri::command]
pub async fn get_resource_project(
    state: State<'_, AppState>,
    id: String,
) -> Result<ResourceProject> {
    state.modrinth.project(&id).await
}

#[tauri::command]
pub async fn get_resource_versions(
    state: State<'_, AppState>,
    id: String,
    game_version: Option<String>,
    loader: Option<String>,
) -> Result<Vec<ResourceVersion>> {
    state
        .modrinth
        .versions(&id, game_version.as_deref(), loader.as_deref())
        .await
}

async fn instance_base_version(
    state: &AppState,
    game: &GamePaths,
    instance: &Instance,
) -> Result<String> {
    if let Some(base) = state
        .versions
        .base_version_of(game, &instance.version_id)
        .await?
    {
        return Ok(base);
    }
    let id = instance.version_id.as_str();
    for prefix in ["fabric-loader-", "quilt-loader-"] {
        if let Some(rest) = id.strip_prefix(prefix) {
            if let Some((_, mc)) = rest.split_once('-') {
                return Ok(mc.to_string());
            }
        }
    }
    if let Some((base, _)) = id.rsplit_once("-forge-") {
        return Ok(base.to_string());
    }
    if let Some((base, _)) = id.rsplit_once("-neoforge-") {
        return Ok(base.to_string());
    }
    Ok(id.to_string())
}

fn modpack_file(version: &ResourceVersion) -> Result<&sr_core::modrinth::ResourceFile> {
    version
        .files
        .iter()
        .find(|f| f.filename.to_ascii_lowercase().ends_with(".mrpack"))
        .or_else(|| version.files.iter().find(|f| f.primary))
        .ok_or_else(|| AppError::not_found("该整合包版本没有 .mrpack 文件"))
}

#[tauri::command]
pub async fn install_resource(
    state: State<'_, AppState>,
    project_id: String,
    version_id: String,
    target: ResourceTarget,
) -> Result<ResourceTaskAccepted> {
    let project = state.modrinth.project(&project_id).await?;
    let version = state.modrinth.version(&version_id, &project_id).await?;
    let file = if project.kind == "modpack" {
        modpack_file(&version)?.clone()
    } else {
        resource_install::primary_file(&version)?.clone()
    };
    let game = state.game_snapshot().await;

    #[allow(clippy::large_enum_variant)]
    enum Action {
        Instance { instance: Instance, base: String },
        Directory(PathBuf),
    }

    let (action, verb) = match target {
        ResourceTarget::Instance { instance_id } => {
            let instance = InstanceManager::new(game.clone()).get(&instance_id).await?;
            let base = instance_base_version(&state, &game, &instance).await?;
            resource_install::validate_instance_compatibility(
                &project.kind,
                &version,
                &instance,
                &base,
            )?;
            (Action::Instance { instance, base }, "安装")
        }
        ResourceTarget::Directory { path } => {
            if path.trim().is_empty() {
                return Err(AppError::invalid_input("请选择下载目录"));
            }
            (Action::Directory(PathBuf::from(path)), "下载")
        }
    };

    let title = if project.kind == "modpack" {
        format!("{verb}整合包 {} {}", project.title, version.version_number)
    } else {
        format!("{verb} {} {}", project.title, version.version_number)
    };
    let task = state.downloads.create("resource", &title, 0, 0).await;
    let task_id = task.id.clone();
    let _ = state.downloads.save().await;

    let state = state.inner().clone();
    let kind = project.kind.clone();
    let version_clone = version.clone();
    let file_clone = file.clone();
    let spawn_title = title.clone();
    tauri::async_runtime::spawn(async move {
        let cancel = task.cancel_token();
        let result = match action {
            Action::Instance { instance, base } => {
                if kind == "modpack" {
                    let cache = state.paths.cache_dir().join("modpacks");
                    resource_install::install_modpack(resource_install::ModpackInstall {
                        http: &state.http,
                        version: &version_clone,
                        file: &file_clone,
                        instance: &instance,
                        base_version: &base,
                        cache_dir: &cache,
                        task: &task,
                        cancel: &cancel,
                        allow_incomplete: false,
                        curseforge_key: None,
                    })
                    .await
                } else {
                    resource_install::install_file_to_instance(
                        &state.http,
                        &kind,
                        &file_clone,
                        &instance,
                        &task,
                        &cancel,
                    )
                    .await
                    .map(|_| ())
                }
            }
            Action::Directory(dir) => resource_install::install_file_to_directory(
                &state.http,
                &file_clone,
                &dir,
                &task,
                &cancel,
            )
            .await
            .map(|_| ()),
        };
        if let Err(error) = result {
            tracing::warn!(%spawn_title, %error, "resource install failed");
        }
    });

    Ok(ResourceTaskAccepted { task_id, title })
}

#[tauri::command]
pub async fn import_modpack(
    state: State<'_, AppState>,
    instance_id: String,
    path: String,
    allow_incomplete: bool,
) -> Result<ResourceTaskAccepted> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game.clone()).get(&instance_id).await?;
    let base = instance_base_version(&state, &game, &instance).await?;
    let source = PathBuf::from(path);
    if !tokio::fs::try_exists(&source).await.unwrap_or(false) {
        return Err(AppError::not_found("整合包文件不存在"));
    }
    let title = format!(
        "导入整合包 {}",
        source
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "本地整合包".into())
    );
    let task = state.downloads.create("resource", &title, 0, 0).await;
    let task_id = task.id.clone();
    let _ = state.downloads.save().await;
    let curseforge_key = Some(curseforge_api_key(&state)?);
    let state = state.inner().clone();
    let spawn_title = title.clone();
    tauri::async_runtime::spawn(async move {
        let cancel = task.cancel_token();
        if let Err(error) = resource_install::import_modpack(
            &state.http,
            &source,
            &instance,
            &base,
            &task,
            &cancel,
            allow_incomplete,
            curseforge_key,
        )
        .await
        {
            tracing::warn!(%spawn_title, %error, "local modpack import failed");
        }
    });
    Ok(ResourceTaskAccepted { task_id, title })
}

#[tauri::command]
pub async fn import_modpack_bytes(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<ResourceTaskAccepted> {
    let instance_id = request_header(&request, "x-sr-instance")?.to_string();
    let file_name = decode_header_name(request_header(&request, "x-sr-name")?)?;
    let allow_incomplete = request_header(&request, "x-sr-allow-incomplete").unwrap_or("0") == "1";
    let bytes = request_bytes(&request)?;
    if bytes.len() > 512 * 1024 * 1024 {
        return Err(AppError::invalid_input("本地整合包超过 512MB"));
    }
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game.clone()).get(&instance_id).await?;
    let base = instance_base_version(&state, &game, &instance).await?;
    let cache = state.paths.cache_dir().join("modpacks");
    util_ensure_dir(&cache).await?;
    let temp = cache.join(format!("import-{}.mrpack", sr_core::util::short_id()));
    sr_core::util::atomic_write(&temp, bytes).await?;
    let title = format!("导入整合包 {file_name}");
    let task = state.downloads.create("resource", &title, 0, 0).await;
    let task_id = task.id.clone();
    let _ = state.downloads.save().await;
    let curseforge_key = Some(curseforge_api_key(&state)?);
    let state = state.inner().clone();
    let spawn_title = title.clone();
    tauri::async_runtime::spawn(async move {
        let cancel = task.cancel_token();
        if let Err(error) = resource_install::import_modpack(
            &state.http,
            &temp,
            &instance,
            &base,
            &task,
            &cancel,
            allow_incomplete,
            curseforge_key,
        )
        .await
        {
            tracing::warn!(%spawn_title, %error, "local modpack import failed");
        }
        let _ = tokio::fs::remove_file(&temp).await;
    });
    Ok(ResourceTaskAccepted { task_id, title })
}

const CF_CUSTOM_KEY_MARKER: &str = "curseforge-custom-key";

fn cf_custom_key_marker(state: &AppState) -> PathBuf {
    state.paths.base().join(CF_CUSTOM_KEY_MARKER)
}

fn cf_custom_key_enabled(state: &AppState) -> bool {
    cf_custom_key_marker(state).is_file()
}

/// Returns the personal key only when the user explicitly saved one. Otherwise
/// the bundled public key is used without touching the OS keychain, so searching
/// never triggers a keychain permission prompt.
fn curseforge_api_key(state: &AppState) -> Result<String> {
    if cf_custom_key_enabled(state) {
        let store = KeyringStore::new("sr-launcher");
        if let Some(key) = curseforge_service::load_key(&store)? {
            return Ok(key);
        }
    }
    Ok(curseforge_service::builtin_key().to_string())
}

fn curseforge_client(state: &AppState) -> Result<CurseForgeService> {
    Ok(CurseForgeService::new(
        state.http.clone(),
        curseforge_api_key(state)?,
    ))
}

fn file_to_resource(file: &CurseForgeFile) -> sr_core::modrinth::ResourceFile {
    sr_core::modrinth::ResourceFile {
        filename: file.file_name.clone(),
        url: file.download_url.clone(),
        size: file.size,
        primary: true,
        sha1: file.sha1.clone(),
        sha512: None,
        file_type: None,
    }
}

fn cf_release_type(value: u32) -> &'static str {
    match value {
        2 => "beta",
        3 => "alpha",
        _ => "release",
    }
}

fn cf_sort_field(sort: &str) -> u32 {
    match sort {
        "downloads" => 6,
        "newest" | "updated" => 3,
        _ => 2,
    }
}

fn cf_website(kind: &str, slug: &str) -> String {
    let section = match kind {
        "mod" => "mc-mods",
        "modpack" => "modpacks",
        "resourcepack" => "texture-packs",
        "shader" => "shaders",
        _ => "worlds",
    };
    format!("https://www.curseforge.com/minecraft/{section}/{slug}")
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn search_curseforge(
    state: State<'_, AppState>,
    kind: String,
    query: String,
    game_version: Option<String>,
    category_id: Option<u64>,
    loader: Option<String>,
    sort: String,
    offset: Option<u32>,
    limit: Option<u32>,
) -> Result<ResourceSearchPage> {
    let class_id = curseforge_service::class_id_for_kind(&kind)?;
    let service = curseforge_client(&state)?;
    let loader_type = loader
        .as_deref()
        .and_then(curseforge_service::mod_loader_type);
    let page = service
        .search(
            class_id,
            &query,
            game_version.as_deref(),
            category_id,
            loader_type,
            cf_sort_field(&sort),
            offset.unwrap_or(0),
            limit.unwrap_or(20),
        )
        .await?;
    Ok(ResourceSearchPage {
        total_hits: page.total,
        offset: page.offset,
        limit: page.limit,
        hits: page
            .mods
            .into_iter()
            .map(|m| {
                let mut versions: Vec<String> = match game_version.as_deref() {
                    Some(value) => vec![value.to_string()],
                    None => m
                        .files
                        .iter()
                        .flat_map(|f| curseforge_service::split_versions(&f.game_versions).0)
                        .collect(),
                };
                versions.sort();
                versions.dedup();
                ResourceHit {
                    id: m.id.to_string(),
                    kind: kind.clone(),
                    source: "curseforge".into(),
                    slug: m.slug,
                    title: m.name,
                    description: m.summary,
                    author: m.author.unwrap_or_else(|| "CurseForge".into()),
                    categories: m.category_names,
                    versions,
                    downloads: m.download_count,
                    follows: 0,
                    icon_url: m.logo_url,
                    date_modified: m.date_modified,
                    latest_version: m.latest_file.map(|f| f.id.to_string()),
                    license: None,
                    client_side: None,
                    server_side: None,
                }
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn get_curseforge_project(
    state: State<'_, AppState>,
    id: u64,
    kind: Option<String>,
    game_version: Option<String>,
) -> Result<ResourceProject> {
    let kind = kind.unwrap_or_else(|| "map".into());
    let service = curseforge_client(&state)?;
    let m = service.project(id).await?;
    let website = m
        .external_url
        .clone()
        .unwrap_or_else(|| cf_website(&kind, &m.slug));
    let mut versions = Vec::new();
    let mut loaders = Vec::new();
    for file in &m.files {
        let (file_versions, file_loaders) = curseforge_service::split_versions(&file.game_versions);
        versions.extend(file_versions);
        loaders.extend(file_loaders);
    }
    versions.sort();
    versions.dedup();
    loaders.sort();
    loaders.dedup();
    let game_versions = match game_version {
        Some(value) => vec![value],
        None => versions,
    };
    Ok(ResourceProject {
        id: m.id.to_string(),
        kind,
        source: "curseforge".into(),
        slug: m.slug.clone(),
        title: m.name,
        description: m.summary,
        author: m.author.unwrap_or_else(|| "CurseForge".into()),
        categories: m.category_names,
        downloads: m.download_count,
        follows: 0,
        icon_url: m.logo_url,
        game_versions,
        loaders,
        license: None,
        source_url: None,
        issues_url: None,
        wiki_url: None,
        website_url: website,
        published: None,
        updated: m.date_modified,
    })
}

#[tauri::command]
pub async fn get_curseforge_files(
    state: State<'_, AppState>,
    id: u64,
    kind: Option<String>,
    game_version: Option<String>,
    loader: Option<String>,
) -> Result<Vec<ResourceVersion>> {
    let _kind = kind.unwrap_or_else(|| "map".into());
    let service = curseforge_client(&state)?;
    let loader_type = loader
        .as_deref()
        .and_then(curseforge_service::mod_loader_type);
    let mut files = service
        .files(id, game_version.as_deref(), loader_type)
        .await?;
    files.sort_by(|a, b| b.file_date.cmp(&a.file_date));
    Ok(files
        .into_iter()
        .map(|f| {
            let (mc_versions, loaders) = curseforge_service::split_versions(&f.game_versions);
            let file_resource = file_to_resource(&f);
            ResourceVersion {
                id: f.id.to_string(),
                project_id: id.to_string(),
                name: f.file_name.clone(),
                version_number: f.file_name.clone(),
                version_type: cf_release_type(f.release_type).into(),
                date_published: f.file_date,
                downloads: 0,
                game_versions: mc_versions,
                loaders,
                files: vec![file_resource],
                dependencies: vec![],
            }
        })
        .collect())
}

#[tauri::command]
pub async fn install_curseforge(
    state: State<'_, AppState>,
    project_id: u64,
    file_id: u64,
    kind: String,
    target: ResourceTarget,
) -> Result<ResourceTaskAccepted> {
    let service = curseforge_client(&state)?;
    let file = service.file(project_id, file_id).await?;
    if kind == "map" && !file.file_name.to_ascii_lowercase().ends_with(".zip") {
        return Err(AppError::invalid_input("CurseForge 地图文件不是 .zip 存档"));
    }
    if file.sha1.is_none() {
        return Err(AppError::invalid_input("文件缺少 SHA-1，已拒绝安装"));
    }
    let (mc_versions, loaders) = curseforge_service::split_versions(&file.game_versions);
    let resource_file = file_to_resource(&file);
    let version = ResourceVersion {
        id: file.id.to_string(),
        project_id: project_id.to_string(),
        name: file.file_name.clone(),
        version_number: file.file_name.clone(),
        version_type: cf_release_type(file.release_type).into(),
        date_published: file.file_date.clone(),
        downloads: 0,
        game_versions: mc_versions,
        loaders,
        files: vec![resource_file.clone()],
        dependencies: vec![],
    };
    let label = match kind.as_str() {
        "map" => "地图",
        "modpack" => "整合包",
        "shader" => "光影",
        "resourcepack" => "材质",
        _ => "模组",
    };
    let title = format!("安装{label} {}", file.file_name);
    let game = state.game_snapshot().await;
    let curseforge_key = Some(curseforge_api_key(&state)?);

    let task = state.downloads.create("resource", &title, 0, 0).await;
    let task_id = task.id.clone();
    let _ = state.downloads.save().await;
    let state = state.inner().clone();
    let spawn_title = title.clone();
    tauri::async_runtime::spawn(async move {
        let cancel = task.cancel_token();
        let result = match target {
            ResourceTarget::Instance { instance_id } => {
                match InstanceManager::new(game.clone()).get(&instance_id).await {
                    Ok(instance) => {
                        if kind == "map" {
                            let cache = state.paths.cache_dir().join("maps");
                            resource_install::install_world_archive(
                                &state.http,
                                &resource_file,
                                &instance,
                                &cache,
                                &task,
                                &cancel,
                            )
                            .await
                            .map(|_| ())
                        } else {
                            match instance_base_version(&state, &game, &instance).await {
                                Ok(base) => {
                                    if let Err(error) =
                                        resource_install::validate_instance_compatibility(
                                            &kind, &version, &instance, &base,
                                        )
                                    {
                                        Err(error)
                                    } else if kind == "modpack" {
                                        let cache = state.paths.cache_dir().join("modpacks");
                                        resource_install::install_modpack(
                                            resource_install::ModpackInstall {
                                                http: &state.http,
                                                version: &version,
                                                file: &resource_file,
                                                instance: &instance,
                                                base_version: &base,
                                                cache_dir: &cache,
                                                task: &task,
                                                cancel: &cancel,
                                                allow_incomplete: false,
                                                curseforge_key: curseforge_key.clone(),
                                            },
                                        )
                                        .await
                                    } else {
                                        resource_install::install_file_to_instance(
                                            &state.http,
                                            &kind,
                                            &resource_file,
                                            &instance,
                                            &task,
                                            &cancel,
                                        )
                                        .await
                                        .map(|_| ())
                                    }
                                }
                                Err(error) => Err(error),
                            }
                        }
                    }
                    Err(e) => Err(e),
                }
            }
            ResourceTarget::Directory { path } => resource_install::install_file_to_directory(
                &state.http,
                &resource_file,
                Path::new(&path),
                &task,
                &cancel,
            )
            .await
            .map(|_| ()),
        };
        if let Err(error) = result {
            tracing::warn!(%spawn_title, %error, "curseforge install failed");
        }
    });

    Ok(ResourceTaskAccepted { task_id, title })
}

#[tauri::command]
pub async fn curseforge_key_status(state: State<'_, AppState>) -> Result<String> {
    Ok(if cf_custom_key_enabled(&state) {
        "custom"
    } else {
        "builtin"
    }
    .into())
}

#[tauri::command]
pub async fn set_curseforge_api_key(state: State<'_, AppState>, key: String) -> Result<()> {
    let store = KeyringStore::new("sr-launcher");
    curseforge_service::save_key(&store, &key)?;
    let marker = cf_custom_key_marker(&state);
    tokio::fs::write(&marker, b"1")
        .await
        .map_err(|e| AppError::internal(format!("无法写入 CurseForge key 标记：{e}")))
}

#[tauri::command]
pub async fn clear_curseforge_api_key(state: State<'_, AppState>) -> Result<()> {
    let marker = cf_custom_key_marker(&state);
    if tokio::fs::try_exists(&marker).await.unwrap_or(false) {
        let _ = tokio::fs::remove_file(&marker).await;
    }
    let store = KeyringStore::new("sr-launcher");
    if let Err(error) = curseforge_service::clear_key(&store) {
        tracing::warn!(%error, "could not delete CurseForge key from keychain");
    }
    Ok(())
}

#[tauri::command]
pub async fn test_curseforge_api_key(state: State<'_, AppState>) -> Result<()> {
    CurseForgeService::new(state.http.clone(), curseforge_api_key(&state)?)
        .test()
        .await
}

#[tauri::command]
pub async fn default_resource_dir() -> Result<String> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| AppError::not_found("无法确定用户主目录"))?;
    let home = PathBuf::from(home);
    let downloads = home.join("Downloads");
    let dir = if downloads.is_dir() { downloads } else { home };
    Ok(dir.to_string_lossy().into_owned())
}

async fn util_ensure_dir(path: &Path) -> Result<()> {
    sr_core::util::ensure_dir(path).await
}

// ---------------------------------------------------------------------------
// Worlds (maps)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_worlds(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<WorldEntry>> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    world_service::list(&instance).await
}

#[tauri::command]
pub async fn open_world_folder(
    state: State<'_, AppState>,
    instance_id: String,
    folder: String,
) -> Result<()> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    let dir = world_service::world_dir(&instance, &folder)?;
    let meta = tokio::fs::symlink_metadata(&dir).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("存档目录不存在")
        } else {
            e.into()
        }
    })?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(AppError::path_unsafe(
            "存档目录是符号链接或非目录，已拒绝打开",
        ));
    }
    open_directory(&dir).await
}

// ---------------------------------------------------------------------------
// Resource packs (materials)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_resourcepacks(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<ResourcePackEntry>> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    resourcepack_service::list(&instance).await
}

#[tauri::command]
pub async fn import_resourcepacks(
    state: State<'_, AppState>,
    instance_id: String,
    source_paths: Vec<String>,
    overwrite: bool,
) -> Result<Vec<ImportOutcome>> {
    if source_paths.is_empty() {
        return Err(AppError::invalid_input("没有可导入的文件"));
    }
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    let mut out = Vec::new();
    for source in source_paths {
        let path = PathBuf::from(&source);
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| source.clone());
        match resourcepack_service::import(&instance, &path, overwrite).await {
            Ok(entry) => out.push(ImportOutcome {
                file_name: entry.file_name,
                ok: true,
                error: None,
            }),
            Err(e) => out.push(ImportOutcome {
                file_name,
                ok: false,
                error: Some(e.message),
            }),
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn import_resourcepack_bytes(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<ResourcePackEntry> {
    let instance_id = request_header(&request, "x-sr-instance")?.to_string();
    let file_name = decode_header_name(request_header(&request, "x-sr-name")?)?;
    let overwrite = request_header(&request, "x-sr-overwrite").unwrap_or("0") == "1";
    let bytes = request_bytes(&request)?;
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    resourcepack_service::import_bytes(&instance, &file_name, bytes, overwrite).await
}

#[tauri::command]
pub async fn set_resourcepack_enabled(
    state: State<'_, AppState>,
    instance_id: String,
    file_name: String,
    enabled: bool,
) -> Result<ResourcePackEntry> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    resourcepack_service::set_enabled(&instance, &file_name, enabled).await
}

#[tauri::command]
pub async fn delete_resourcepack(
    state: State<'_, AppState>,
    instance_id: String,
    file_name: String,
) -> Result<()> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    resourcepack_service::delete(&instance, &file_name).await
}

#[tauri::command]
pub async fn install_optifine(state: State<'_, AppState>, instance_id: String) -> Result<ModEntry> {
    optifine_install_inner(&state, &instance_id, None).await
}

async fn optifine_install_inner(
    state: &AppState,
    instance_id: &str,
    requested_label: Option<&str>,
) -> Result<ModEntry> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game.clone()).get(instance_id).await?;
    let is_forge = instance
        .mod_loader
        .as_ref()
        .map(|m| m.kind.eq_ignore_ascii_case("forge"))
        .unwrap_or(false)
        || instance.version_id.contains("-forge-");
    if !is_forge {
        return Err(AppError::new(
            ErrorCode::Unsupported,
            "OptiFine 直装仅支持 Forge 实例；Fabric/Quilt 请使用 Iris",
        ));
    }
    let game_version = state
        .versions
        .base_version_of(&game, &instance.version_id)
        .await?
        .ok_or_else(|| AppError::new(ErrorCode::VersionInvalid, "无法解析基础 Minecraft 版本"))?;
    let info = recommend_service::optifine_build_info_for(
        &state.http,
        &game_version,
        requested_label,
    )
    .await?
    .ok_or_else(|| {
            AppError::not_found(format!(
                "Minecraft {game_version} 没有兼容 Forge 的 OptiFine 版本；建议使用 1.21.1 + Forge 52 + OptiFine 稳定版"
            ))
        })?;
    // OptiFine is a binary patch against one specific Forge build. Newer Forge
    // releases can add methods that OptiFine's patches remove, producing a
    // NoSuchMethodError only when a world starts ticking (e.g. Vault blocks).
    if let (Some(required), Some(current)) = (
        recommend_service::optifine_forge_requirement(info.forge.as_deref()),
        forge_version_of(&instance),
    ) {
        if recommend_service::version_gt(&current, &required) {
            return Err(AppError::new(
                ErrorCode::Unsupported,
                format!(
                    "OptiFine {} 要求 Forge {required}，当前实例使用 Forge {current}；新版 Forge 与 OptiFine 补丁冲突，进入世界会崩溃",
                    info.label
                ),
            )
            .with_detail(format!(
                "请先安装 Forge {required} 并用它创建实例，或改用 Fabric/Quilt + Iris。"
            )));
        }
    }
    for entry in mods_service::list(&instance).await? {
        if entry.file_name.to_lowercase().contains("optifine") {
            if !entry.enabled {
                return mods_service::toggle(&instance, &entry.file_name, true).await;
            }
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                format!("该实例已存在 OptiFine：{}", entry.file_name),
            ));
        }
    }
    let filename = info.filename.clone();
    let url = info.url.clone();
    let parsed = sr_core::download::ensure_trusted_url(&url, false)?;
    let mut bytes = None;
    let mut last_error: Option<AppError> = None;
    for attempt in 0..3u32 {
        match state
            .http
            .get(parsed.clone())
            .header(reqwest::header::USER_AGENT, sr_core::USER_AGENT)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(b) if !b.is_empty() => {
                    if b.len() > 64 * 1024 * 1024 {
                        return Err(AppError::new(
                            ErrorCode::VersionInvalid,
                            "OptiFine 文件异常过大",
                        ));
                    }
                    bytes = Some(b);
                    break;
                }
                Ok(_) => last_error = Some(AppError::new(ErrorCode::Http, "OptiFine 文件为空")),
                Err(e) => last_error = Some(e.into()),
            },
            Ok(resp) => {
                last_error = Some(AppError::new(
                    ErrorCode::Http,
                    format!("OptiFine 下载返回 HTTP {}", resp.status().as_u16()),
                ))
            }
            Err(e) => last_error = Some(e.into()),
        }
        if attempt < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        }
    }
    let bytes = bytes
        .ok_or_else(|| last_error.unwrap_or_else(|| AppError::network("无法下载 OptiFine")))?;
    mods_service::import_bytes(&instance, &filename, &bytes, false).await
}

fn request_header<'a>(request: &'a tauri::ipc::Request<'_>, key: &str) -> Result<&'a str> {
    request
        .headers()
        .get(key)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::invalid_input(format!("缺少请求头 {key}")))
}

fn request_bytes<'a>(request: &'a tauri::ipc::Request<'_>) -> Result<&'a [u8]> {
    match request.body() {
        tauri::ipc::InvokeBody::Raw(bytes) => Ok(bytes.as_slice()),
        tauri::ipc::InvokeBody::Json(_) => {
            Err(AppError::invalid_input("该命令只接受原始字节请求体"))
        }
    }
}

fn decode_header_name(value: &str) -> Result<String> {
    percent_encoding::percent_decode_str(value)
        .decode_utf8()
        .map(|s| s.into_owned())
        .map_err(|_| AppError::invalid_input("文件名编码无效"))
}

// ---------------------------------------------------------------------------
// Java
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_java(state: State<'_, AppState>, refresh: bool) -> Result<Vec<JavaInfo>> {
    if refresh {
        let settings = state.settings_snapshot().await;
        let found = java::discover_with_managed(
            settings.java_path.as_deref(),
            Some(&state.paths.java_dir()),
        )
        .await;
        *state.java.write().await = found;
    }
    Ok(state.java.read().await.clone())
}

#[tauri::command]
pub async fn java_runtime_options(
    state: State<'_, AppState>,
) -> Result<java_runtime::JavaRuntimeOptions> {
    java_runtime::options(&state.http).await
}

#[tauri::command]
pub async fn install_java_runtime(state: State<'_, AppState>, major: u32) -> Result<JavaInfo> {
    if !(8..=40).contains(&major) {
        return Err(AppError::invalid_input("Java 版本不在支持范围内"));
    }
    let settings = state.settings_snapshot().await;
    let title = format!("安装 Java {major}");
    let task = state.downloads.create("java", &title, 1, 0).await;
    let _ = state.downloads.save().await;
    let cancel = task.cancel_token();
    let result = java_runtime::install(
        &state.http,
        &state.paths,
        major,
        settings.use_mirror,
        &task,
        &cancel,
    )
    .await;
    let info = match result {
        Ok(info) => info,
        Err(error) => {
            task.fail(error.clone()).await;
            return Err(error);
        }
    };
    {
        let mut list = state.java.write().await;
        if !list.iter().any(|j| j.path == info.path) {
            list.push(info.clone());
            list.sort_by_key(|a| std::cmp::Reverse(a.major));
        }
    }
    let mut settings = state.settings_snapshot().await;
    settings.java_path = Some(info.path.clone());
    settings::save(&state.paths, &settings).await?;
    *state.settings.write().await = settings;
    Ok(info)
}

#[tauri::command]
pub async fn add_java_path(state: State<'_, AppState>, path: String) -> Result<JavaInfo> {
    if path.trim().is_empty() {
        return Err(AppError::invalid_input("Java 路径不能为空"));
    }
    let info = java::probe(&PathBuf::from(&path), "manual").await?;
    {
        let mut list = state.java.write().await;
        if !list.iter().any(|j| j.path == info.path) {
            list.push(info.clone());
            list.sort_by_key(|a| std::cmp::Reverse(a.major));
        }
    }
    let mut settings = state.settings_snapshot().await;
    settings.java_path = Some(info.path.clone());
    settings::save(&state.paths, &settings).await?;
    *state.settings.write().await = settings;
    Ok(info)
}

#[tauri::command]
pub async fn check_java_for_version(
    state: State<'_, AppState>,
    version_id: String,
) -> Result<JavaCheck> {
    let game = state.game_snapshot().await;
    let resolved = state.versions.load_installed(&game, &version_id).await?;
    let required = resolved.java_major();
    let settings = state.settings_snapshot().await;
    let javas = state.java.read().await.clone();
    let selected = java::select(&javas, required, settings.java_path.as_deref())
        .ok_or_else(|| AppError::new(ErrorCode::JavaInvalid, "未发现可用的 Java"))?;
    Ok(java::check_compatibility(required, selected.major))
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>) -> Result<Vec<AccountView>> {
    state.accounts.list().await
}

#[tauri::command]
pub async fn add_offline_account(
    state: State<'_, AppState>,
    username: String,
    skin_id: Option<String>,
    skin_model: Option<SkinModel>,
) -> Result<Account> {
    let account = state.accounts.add_offline(&username).await?;
    match skin_id {
        Some(id) => {
            state.skins.get(&id).await?;
            state
                .accounts
                .set_skin(&account.id, Some(id), skin_model)
                .await
        }
        None => Ok(account),
    }
}

#[tauri::command]
pub async fn remove_account(state: State<'_, AppState>, account_id: String) -> Result<()> {
    state.accounts.remove(&account_id).await
}

#[tauri::command]
pub async fn set_active_account(state: State<'_, AppState>, account_id: String) -> Result<()> {
    state.accounts.set_active(&account_id).await
}

// ---------------------------------------------------------------------------
// Skins
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_skins(state: State<'_, AppState>) -> Result<skin_service::SkinListView> {
    let game = state.game_snapshot().await;
    state.skins.list(&game).await
}

#[tauri::command]
pub async fn import_skin_file(
    state: State<'_, AppState>,
    path: String,
    model: SkinModel,
) -> Result<SkinView> {
    if path.trim().is_empty() {
        return Err(AppError::invalid_input("请选择皮肤文件"));
    }
    let skin = state.skins.import_path(Path::new(&path), model).await?;
    state.skins.view(&skin).await
}

#[tauri::command]
pub async fn import_skin_bytes(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> Result<SkinView> {
    let file_name = decode_header_name(request_header(&request, "x-sr-name")?)?;
    let model = match request_header(&request, "x-sr-model")? {
        "slim" => SkinModel::Slim,
        "classic" => SkinModel::Classic,
        other => return Err(AppError::invalid_input(format!("未知的皮肤模型：{other}"))),
    };
    let bytes = request_bytes(&request)?;
    let skin = state.skins.import_bytes(&file_name, bytes, model).await?;
    state.skins.view(&skin).await
}

#[tauri::command]
pub async fn fetch_skin_by_name(state: State<'_, AppState>, username: String) -> Result<SkinView> {
    let skin = state.skins.fetch_mojang(&username).await?;
    state.skins.view(&skin).await
}

#[tauri::command]
pub async fn delete_skin(state: State<'_, AppState>, skin_id: String) -> Result<()> {
    // Delete first so a rejected deletion (e.g. builtin skins) leaves account
    // references untouched.
    state.skins.delete(&skin_id).await?;
    state.accounts.clear_skin_reference(&skin_id).await
}

#[tauri::command]
pub async fn set_account_skin(
    state: State<'_, AppState>,
    account_id: String,
    skin_id: Option<String>,
    model: Option<SkinModel>,
) -> Result<Account> {
    if let Some(id) = skin_id.as_deref() {
        state.skins.get(id).await?;
    }
    state.accounts.set_skin(&account_id, skin_id, model).await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinApplyResult {
    pub message: String,
}

#[tauri::command]
pub async fn apply_account_skin(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<SkinApplyResult> {
    let account = state.accounts.get(&account_id).await?;
    if account.kind != sr_core::account::AccountKind::Microsoft {
        return Err(
            AppError::new(ErrorCode::Unsupported, "离线档案没有可上传的官方皮肤服务")
                .with_detail("离线档案的皮肤只会在本机通过资源包显示，无法上传到 Mojang。"),
        );
    }
    let skin_id = account
        .skin_id
        .as_deref()
        .ok_or_else(|| AppError::invalid_input("请先为该账号选择皮肤"))?;
    let skin = state.skins.get(skin_id).await?;
    let model = account.skin_model.unwrap_or(skin.model);
    let png = state.skins.compat_bytes(&skin).await?;

    let settings = state.settings_snapshot().await;
    let store = KeyringStore::new("sr-launcher");
    let mut tokens = load_tokens(&store, &account.uuid)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::AuthenticationRequired,
            "该账号没有可用的 Minecraft 令牌，请重新登录",
        )
    })?;
    if tokens.mc_expires_at <= chrono::Utc::now().timestamp() {
        let client_id = settings
            .microsoft_client_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::NotConfigured,
                    "登录已过期，且未配置 Microsoft client id，无法刷新",
                )
            })?;
        let session = MicrosoftAuth::new(state.http.clone(), client_id)
            .refresh(&tokens.refresh_token)
            .await?;
        tokens = save_tokens(&store, &account.uuid, &session)?;
    }
    skin_service::upload_microsoft(&state.http, &tokens.mc_access_token, &png, model).await?;
    Ok(SkinApplyResult {
        message: "皮肤已上传到 Minecraft 账号，通常在 1 分钟内生效".into(),
    })
}

#[tauri::command]
pub async fn refresh_microsoft_account(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Account> {
    let account = state.accounts.get(&account_id).await?;
    if account.kind != sr_core::account::AccountKind::Microsoft {
        return Err(AppError::new(
            ErrorCode::Unsupported,
            "离线档案没有可刷新的 Microsoft 授权",
        ));
    }
    let settings = state.settings_snapshot().await;
    let client_id = settings
        .microsoft_client_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| AppError::new(ErrorCode::NotConfigured, "Microsoft 登录尚未配置"))?;
    let store = KeyringStore::new("sr-launcher");
    let tokens = load_tokens(&store, &account.uuid)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::AuthenticationRequired,
            "该账号没有保存的令牌，请重新登录",
        )
    })?;
    let session = MicrosoftAuth::new(state.http.clone(), client_id)
        .refresh(&tokens.refresh_token)
        .await?;
    save_tokens(&store, &session.uuid, &session)?;
    let updated = state
        .accounts
        .upsert_microsoft(&session.username, &session.uuid)
        .await?;
    Ok(updated)
}

#[tauri::command]
pub async fn begin_microsoft_login(state: State<'_, AppState>) -> Result<DeviceCodeView> {
    let settings = state.settings_snapshot().await;
    let client_id = settings
        .microsoft_client_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| {
            AppError::new(ErrorCode::NotConfigured, "Microsoft 登录尚未配置")
                .with_detail("请在设置中填写 Azure 应用的公开 client id。")
        })?;
    MicrosoftAuth::new(state.http.clone(), client_id)
        .begin_device_code()
        .await
}

#[tauri::command]
pub async fn poll_microsoft_login(
    state: State<'_, AppState>,
    device_code: String,
) -> Result<MicrosoftPoll> {
    let settings = state.settings_snapshot().await;
    let client_id = settings
        .microsoft_client_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| AppError::new(ErrorCode::NotConfigured, "Microsoft 登录尚未配置"))?;
    let auth = MicrosoftAuth::new(state.http.clone(), client_id);
    let (mut poll, session) = auth.complete_device_login(&device_code).await?;
    if let Some(session) = session {
        // Tokens are keyed by the stable Minecraft UUID so re-login reuses the
        // same credential entry.
        let store = KeyringStore::new("sr-launcher");
        save_tokens(&store, &session.uuid, &session)?;
        let account = state
            .accounts
            .upsert_microsoft(&session.username, &session.uuid)
            .await?;
        state.accounts.set_active(&account.id).await?;
        poll.account = Some(account);
    }
    Ok(poll)
}

#[tauri::command]
pub async fn get_loader_recommendations(
    state: State<'_, AppState>,
    game_version: String,
) -> Result<Recommendations> {
    let game = state.game_snapshot().await;
    if !tokio::fs::try_exists(game.version_json(&game_version))
        .await
        .unwrap_or(false)
    {
        return Err(AppError::not_found(format!(
            "请先安装基础版本 {game_version}"
        )));
    }
    Ok(recommend_service::recommend(&state.http, &game_version).await)
}

/// Open one of the approved official/mirror download pages in the system
/// browser. Arbitrary URLs are rejected.
#[tauri::command]
pub async fn open_trusted_url(url: String) -> Result<()> {
    let parsed =
        reqwest::Url::parse(&url).map_err(|_| AppError::invalid_input("下载链接格式无效"))?;
    if parsed.scheme() != "https" {
        return Err(AppError::path_unsafe("只允许打开 HTTPS 链接"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::invalid_input("下载链接缺少主机名"))?;
    const ALLOWED: &[&str] = &[
        "bmclapi2.bangbang93.com",
        "maven.neoforged.net",
        "files.minecraftforge.net",
        "www.minecraftforge.net",
        "optifine.net",
        "www.optifine.net",
        "modrinth.com",
        "www.modrinth.com",
        "curseforge.com",
        "platform.deepseek.com",
        "microsoft.com",
        "www.microsoft.com",
        "portal.azure.com",
        "learn.microsoft.com",
        "www.curseforge.com",
        "console.curseforge.com",
        "www.52frp.com",
        "52frp.com",
    ];
    if !ALLOWED.contains(&host) {
        return Err(AppError::path_unsafe(format!("不允许打开的站点：{host}")));
    }
    #[cfg(target_os = "macos")]
    let mut command = tokio::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = tokio::process::Command::new("explorer");
        c.arg(&url);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = tokio::process::Command::new("xdg-open");
    #[cfg(not(target_os = "windows"))]
    command.arg(&url);
    let status = command.status().await.map_err(|e| {
        AppError::new(ErrorCode::Io, "无法调用系统浏览器").with_detail(e.to_string())
    })?;
    #[cfg(not(target_os = "windows"))]
    if !status.success() {
        return Err(AppError::new(ErrorCode::Io, "系统浏览器返回失败状态"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Loaders (Fabric / Quilt)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_fabric_loaders(
    state: State<'_, AppState>,
    game_version: String,
) -> Result<Vec<FabricLoader>> {
    list_loader(state, LoaderKind::Fabric, game_version).await
}

#[tauri::command]
pub async fn list_quilt_loaders(
    state: State<'_, AppState>,
    game_version: String,
) -> Result<Vec<FabricLoader>> {
    list_loader(state, LoaderKind::Quilt, game_version).await
}

async fn list_loader(
    state: State<'_, AppState>,
    kind: LoaderKind,
    game_version: String,
) -> Result<Vec<FabricLoader>> {
    // The base version does not need to be installed yet: the install flow can
    // download it first, just like Forge/NeoForge. Only the meta lookup matters
    // for listing loader versions.
    let settings = state.settings_snapshot().await;
    service_for(kind, state.http.clone(), settings.use_mirror)
        .list_loaders(&game_version)
        .await
}

fn service_for(kind: LoaderKind, http: reqwest::Client, use_mirror: bool) -> FabricService {
    let service = match kind {
        LoaderKind::Fabric => FabricService::new(http),
        LoaderKind::Quilt => FabricService::quilt(http),
    };
    service.with_mirror(use_mirror)
}

#[tauri::command]
pub async fn list_forge_versions(
    state: State<'_, AppState>,
    game_version: String,
) -> Result<Vec<String>> {
    recommend_service::forge_available_versions(&state.http, &game_version).await
}

#[tauri::command]
pub async fn list_neoforge_versions(
    state: State<'_, AppState>,
    game_version: String,
) -> Result<Vec<String>> {
    recommend_service::neoforge_available_versions(&state.http, &game_version).await
}

#[tauri::command]
pub async fn install_forge(
    state: State<'_, AppState>,
    game_version: String,
    loader_version: Option<String>,
) -> Result<LoaderInstallAccepted> {
    install_installer_loader(
        state,
        InstallerKind::Forge,
        game_version,
        loader_version,
        None,
    )
    .await
}

#[tauri::command]
pub async fn install_neoforge(
    state: State<'_, AppState>,
    game_version: String,
    loader_version: Option<String>,
) -> Result<LoaderInstallAccepted> {
    install_installer_loader(
        state,
        InstallerKind::NeoForge,
        game_version,
        loader_version,
        None,
    )
    .await
}

async fn install_installer_loader(
    state: State<'_, AppState>,
    kind: InstallerKind,
    game_version: String,
    loader_version: Option<String>,
    instance_id: Option<String>,
) -> Result<LoaderInstallAccepted> {
    let game = state.game_snapshot().await;
    if !tokio::fs::try_exists(game.version_json(&game_version))
        .await
        .unwrap_or(false)
    {
        return Err(AppError::not_found(format!(
            "请先安装基础版本 {game_version}"
        )));
    }
    if let Some(id) = instance_id.as_deref() {
        let instance = InstanceManager::new(game.clone()).get(id).await?;
        let instance_base = instance_base_version(&state, &game, &instance).await?;
        if instance_base != game_version {
            return Err(AppError::invalid_input(format!(
                "实例基础版本是 {instance_base}，与所选 Minecraft {game_version} 不一致"
            )));
        }
    }
    let base = state.versions.load_installed(&game, &game_version).await?;
    let javas = state.java.read().await.clone();
    let settings = state.settings_snapshot().await;
    let java = java::select(&javas, base.java_major(), settings.java_path.as_deref())
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::JavaInvalid,
                format!(
                    "没有可用的 Java {}，请先在 Java 管理中添加",
                    base.java_major()
                ),
            )
        })?
        .clone();

    let plan = installer_service::plan(&state.http, &game_version, kind, loader_version.as_deref())
        .await?;
    let guard_key = format!("{}:{game_version}:{}", kind.name(), plan.loader_version);
    {
        let mut installing = state.installing.lock().await;
        if !installing.insert(guard_key.clone()) {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                format!("该 {} 版本正在安装中", kind.name()),
            ));
        }
    }

    let task = state
        .downloads
        .create(
            "install",
            &format!(
                "安装 {} {} · {game_version}",
                kind.name(),
                plan.loader_version
            ),
            0,
            0,
        )
        .await;
    let task_id = task.id.clone();
    let _ = state.downloads.save().await;

    let state = state.inner().clone();
    let plan_for_task = plan.clone();
    let target_instance = instance_id.clone();
    tauri::async_runtime::spawn(async move {
        let game = state.game_snapshot().await;
        let cancel = task.cancel_token();
        let result = installer_service::install(
            &state.http,
            &state.versions,
            &state.paths,
            &game,
            &java.path,
            &plan_for_task,
            task,
            cancel,
        )
        .await;
        if let Err(e) = &result {
            tracing::warn!(version = %plan_for_task.version_id, error = %e, "installer loader failed");
        } else if let Some(id) = target_instance.as_deref() {
            if let Err(e) = InstanceManager::new(game.clone())
                .set_version(id, &plan_for_task.version_id)
                .await
            {
                tracing::warn!(instance = %id, error = %e, "could not attach loader version to instance");
            }
        }
        let _ = state.downloads.save().await;
        state.installing.lock().await.remove(&guard_key);
    });

    Ok(LoaderInstallAccepted {
        task_id,
        version_id: plan.version_id,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderInstallAccepted {
    pub task_id: String,
    pub version_id: String,
}

#[tauri::command]
pub async fn install_fabric(
    state: State<'_, AppState>,
    game_version: String,
    loader_version: String,
) -> Result<LoaderInstallAccepted> {
    install_loader(
        state,
        LoaderKind::Fabric,
        game_version,
        loader_version,
        None,
    )
    .await
}

#[tauri::command]
pub async fn install_quilt(
    state: State<'_, AppState>,
    game_version: String,
    loader_version: String,
) -> Result<LoaderInstallAccepted> {
    install_loader(state, LoaderKind::Quilt, game_version, loader_version, None).await
}

async fn install_loader(
    state: State<'_, AppState>,
    kind: LoaderKind,
    game_version: String,
    loader_version: String,
    instance_id: Option<String>,
) -> Result<LoaderInstallAccepted> {
    let game = state.game_snapshot().await;
    if !tokio::fs::try_exists(game.version_json(&game_version))
        .await
        .unwrap_or(false)
    {
        return Err(AppError::not_found(format!(
            "请先安装基础版本 {game_version}"
        )));
    }
    if let Some(id) = instance_id.as_deref() {
        let instance = InstanceManager::new(game.clone()).get(id).await?;
        let instance_base = instance_base_version(&state, &game, &instance).await?;
        if instance_base != game_version {
            return Err(AppError::invalid_input(format!(
                "实例基础版本是 {instance_base}，与所选 Minecraft {game_version} 不一致"
            )));
        }
    }
    let prefix = match kind {
        LoaderKind::Fabric => "fabric-loader",
        LoaderKind::Quilt => "quilt-loader",
    };
    let version_id = format!("{prefix}-{loader_version}-{game_version}");
    let guard_key = format!("{}:{game_version}:{loader_version}", kind.name());
    {
        let mut installing = state.installing.lock().await;
        if !installing.insert(guard_key.clone()) {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                format!("该 {} 版本正在安装中", kind.name()),
            ));
        }
    }

    let settings = state.settings_snapshot().await;
    let options = sr_core::install::InstallOptions {
        include_assets: false,
        concurrency: settings.max_concurrent_downloads,
        allow_custom_hosts: settings.allow_custom_hosts,
    };
    let task = state
        .downloads
        .create(
            "install",
            &format!("安装 {} {loader_version} · {game_version}", kind.name()),
            0,
            0,
        )
        .await;
    let task_id = task.id.clone();
    let _ = state.downloads.save().await;

    let state = state.inner().clone();
    let spawn_game = game_version.clone();
    let spawn_loader = loader_version.clone();
    let target_instance = instance_id.clone();
    tauri::async_runtime::spawn(async move {
        let game = state.game_snapshot().await;
        let service = service_for(kind, state.http.clone(), settings.use_mirror);
        match service
            .resolve_for_install(&state.versions, &game, &spawn_game, &spawn_loader)
            .await
        {
            Ok(resolved) => {
                let actual_id = resolved.id.clone();
                let installer = sr_core::install::Installer {
                    versions: &state.versions,
                    http: &state.http,
                    paths: &state.paths,
                    game: &game,
                    downloads: &state.downloads,
                    prepared: Some(resolved),
                    options,
                };
                let result = installer.install(task, &actual_id).await;
                if let Err(e) = &result {
                    tracing::warn!(version = %actual_id, error = %e, "loader install failed");
                } else if let Some(id) = target_instance.as_deref() {
                    if let Err(e) = InstanceManager::new(game.clone())
                        .set_version(id, &actual_id)
                        .await
                    {
                        tracing::warn!(instance = %id, error = %e, "could not attach loader version to instance");
                    }
                }
                let _ = state.downloads.save().await;
            }
            Err(e) => {
                task.fail(e.clone()).await;
                tracing::warn!(version = %spawn_game, error = %e, "loader metadata failed");
                let _ = state.downloads.save().await;
            }
        }
        state.installing.lock().await.remove(&guard_key);
    });

    Ok(LoaderInstallAccepted {
        task_id,
        version_id,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersionOption {
    pub version: String,
    pub label: String,
    pub variant: Option<String>,
    pub stable: Option<bool>,
}

fn loader_key(loader: &str) -> Result<&'static str> {
    match loader.trim().to_ascii_lowercase().as_str() {
        "fabric" => Ok("fabric"),
        "quilt" => Ok("quilt"),
        "forge" => Ok("forge"),
        "neoforge" => Ok("neoforge"),
        "iris" => Ok("iris"),
        "optifine" => Ok("optifine"),
        _ => Err(AppError::invalid_input("不支持的加载器类型")),
    }
}

fn loader_display(loader: &str) -> &'static str {
    match loader {
        "fabric" => "Fabric",
        "quilt" => "Quilt",
        "forge" => "Forge",
        "neoforge" => "NeoForge",
        "iris" => "Iris",
        "optifine" => "OptiFine",
        _ => "加载器",
    }
}

fn validate_loader_token(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    {
        return Err(AppError::invalid_input(format!("{label} 格式非法")));
    }
    Ok(())
}

fn validate_optifine_label(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_' | '+'))
    {
        return Err(AppError::invalid_input("OptiFine 版本格式非法"));
    }
    Ok(())
}

#[tauri::command]
pub async fn loader_resource_versions(
    state: State<'_, AppState>,
    loader: String,
    game_version: String,
) -> Result<Vec<LoaderVersionOption>> {
    let loader = loader_key(&loader)?;
    validate_loader_token(&game_version, "Minecraft 版本")?;
    match loader {
        "fabric" | "quilt" => {
            let kind = if loader == "fabric" {
                LoaderKind::Fabric
            } else {
                LoaderKind::Quilt
            };
            let settings = state.settings_snapshot().await;
            let list = service_for(kind, state.http.clone(), settings.use_mirror)
                .list_loaders(&game_version)
                .await?;
            Ok(list
                .into_iter()
                .map(|item| LoaderVersionOption {
                    version: item.version.clone(),
                    label: item.version,
                    variant: None,
                    stable: Some(item.stable),
                })
                .collect())
        }
        "forge" => Ok(
            recommend_service::forge_available_versions(&state.http, &game_version)
                .await?
                .into_iter()
                .map(|version| LoaderVersionOption {
                    label: version.clone(),
                    version,
                    variant: None,
                    stable: None,
                })
                .collect(),
        ),
        "neoforge" => Ok(recommend_service::neoforge_available_versions(
            &state.http,
            &game_version,
        )
        .await?
        .into_iter()
        .map(|version| LoaderVersionOption {
            label: version.clone(),
            version,
            variant: None,
            stable: None,
        })
        .collect()),
        "iris" => {
            let mut out = Vec::new();
            for variant in ["fabric", "neoforge"] {
                if let Ok(list) = iris::fetch_versions(&state.http, &game_version, variant).await {
                    for item in list {
                        out.push(LoaderVersionOption {
                            label: format!("{} ({})", item.version_number, variant),
                            version: item.version_number,
                            variant: Some(variant.to_string()),
                            stable: None,
                        });
                    }
                }
            }
            if out.is_empty() {
                return Err(AppError::not_found(format!(
                    "Minecraft {game_version} 没有可用的 Iris 版本"
                )));
            }
            Ok(out)
        }
        "optifine" => {
            let labels =
                recommend_service::optifine_build_labels(&state.http, &game_version).await?;
            if labels.is_empty() {
                return Err(AppError::not_found(format!(
                    "Minecraft {game_version} 没有可用的 OptiFine 版本"
                )));
            }
            Ok(labels
                .into_iter()
                .map(|label| LoaderVersionOption {
                    version: label.clone(),
                    label,
                    variant: None,
                    stable: None,
                })
                .collect())
        }
        _ => Err(AppError::invalid_input("不支持的加载器类型")),
    }
}

struct LoaderArtifact {
    filename: String,
    url: String,
    size: Option<u64>,
    sha1: Option<String>,
}

fn file_name_from_url(url: &str) -> String {
    url.rsplit('/')
        .next()
        .unwrap_or("loader-artifact")
        .split('?')
        .next()
        .unwrap_or("loader-artifact")
        .to_string()
}

async fn loader_artifact(
    state: &AppState,
    loader: &str,
    game_version: &str,
    loader_version: &str,
    variant: Option<&str>,
) -> Result<LoaderArtifact> {
    match loader {
        "fabric" | "quilt" => {
            let host = if loader == "fabric" {
                "https://meta.fabricmc.net/v2"
            } else {
                "https://meta.quiltmc.org/v3"
            };
            Ok(LoaderArtifact {
                filename: format!("{loader}-loader-{loader_version}-{game_version}.json"),
                url: format!("{host}/versions/loader/{game_version}/{loader_version}/profile/json"),
                size: None,
                sha1: None,
            })
        }
        "forge" | "neoforge" => {
            let kind = if loader == "forge" {
                InstallerKind::Forge
            } else {
                InstallerKind::NeoForge
            };
            let plan =
                installer_service::plan(&state.http, game_version, kind, Some(loader_version))
                    .await?;
            Ok(LoaderArtifact {
                filename: file_name_from_url(&plan.installer_url),
                url: plan.installer_url,
                size: None,
                sha1: None,
            })
        }
        "iris" => {
            let variant = variant.unwrap_or("fabric");
            let list = iris::fetch_versions(&state.http, game_version, variant).await?;
            let item = list
                .into_iter()
                .find(|v| v.version_number == loader_version)
                .ok_or_else(|| {
                    AppError::not_found(format!(
                        "Iris {loader_version}（{variant}）不支持 Minecraft {game_version}"
                    ))
                })?;
            Ok(LoaderArtifact {
                filename: item.file_name,
                url: item.url,
                size: Some(item.size),
                sha1: item.sha1,
            })
        }
        "optifine" => {
            let info = recommend_service::optifine_build_info(&state.http, game_version)
                .await?
                .ok_or_else(|| {
                    AppError::not_found(format!("Minecraft {game_version} 没有可用的 OptiFine"))
                })?;
            Ok(LoaderArtifact {
                filename: info.filename,
                url: info.url,
                size: None,
                sha1: None,
            })
        }
        _ => Err(AppError::invalid_input("不支持的加载器类型")),
    }
}

#[tauri::command]
pub async fn download_loader_resource(
    state: State<'_, AppState>,
    loader: String,
    game_version: String,
    loader_version: String,
    variant: Option<String>,
    directory: String,
) -> Result<ResourceTaskAccepted> {
    let loader = loader_key(&loader)?;
    validate_loader_token(&game_version, "Minecraft 版本")?;
    validate_loader_token(&loader_version, "加载器版本")?;
    if directory.trim().is_empty() {
        return Err(AppError::invalid_input("请选择下载目录"));
    }
    let artifact = loader_artifact(
        &state,
        loader,
        &game_version,
        &loader_version,
        variant.as_deref(),
    )
    .await?;
    let title = format!(
        "下载 {} {} · {game_version}",
        loader_display(loader),
        loader_version
    );
    let task = state.downloads.create("resource", &title, 1, 0).await;
    let task_id = task.id.clone();
    let _ = state.downloads.save().await;
    let state = state.inner().clone();
    let dir = PathBuf::from(directory.trim());
    let spawn_title = title.clone();
    tauri::async_runtime::spawn(async move {
        let cancel = task.cancel_token();
        let result = resource_install::download_artifact_to_directory(
            &state.http,
            &artifact.url,
            &artifact.filename,
            artifact.size,
            artifact.sha1.as_deref(),
            &dir,
            &task,
            &cancel,
        )
        .await;
        if let Err(error) = result {
            tracing::warn!(%spawn_title, %error, "loader artifact download failed");
        }
        let _ = state.downloads.save().await;
    });
    Ok(ResourceTaskAccepted { task_id, title })
}

#[tauri::command]
pub async fn install_loader_resource(
    state: State<'_, AppState>,
    loader: String,
    game_version: String,
    loader_version: String,
    variant: Option<String>,
    instance_id: String,
) -> Result<ResourceTaskAccepted> {
    let loader = loader_key(&loader)?;
    validate_loader_token(&game_version, "Minecraft 版本")?;
    if loader == "optifine" {
        validate_optifine_label(&loader_version)?;
    } else {
        validate_loader_token(&loader_version, "加载器版本")?;
    }
    match loader {
        "fabric" | "quilt" => {
            let kind = if loader == "fabric" {
                LoaderKind::Fabric
            } else {
                LoaderKind::Quilt
            };
            let accepted = install_loader(
                state,
                kind,
                game_version.clone(),
                loader_version.clone(),
                Some(instance_id),
            )
            .await?;
            Ok(ResourceTaskAccepted {
                task_id: accepted.task_id,
                title: format!(
                    "安装 {} {} · {game_version}",
                    loader_display(loader),
                    loader_version
                ),
            })
        }
        "forge" | "neoforge" => {
            let kind = if loader == "forge" {
                InstallerKind::Forge
            } else {
                InstallerKind::NeoForge
            };
            let accepted = install_installer_loader(
                state,
                kind,
                game_version.clone(),
                Some(loader_version.clone()),
                Some(instance_id),
            )
            .await?;
            Ok(ResourceTaskAccepted {
                task_id: accepted.task_id,
                title: format!(
                    "安装 {} {} · {game_version}",
                    loader_display(loader),
                    loader_version
                ),
            })
        }
        "iris" => {
            let game = state.game_snapshot().await;
            let instance = InstanceManager::new(game.clone()).get(&instance_id).await?;
            let base = instance_base_version(&state, &game, &instance).await?;
            if base != game_version {
                return Err(AppError::invalid_input(format!(
                    "实例基础版本是 {base}，与所选 Minecraft {game_version} 不一致"
                )));
            }
            let instance_loader = instance
                .mod_loader
                .as_ref()
                .map(|m| m.kind.to_ascii_lowercase())
                .unwrap_or_default();
            let variant = variant.unwrap_or_else(|| {
                if instance_loader == "quilt" {
                    "quilt".into()
                } else {
                    "fabric".into()
                }
            });
            if !variant.eq_ignore_ascii_case(&instance_loader) && instance_loader != "quilt" {
                return Err(AppError::new(
                    ErrorCode::Unsupported,
                    format!("Iris 需要 {variant} 实例，当前实例加载器为 {instance_loader}"),
                ));
            }
            let list = iris::fetch_versions(&state.http, &game_version, &variant).await?;
            let item = list
                .into_iter()
                .find(|v| v.version_number == loader_version)
                .ok_or_else(|| {
                    AppError::not_found(format!(
                        "Iris {loader_version} 不支持 Minecraft {game_version}"
                    ))
                })?;
            let file = sr_core::modrinth::ResourceFile {
                filename: item.file_name.clone(),
                url: item.url.clone(),
                size: item.size,
                primary: true,
                sha1: item.sha1.clone(),
                sha512: None,
                file_type: None,
            };
            let title = format!("安装 Iris {} · {game_version}", loader_version);
            let task = state.downloads.create("resource", &title, 1, 0).await;
            let task_id = task.id.clone();
            let _ = state.downloads.save().await;
            let state = state.inner().clone();
            let spawn_title = title.clone();
            tauri::async_runtime::spawn(async move {
                let cancel = task.cancel_token();
                let result = resource_install::install_file_to_instance(
                    &state.http,
                    "mod",
                    &file,
                    &instance,
                    &task,
                    &cancel,
                )
                .await;
                if let Err(error) = result {
                    tracing::warn!(%spawn_title, %error, "iris install failed");
                }
                let _ = state.downloads.save().await;
            });
            Ok(ResourceTaskAccepted { task_id, title })
        }
        "optifine" => {
            let game = state.game_snapshot().await;
            let instance = InstanceManager::new(game.clone()).get(&instance_id).await?;
            let base = instance_base_version(&state, &game, &instance).await?;
            if base != game_version {
                return Err(AppError::invalid_input(format!(
                    "实例基础版本是 {base}，与所选 Minecraft {game_version} 不一致"
                )));
            }
            let is_forge = instance
                .mod_loader
                .as_ref()
                .map(|m| m.kind.eq_ignore_ascii_case("forge"))
                .unwrap_or(false)
                || instance.version_id.contains("-forge-");
            if !is_forge {
                return Err(AppError::new(
                    ErrorCode::Unsupported,
                    "OptiFine 直装仅支持 Forge 实例；Fabric/Quilt 请使用 Iris",
                ));
            }
            let requested = loader_version.clone();
            let title = format!("安装 OptiFine {loader_version} · {game_version}");
            let task = state.downloads.create("resource", &title, 1, 0).await;
            let task_id = task.id.clone();
            let _ = state.downloads.save().await;
            let state = state.inner().clone();
            let spawn_title = title.clone();
            tauri::async_runtime::spawn(async move {
                match optifine_install_inner(&state, &instance_id, Some(&requested)).await {
                    Ok(_) => task.complete().await,
                    Err(error) => {
                        task.fail(error.clone()).await;
                        tracing::warn!(%spawn_title, %error, "optifine install failed");
                    }
                }
                let _ = state.downloads.save().await;
            });
            Ok(ResourceTaskAccepted { task_id, title })
        }
        _ => Err(AppError::invalid_input("不支持的加载器类型")),
    }
}

#[tauri::command]
pub async fn list_crash_reports(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<CrashFile>> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    crash_service::list(&instance).await
}

#[tauri::command]
pub async fn delete_crash_report(
    state: State<'_, AppState>,
    instance_id: String,
    file_name: String,
) -> Result<()> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    crash_service::delete(&instance, &file_name).await
}

#[tauri::command]
pub async fn analyze_crash(
    state: State<'_, AppState>,
    instance_id: String,
    file_name: String,
) -> Result<CrashAnalysis> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    crash_service::analyze(&instance, &file_name).await
}

// ---------------------------------------------------------------------------
// 鸦鸭 assistant (DeepSeek)
// ---------------------------------------------------------------------------

const ASSISTANT_KEY_MARKER: &str = "assistant-deepseek-key";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantStatus {
    pub configured: bool,
    pub model: String,
    pub base_url: String,
    pub audit_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantToolTrace {
    pub name: String,
    pub summary: String,
    pub ok: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantReply {
    pub content: String,
    pub tool_traces: Vec<AssistantToolTrace>,
    pub proposals: Vec<AssistantProposal>,
    pub model: String,
    pub conversation_id: String,
    pub title: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantProposal {
    pub kind: String,
    pub id: String,
    pub title: String,
    pub summary: String,
    pub plan: serde_json::Value,
}

fn assistant_key_marker(state: &AppState) -> PathBuf {
    state.paths.base().join(ASSISTANT_KEY_MARKER)
}

fn has_assistant_key(state: &AppState) -> bool {
    assistant_key_marker(state).is_file()
}

fn assistant_api_key(state: &AppState) -> Result<String> {
    if !has_assistant_key(state) {
        return Err(
            AppError::new(ErrorCode::NotConfigured, "尚未配置 DeepSeek API key")
                .with_detail("请在设置 → 鸦鸭 AI 助手中配置 API key"),
        );
    }
    let store = KeyringStore::new("sr-launcher");
    assistant::load_key(&store)?.ok_or_else(|| {
        AppError::new(
            ErrorCode::NotConfigured,
            "DeepSeek API key 不存在或无法读取",
        )
    })
}

fn assistant_client(state: &AppState, settings: &Settings) -> Result<assistant::DeepSeekClient> {
    assistant::DeepSeekClient::new(
        state.http.clone(),
        assistant_api_key(state)?,
        &settings.assistant_base_url,
        &settings.assistant_model,
    )
}

#[tauri::command]
pub async fn assistant_status(state: State<'_, AppState>) -> Result<AssistantStatus> {
    let settings = state.settings_snapshot().await;
    Ok(AssistantStatus {
        configured: has_assistant_key(&state),
        model: settings.assistant_model.clone(),
        base_url: settings.assistant_base_url.clone(),
        audit_path: state
            .paths
            .base()
            .join("assistant-audit.jsonl")
            .display()
            .to_string(),
    })
}

#[tauri::command]
pub async fn assistant_conversations(
    state: State<'_, AppState>,
) -> Result<Vec<assistant_history::ConversationSummary>> {
    assistant_history::list(&state.paths).await
}

#[tauri::command]
pub async fn assistant_conversation(
    state: State<'_, AppState>,
    id: String,
) -> Result<assistant_history::Conversation> {
    assistant_history::load(&state.paths, &id).await
}

#[tauri::command]
pub async fn assistant_delete_conversation(state: State<'_, AppState>, id: String) -> Result<()> {
    assistant_history::delete(&state.paths, &id).await
}

#[tauri::command]
pub async fn set_deepseek_api_key(state: State<'_, AppState>, key: String) -> Result<()> {
    let store = KeyringStore::new("sr-launcher");
    assistant::save_key(&store, &key)?;
    tokio::fs::write(assistant_key_marker(&state), b"1")
        .await
        .map_err(|e| AppError::internal(format!("无法写入 AI key 标记：{e}")))
}

#[tauri::command]
pub async fn clear_deepseek_api_key(state: State<'_, AppState>) -> Result<()> {
    let marker = assistant_key_marker(&state);
    if tokio::fs::try_exists(&marker).await.unwrap_or(false) {
        let _ = tokio::fs::remove_file(&marker).await;
    }
    let store = KeyringStore::new("sr-launcher");
    if let Err(error) = assistant::clear_key(&store) {
        tracing::warn!(%error, "could not delete DeepSeek key from keychain");
    }
    Ok(())
}

#[tauri::command]
pub async fn test_deepseek_api_key(state: State<'_, AppState>) -> Result<()> {
    let settings = state.settings_snapshot().await;
    assistant_client(&state, &settings)?.test().await
}

#[tauri::command]
pub async fn assistant_local_report(
    state: State<'_, AppState>,
    instance_id: String,
    crash_file: Option<String>,
) -> Result<assistant::LocalReport> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    assistant::local_report(&instance, crash_file.as_deref(), true).await
}

async fn run_assistant_tool(
    state: &AppState,
    name: &str,
    args: &serde_json::Value,
    default_instance: Option<&str>,
) -> Result<serde_json::Value> {
    match name {
        "get_instance_diagnostics" => {
            let id = args
                .get("instance_id")
                .and_then(|v| v.as_str())
                .or(default_instance)
                .ok_or_else(|| AppError::invalid_input("缺少 instance_id"))?;
            let game = state.game_snapshot().await;
            let instance = InstanceManager::new(game).get(id).await?;
            let crash_file = args.get("crash_file").and_then(|v| v.as_str());
            let report = assistant::local_report(&instance, crash_file, true).await?;
            Ok(serde_json::to_value(report)?)
        }
        "read_instance_log" => {
            let id = args
                .get("instance_id")
                .and_then(|v| v.as_str())
                .or(default_instance)
                .ok_or_else(|| AppError::invalid_input("缺少 instance_id"))?;
            let file = args
                .get("file")
                .and_then(|v| v.as_str())
                .unwrap_or("latest.log");
            let game = state.game_snapshot().await;
            let instance = InstanceManager::new(game).get(id).await?;
            let log = assistant::read_log(&instance, file).await?;
            Ok(serde_json::to_value(log)?)
        }
        "get_instance_info" => {
            let id = args
                .get("instance_id")
                .and_then(|v| v.as_str())
                .or(default_instance)
                .ok_or_else(|| AppError::invalid_input("缺少 instance_id"))?;
            let game = state.game_snapshot().await;
            let instance = InstanceManager::new(game.clone()).get(id).await?;
            let base = instance_base_version(state, &game, &instance).await?;
            Ok(serde_json::json!({
                "id": instance.id,
                "name": instance.name,
                "versionId": instance.version_id,
                "baseVersion": base,
                "loader": instance.mod_loader,
                "memoryMb": instance.memory_mb,
                "javaPath": instance.java_path,
            }))
        }
        "list_instance_mods" => {
            let id = args
                .get("instance_id")
                .and_then(|v| v.as_str())
                .or(default_instance)
                .ok_or_else(|| AppError::invalid_input("缺少 instance_id"))?;
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let game = state.game_snapshot().await;
            let instance = InstanceManager::new(game).get(id).await?;
            let entries = mods_service::list(&instance).await?;
            let rows: Vec<_> = entries
                .into_iter()
                .filter(|m| query.is_empty() || m.file_name.to_ascii_lowercase().contains(&query))
                .take(200)
                .map(|m| {
                    serde_json::json!({
                        "fileName": m.file_name,
                        "enabled": m.enabled,
                        "size": m.size,
                    })
                })
                .collect();
            Ok(serde_json::json!({ "mods": rows }))
        }
        "search_mods" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if query.is_empty() {
                return Err(AppError::invalid_input("搜索关键词不能为空"));
            }
            let target_instance = args
                .get("instance_id")
                .and_then(|v| v.as_str())
                .or(default_instance);
            let mut default_version = None;
            let mut default_loader = None;
            if let Some(id) = target_instance {
                let game = state.game_snapshot().await;
                if let Ok(instance) = InstanceManager::new(game.clone()).get(id).await {
                    default_version = instance_base_version(state, &game, &instance).await.ok();
                    default_loader = instance
                        .mod_loader
                        .as_ref()
                        .map(|m| m.kind.to_ascii_lowercase())
                        .filter(|k| k != "vanilla");
                }
            }
            let game_version = args
                .get("game_version")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string())
                .or(default_version);
            let loader = args
                .get("loader")
                .and_then(|v| v.as_str())
                .map(|v| v.to_ascii_lowercase())
                .or(default_loader);
            if let Some(loader) = loader.as_deref() {
                if !matches!(loader, "fabric" | "forge" | "quilt" | "neoforge") {
                    return Err(AppError::invalid_input("加载器参数非法"));
                }
            }
            let category = args
                .get("category")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let index = match args.get("sort").and_then(|v| v.as_str()) {
                Some("downloads") => "downloads",
                Some("newest") => "newest",
                Some("updated") => "updated",
                _ => "relevance",
            };
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(8)
                .clamp(1, 30) as u32;
            let offset = args
                .get("offset")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                .min(10_000) as u32;
            let page = state
                .modrinth
                .search(&sr_core::modrinth::ResourceSearchQuery {
                    query,
                    kind: "mod".into(),
                    game_version,
                    loader,
                    category,
                    index: Some(index.into()),
                    offset: Some(offset),
                    limit: Some(limit),
                })
                .await?;
            Ok(serde_json::json!({
                "totalHits": page.total_hits,
                "offset": page.offset,
                "limit": page.limit,
                "hits": page.hits.into_iter().map(|h| serde_json::json!({
                    "projectId": h.id,
                    "slug": h.slug,
                    "title": h.title,
                    "description": h.description,
                    "author": h.author,
                    "downloads": h.downloads,
                    "follows": h.follows,
                    "versions": h.versions.into_iter().take(10).collect::<Vec<_>>(),
                    "categories": h.categories,
                    "latestVersion": h.latest_version,
                    "iconUrl": h.icon_url,
                })).collect::<Vec<_>>()
            }))
        }
        "prepare_mod_install" => {
            let id = args
                .get("instance_id")
                .and_then(|v| v.as_str())
                .or(default_instance)
                .ok_or_else(|| AppError::invalid_input("缺少 instance_id"))?;
            let project_id = args
                .get("project_id")
                .and_then(|v| v.as_str())
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| AppError::invalid_input("缺少 project_id"))?;
            let version_id = args.get("version_id").and_then(|v| v.as_str());
            let game = state.game_snapshot().await;
            let instance = InstanceManager::new(game.clone()).get(id).await?;
            let base = instance_base_version(state, &game, &instance).await?;
            let loader = instance
                .mod_loader
                .as_ref()
                .map(|m| m.kind.to_ascii_lowercase())
                .unwrap_or_default();
            if loader.is_empty() || loader == "vanilla" {
                return Err(AppError::new(
                    ErrorCode::Unsupported,
                    "实例没有模组加载器，无法安装模组",
                ));
            }
            let plan = mod_install::plan(
                &state.modrinth,
                &instance,
                project_id,
                version_id,
                &base,
                &loader,
            )
            .await?;
            mod_install::save_plan(&state.paths, &plan).await?;
            Ok(serde_json::to_value(plan)?)
        }
        "prepare_modpack_install" => {
            let id = args
                .get("instance_id")
                .and_then(|v| v.as_str())
                .or(default_instance)
                .ok_or_else(|| AppError::invalid_input("缺少 instance_id"))?;
            let projects: Vec<String> = args
                .get("project_ids")
                .and_then(|v| v.as_array())
                .map(|rows| {
                    rows.iter()
                        .filter_map(|v| v.as_str())
                        .filter(|v| !v.trim().is_empty())
                        .map(|v| v.to_string())
                        .collect()
                })
                .unwrap_or_default();
            let game = state.game_snapshot().await;
            let instance = InstanceManager::new(game.clone()).get(id).await?;
            let base = instance_base_version(state, &game, &instance).await?;
            let loader = instance
                .mod_loader
                .as_ref()
                .map(|m| m.kind.to_ascii_lowercase())
                .unwrap_or_default();
            if loader.is_empty() || loader == "vanilla" {
                return Err(AppError::new(
                    ErrorCode::Unsupported,
                    "实例没有模组加载器，无法规划整合包",
                ));
            }
            let plan =
                mod_install::plan_many(&state.modrinth, &instance, &projects, &base, &loader)
                    .await?;
            mod_install::save_plan(&state.paths, &plan).await?;
            Ok(serde_json::to_value(plan)?)
        }
        _ => Err(AppError::invalid_input(format!("未知工具：{name}"))),
    }
}

fn tool_summary(name: &str, result: &serde_json::Value, ok: bool) -> String {
    if !ok {
        return format!("{name} 执行失败");
    }
    match name {
        "get_instance_diagnostics" => {
            let crash = result
                .get("crash")
                .and_then(|c| c.get("fileName"))
                .and_then(|v| v.as_str())
                .unwrap_or("无崩溃报告");
            format!("已读取实例诊断与 {crash}")
        }
        "read_instance_log" => "已读取实例日志尾部".into(),
        "get_instance_info" => "已读取实例信息".into(),
        "list_instance_mods" => format!(
            "已读取模组列表（{} 项）",
            result
                .get("mods")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        ),
        "prepare_modpack_install" => format!(
            "已生成整合包安装计划（{} 个文件，需用户确认）",
            result
                .get("entries")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        ),
        "prepare_mod_install" => format!(
            "已生成模组安装计划（{} 个文件，需用户确认）",
            result
                .get("entries")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        ),
        "get_repair_plan" => format!(
            "已生成本地修复计划（{} 个低风险操作，需用户确认）",
            result
                .get("actions")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        ),
        "search_mods" => format!(
            "已搜索 Modrinth（{} 条结果）",
            result
                .get("hits")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        ),
        _ => "工具执行完成".into(),
    }
}

fn assistant_tools() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "get_instance_diagnostics",
                "description": "读取实例的本地诊断证据：最新崩溃报告（异常、Caused by、栈帧、原始行号、模组列表）与 latest.log 尾部。只读。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "instance_id": {"type": "string", "description": "实例 ID"},
                        "crash_file": {"type": "string", "description": "可选，指定 crash-reports 下的文件名"}
                    },
                    "required": []
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_instance_log",
                "description": "读取实例 logs/latest.log 或 logs/debug.log 的最后 256KB，并提取错误行。只读。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "instance_id": {"type": "string"},
                        "file": {"type": "string", "enum": ["latest.log", "debug.log"]}
                    },
                    "required": []
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_instance_info",
                "description": "读取实例的名称、版本、基础 Minecraft 版本、加载器、内存和 Java。只读。",
                "parameters": {
                    "type": "object",
                    "properties": {"instance_id": {"type": "string"}},
                    "required": []
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_instance_mods",
                "description": "列出实例 mods/ 中的文件，可按文件名过滤。只读。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "instance_id": {"type": "string"},
                        "query": {"type": "string"}
                    },
                    "required": []
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "search_mods",
                "description": "通过官方 Modrinth API 搜索模组，类似资源页：支持关键词、Minecraft 版本、加载器、分类、排序与分页。只返回搜索结果，不下载或安装。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "玩法、用途或模组名；可用同义词多次搜索"},
                        "instance_id": {"type": "string", "description": "可选；省略版本/加载器时从此实例继承"},
                        "game_version": {"type": "string"},
                        "loader": {"type": "string", "enum": ["fabric", "forge", "quilt", "neoforge"]},
                        "category": {"type": "string", "description": "Modrinth 分类，如 optimization、adventure、technology、magic、storage、utility"},
                        "sort": {"type": "string", "enum": ["relevance", "downloads", "newest", "updated"]},
                        "offset": {"type": "integer", "minimum": 0},
                        "limit": {"type": "integer", "minimum": 1, "maximum": 30}
                    },
                    "required": ["query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "prepare_mod_install",
                "description": "根据 project_id 解析模组版本与必需依赖，生成安装计划（含文件、哈希、来源）。只生成计划，不下载；执行必须由用户确认。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "instance_id": {"type": "string"},
                        "project_id": {"type": "string"},
                        "version_id": {"type": "string"}
                    },
                    "required": ["project_id"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "prepare_modpack_install",
                "description": "为多个 Modrinth 项目生成统一的整合包安装计划，自动解析必需依赖。只生成计划，不下载；执行必须由用户确认。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "instance_id": {"type": "string"},
                        "project_ids": {
                            "type": "array",
                            "items": {"type": "string"},
                            "minItems": 1,
                            "maxItems": 20
                        }
                    },
                    "required": ["project_ids"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_repair_plan",
                "description": "基于本地证据生成低风险修复计划（提高内存、禁用疑似模组）。只生成计划，不执行；执行必须由用户在界面确认。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "instance_id": {"type": "string"},
                        "crash_file": {"type": "string"}
                    },
                    "required": []
                }
            }
        }
    ])
}

const ASSISTANT_SYSTEM_PROMPT: &str = r#"你是「鸦鸭」，结构重组（SRL）Minecraft 启动器的内置助手。
规则：
1. 只根据工具返回的本地证据、用户提供的上下文和可核对的公开信息回答，不得伪造日志、版本、源码、测试结果或下载地址。
2. 诊断时先调用 get_instance_diagnostics；必要时再读取日志。引用证据必须带文件名和行号。
3. 明确区分【事实】（日志可直接证明）、【推断】（可能原因）、【未知】（证据不足）。证据不足时直接说明无法确定根因。
4. 不要把第一条栈帧、最后一个异常或最先出现的模组直接认定为根因。
5. 不要请求或输出 API key；不要建议关闭安全校验；不要执行或建议任意 shell 命令。
6. 你可以调用 get_repair_plan 生成低风险修复计划，或 prepare_mod_install 生成带依赖的模组安装计划；但不能执行。任何下载、安装或修改都必须由用户在界面确认。不要建议 shell 命令。
7. 安装模组前先用 search_mods 获取 project_id，再调用 prepare_mod_install；整合包使用 prepare_modpack_install 传入多个 project_id。不要猜测项目 ID。
8. 搜索模组时像资源库一样智能：理解玩法/用途，尝试同义词和分类（如 optimization、adventure、technology、magic、utility、storage），可多次搜索、对比下载量与版本，再给出 3–5 个候选和推荐理由。不要只搜一个关键词就下结论。
9. 回答必须简洁：先给结论，默认不超过 6 句话或 5 个要点；不要复述整段日志；使用 Markdown 的 **加粗**、短列表和必要的小代码块，避免大表格。
10. 用简体中文回答，正常使用标点，但不要堆砌符号。
"#;

fn truncate_tool_result(value: &serde_json::Value) -> String {
    let text = serde_json::to_string(value).unwrap_or_else(|_| "{}".into());
    if text.chars().count() <= 60000 {
        text
    } else {
        format!(
            "{}…（工具结果已截断）",
            text.chars().take(60000).collect::<String>()
        )
    }
}

#[tauri::command]
pub async fn assistant_chat(
    state: State<'_, AppState>,
    conversation_id: Option<String>,
    instance_id: Option<String>,
    message: String,
) -> Result<AssistantReply> {
    let message = message.trim().to_string();
    if message.is_empty() || message.chars().count() > 20000 {
        return Err(AppError::invalid_input("消息不能为空或超过 20000 字"));
    }
    let settings = state.settings_snapshot().await;
    let client = assistant_client(&state, &settings)?;
    let mut conversation = match conversation_id {
        Some(id) => assistant_history::load(&state.paths, &id).await?,
        None => assistant_history::new_conversation(instance_id.clone()),
    };
    if let Some(id) = instance_id.as_deref() {
        conversation.instance_id = Some(id.to_string());
    }
    if conversation.messages.is_empty() || conversation.title == "新对话" {
        conversation.title = assistant_history::title_from(&message);
    }
    conversation
        .messages
        .push(assistant_history::StoredMessage {
            role: "user".into(),
            content: message.clone(),
            traces: Vec::new(),
            proposals: Vec::new(),
            created_at: sr_core::util::now_rfc3339(),
        });
    conversation.updated_at = sr_core::util::now_rfc3339();
    assistant_history::save(&state.paths, &conversation).await?;

    let mut messages = vec![serde_json::json!({
        "role": "system",
        "content": ASSISTANT_SYSTEM_PROMPT
    })];
    if let Some(id) = conversation.instance_id.as_deref() {
        messages.push(serde_json::json!({
            "role": "system",
            "content": format!("当前选中的实例 ID 是 {id}。调用工具时可省略 instance_id。")
        }));
    }
    let mut selected = Vec::new();
    let mut char_budget = 0usize;
    for item in conversation.messages.iter().rev() {
        if !matches!(item.role.as_str(), "user" | "assistant") {
            continue;
        }
        if item.content.trim().is_empty() {
            continue;
        }
        let len = item.content.chars().count();
        if selected.len() >= 100 || char_budget.saturating_add(len) > 180_000 {
            break;
        }
        char_budget = char_budget.saturating_add(len);
        selected.push(item);
    }
    selected.reverse();
    for item in selected {
        messages.push(serde_json::json!({"role": item.role, "content": item.content}));
    }

    let tools = assistant_tools();
    let mut traces: Vec<AssistantToolTrace> = Vec::new();
    let mut proposals: Vec<AssistantProposal> = Vec::new();
    loop {
        let reply = client.complete(&messages, &tools).await?;
        let content = reply
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let calls = reply
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if calls.is_empty() {
            let content = if content.trim().is_empty() {
                "模型没有返回内容。".into()
            } else {
                content
            };
            conversation
                .messages
                .push(assistant_history::StoredMessage {
                    role: "assistant".into(),
                    content: content.clone(),
                    traces: serde_json::to_value(&traces)?
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                    proposals: serde_json::to_value(&proposals)?
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                    created_at: sr_core::util::now_rfc3339(),
                });
            conversation.updated_at = sr_core::util::now_rfc3339();
            assistant_history::save(&state.paths, &conversation).await?;
            return Ok(AssistantReply {
                content,
                tool_traces: traces,
                proposals,
                model: client.model().to_string(),
                conversation_id: conversation.id.clone(),
                title: conversation.title.clone(),
            });
        }
        messages.push(reply.clone());
        for (index, call) in calls.iter().enumerate() {
            let name = call
                .pointer("/function/name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let args_text = call
                .pointer("/function/arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let args: serde_json::Value =
                serde_json::from_str(args_text).unwrap_or_else(|_| serde_json::json!({}));
            let call_id = call
                .get("id")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string())
                .unwrap_or_else(|| format!("call-{index}"));
            let result =
                match run_assistant_tool(&state, &name, &args, instance_id.as_deref()).await {
                    Ok(value) => {
                        traces.push(AssistantToolTrace {
                            name: name.clone(),
                            summary: tool_summary(&name, &value, true),
                            ok: true,
                        });
                        value
                    }
                    Err(error) => {
                        traces.push(AssistantToolTrace {
                            name: name.clone(),
                            summary: error.message.clone(),
                            ok: false,
                        });
                        serde_json::json!({
                            "error": error.message,
                            "code": format!("{:?}", error.code),
                        })
                    }
                };
            if name == "get_repair_plan" && result.get("id").is_some() {
                proposals.push(AssistantProposal {
                    kind: "repair".into(),
                    id: result
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    title: "修复计划".into(),
                    summary: result
                        .get("actions")
                        .and_then(|v| v.as_array())
                        .map(|a| format!("{} 个低风险操作", a.len()))
                        .unwrap_or_else(|| "无可执行操作".into()),
                    plan: result.clone(),
                });
            } else if (name == "prepare_mod_install" || name == "prepare_modpack_install")
                && result.get("id").is_some()
            {
                proposals.push(AssistantProposal {
                    kind: "mod_install".into(),
                    id: result
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    title: if name == "prepare_modpack_install" {
                        "整合包安装计划".into()
                    } else {
                        "模组安装计划".into()
                    },
                    summary: result
                        .get("entries")
                        .and_then(|v| v.as_array())
                        .map(|a| format!("{} 个文件（含必需依赖）", a.len()))
                        .unwrap_or_default(),
                    plan: result.clone(),
                });
            }
            let audit = serde_json::json!({
                "time": sr_core::util::now_rfc3339(),
                "tool": name,
                "args": args,
                "ok": traces.last().map(|t| t.ok).unwrap_or(false),
                "summary": traces.last().map(|t| t.summary.clone()).unwrap_or_default(),
            });
            if let Err(error) =
                assistant::append_audit(&state.paths.base().join("assistant-audit.jsonl"), &audit)
                    .await
            {
                tracing::warn!(%error, "failed to write assistant audit log");
            }
            messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": truncate_tool_result(&result),
            }));
        }
    }
}

#[tauri::command]
pub async fn assistant_repair_plan(
    state: State<'_, AppState>,
    instance_id: String,
    crash_file: Option<String>,
) -> Result<repair::RepairPlan> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    let report = assistant::local_report(&instance, crash_file.as_deref(), true).await?;
    let plan = repair::plan_from_report(&report, &instance);
    repair::save_plan(&state.paths, &plan).await?;
    Ok(plan)
}

#[tauri::command]
pub async fn assistant_apply_repair(
    state: State<'_, AppState>,
    plan_id: String,
) -> Result<repair::RepairResult> {
    let plan = repair::load_plan(&state.paths, &plan_id).await?;
    let game = state.game_snapshot().await;
    let manager = InstanceManager::new(game);
    let instance = manager.get(&plan.instance_id).await?;
    let result = repair::apply(&state.paths, &manager, &instance, &plan).await?;
    let audit = serde_json::json!({
        "time": sr_core::util::now_rfc3339(),
        "tool": "repair.apply",
        "args": {"planId": plan.id, "instanceId": plan.instance_id},
        "ok": true,
        "summary": result.applied.join("；"),
        "rollbackToken": result.token,
    });
    if let Err(error) =
        assistant::append_audit(&state.paths.base().join("assistant-audit.jsonl"), &audit).await
    {
        tracing::warn!(%error, "failed to write repair audit log");
    }
    Ok(result)
}

#[tauri::command]
pub async fn assistant_rollback_repair(
    state: State<'_, AppState>,
    instance_id: String,
    token: String,
) -> Result<()> {
    let game = state.game_snapshot().await;
    let manager = InstanceManager::new(game);
    let instance = manager.get(&instance_id).await?;
    repair::rollback(&state.paths, &manager, &instance, &token).await?;
    let audit = serde_json::json!({
        "time": sr_core::util::now_rfc3339(),
        "tool": "repair.rollback",
        "args": {"instanceId": instance_id, "token": token},
        "ok": true,
        "summary": "已回滚修复",
    });
    if let Err(error) =
        assistant::append_audit(&state.paths.base().join("assistant-audit.jsonl"), &audit).await
    {
        tracing::warn!(%error, "failed to write repair audit log");
    }
    Ok(())
}

#[tauri::command]
pub async fn assistant_mod_plan(
    state: State<'_, AppState>,
    instance_id: String,
    project_id: String,
    version_id: Option<String>,
) -> Result<mod_install::ModInstallPlan> {
    let value = run_assistant_tool(
        &state,
        "prepare_mod_install",
        &serde_json::json!({
            "instance_id": instance_id,
            "project_id": project_id,
            "version_id": version_id,
        }),
        None,
    )
    .await?;
    serde_json::from_value(value).map_err(|e| AppError::internal(format!("安装计划解析失败：{e}")))
}

#[tauri::command]
pub async fn assistant_apply_mod_plan(
    state: State<'_, AppState>,
    plan_id: String,
) -> Result<mod_install::ModInstallResult> {
    let plan = mod_install::load_plan(&state.paths, &plan_id).await?;
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&plan.instance_id).await?;
    let title = format!(
        "安装模组 {}（{} 个文件）",
        plan.entries
            .first()
            .map(|e| e.title.clone())
            .unwrap_or_else(|| "依赖".into()),
        plan.entries.len()
    );
    let total: u64 = plan.entries.iter().map(|e| e.size).sum();
    let task = state
        .downloads
        .create("resource", &title, plan.entries.len() as u32, total)
        .await;
    let _ = state.downloads.save().await;
    let cancel = task.cancel_token();
    let result =
        mod_install::apply_with_http(&state.http, &state.paths, &instance, &plan, &task, &cancel)
            .await;
    let audit = match &result {
        Ok(value) => serde_json::json!({
            "time": sr_core::util::now_rfc3339(),
            "tool": "mod.install",
            "args": {"planId": plan.id, "instanceId": plan.instance_id, "entries": plan.entries.len()},
            "ok": true,
            "summary": value.installed.join("；"),
            "rollbackToken": value.token,
        }),
        Err(error) => serde_json::json!({
            "time": sr_core::util::now_rfc3339(),
            "tool": "mod.install",
            "args": {"planId": plan.id, "instanceId": plan.instance_id},
            "ok": false,
            "summary": error.message,
        }),
    };
    if let Err(error) =
        assistant::append_audit(&state.paths.base().join("assistant-audit.jsonl"), &audit).await
    {
        tracing::warn!(%error, "failed to write mod install audit log");
    }
    let _ = state.downloads.save().await;
    result
}

#[tauri::command]
pub async fn assistant_rollback_mod_install(
    state: State<'_, AppState>,
    instance_id: String,
    token: String,
) -> Result<()> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    mod_install::rollback(&state.paths, &instance, &token).await?;
    let audit = serde_json::json!({
        "time": sr_core::util::now_rfc3339(),
        "tool": "mod.rollback",
        "args": {"instanceId": instance_id, "token": token},
        "ok": true,
        "summary": "已回滚模组安装",
    });
    if let Err(error) =
        assistant::append_audit(&state.paths.base().join("assistant-audit.jsonl"), &audit).await
    {
        tracing::warn!(%error, "failed to write mod rollback audit log");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_diagnostics(
    state: State<'_, AppState>,
    run_network_check: bool,
) -> Result<DiagnosticsReport> {
    let game = state.game_snapshot().await;
    let javas = state.java.read().await.clone();
    let installed = state.versions.installed_versions(&game).await?;
    let instances = InstanceManager::new(game.clone()).list().await?;
    let network_ok = if run_network_check {
        let probe = state
            .http
            .head(sr_core::version::MANIFEST_URL)
            .timeout(std::time::Duration::from_secs(6))
            .send()
            .await;
        Some(probe.map(|r| r.status().is_success()).unwrap_or(false))
    } else {
        None
    };
    Ok(diagnostics::run_checks(
        &state.paths,
        &game,
        &javas,
        installed.len(),
        instances.len(),
        network_ok,
    )
    .await)
}

#[tauri::command]
pub async fn list_logs(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<LogFileInfo>> {
    diagnostics::list_logs(&state.paths, limit.unwrap_or(20).min(100)).await
}

#[tauri::command]
pub async fn read_log(
    state: State<'_, AppState>,
    path: String,
    lines: Option<usize>,
) -> Result<String> {
    diagnostics::read_log_tail(
        &state.paths,
        &PathBuf::from(path),
        lines.unwrap_or(200).min(2000),
    )
    .await
}

#[tauri::command]
pub async fn cleanup_logs(state: State<'_, AppState>) -> Result<diagnostics::LogCleanup> {
    diagnostics::cleanup_logs(
        &state.paths,
        diagnostics::LOG_RETENTION_DAYS,
        diagnostics::LOG_MAX_TOTAL_BYTES,
    )
    .await
}

#[tauri::command]
pub async fn delete_log(state: State<'_, AppState>, path: String) -> Result<()> {
    diagnostics::delete_log(&state.paths, &PathBuf::from(path)).await
}

#[tauri::command]
pub async fn export_diagnostics(
    state: State<'_, AppState>,
    run_network_check: bool,
) -> Result<String> {
    let game = state.game_snapshot().await;
    let javas = state.java.read().await.clone();
    let installed = state.versions.installed_versions(&game).await?;
    let instances = InstanceManager::new(game.clone()).list().await?;
    let network_ok = if run_network_check {
        state
            .http
            .head(sr_core::version::MANIFEST_URL)
            .timeout(std::time::Duration::from_secs(6))
            .send()
            .await
            .map(|r| r.status().is_success())
            .ok()
    } else {
        None
    };
    let report = diagnostics::run_checks(
        &state.paths,
        &game,
        &javas,
        installed.len(),
        instances.len(),
        network_ok,
    )
    .await;
    Ok(diagnostics::export_report(&report, None))
}

// ---------------------------------------------------------------------------
// Minecraft Multiplayer (52FRP)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn multiplayer_status(state: State<'_, AppState>) -> Result<MultiplayerStatus> {
    Ok(state.multiplayer.status().await)
}

#[tauri::command]
pub async fn multiplayer_connect(
    state: State<'_, AppState>,
    token: String,
) -> Result<MultiplayerAccount> {
    state.multiplayer.connect(&token).await
}

#[tauri::command]
pub async fn multiplayer_logout(state: State<'_, AppState>) -> Result<()> {
    state.multiplayer.logout().await
}

#[tauri::command]
pub async fn multiplayer_nodes(
    state: State<'_, AppState>,
    edition: Edition,
) -> Result<Vec<NodeView>> {
    state.multiplayer.nodes(edition).await
}

#[tauri::command]
pub async fn multiplayer_detect(
    state: State<'_, AppState>,
    instance_id: String,
    edition: Edition,
) -> Result<LocalServiceView> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&instance_id).await?;
    let pid = state.launches.read().await.pid(&instance_id).await;
    Ok(state
        .multiplayer
        .detect_local(&instance, pid, edition)
        .await)
}

#[tauri::command]
pub async fn multiplayer_create_room(
    state: State<'_, AppState>,
    request: CreateRoomRequest,
) -> Result<RoomView> {
    let game = state.game_snapshot().await;
    let instance = InstanceManager::new(game).get(&request.instance_id).await?;
    let pid = state.launches.read().await.pid(&request.instance_id).await;
    state
        .multiplayer
        .create_room(&request, &instance, pid)
        .await
}

#[tauri::command]
pub async fn multiplayer_stop_room(
    state: State<'_, AppState>,
    delete_tunnel: bool,
) -> Result<StopRoomResult> {
    state.multiplayer.stop_room(delete_tunnel).await
}

#[tauri::command]
pub async fn multiplayer_restart_frpc(state: State<'_, AppState>) -> Result<FrpcView> {
    state.multiplayer.restart_frpc().await
}

#[tauri::command]
pub async fn multiplayer_orphans(state: State<'_, AppState>) -> Result<Vec<OrphanTunnel>> {
    state.multiplayer.orphans().await
}

#[tauri::command]
pub async fn multiplayer_delete_orphan(state: State<'_, AppState>, tunnel_id: u64) -> Result<()> {
    state.multiplayer.delete_orphan(tunnel_id).await
}
