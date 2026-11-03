use sr_core::download::{DownloadManager, TaskSink};
use sr_core::instance::Instance;
use sr_core::mod_install;
use sr_core::modrinth::ModrinthService;
use sr_core::paths::{AppPaths, GamePaths};
use sr_core::util;
use std::path::PathBuf;
use std::sync::Arc;

fn instance(dir: &std::path::Path) -> Instance {
    let game_dir = dir.join("instances/test");
    Instance {
        id: "testinstance".into(),
        name: "test".into(),
        version_id: "1.21.1".into(),
        game_dir: game_dir.clone(),
        java_path: None,
        memory_mb: 4096,
        jvm_args: Vec::new(),
        game_args: Vec::new(),
        created_at: util::now_rfc3339(),
        last_launched_at: None,
        account_id: None,
        resolution_width: None,
        resolution_height: None,
        mod_loader: Some(sr_core::instance::ModLoaderInfo {
            kind: "Fabric".into(),
            version: "0.16.10".into(),
            installed: true,
        }),
    }
}

#[tokio::test]
#[ignore = "downloads real files from Modrinth"]
async fn plans_installs_and_rolls_back_fabric_mod() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(dir.path());
    paths.ensure_layout().await.unwrap();
    let inst = instance(dir.path());
    tokio::fs::create_dir_all(inst.game_dir.join("mods"))
        .await
        .unwrap();
    let service = ModrinthService::new(reqwest::Client::new());
    let plan = mod_install::plan(
        &service, &inst, "AANobbMI", // Sodium
        None, "1.21.1", "fabric",
    )
    .await
    .unwrap();
    assert!(!plan.entries.is_empty());

    let sink: TaskSink = Arc::new(|_| {});
    let manager = DownloadManager::new(sink);
    let task = manager
        .create("resource", "test", plan.entries.len() as u32, 0)
        .await;
    let cancel = task.cancel_token();
    let result = mod_install::apply_with_http(
        &reqwest::Client::new(),
        &paths,
        &inst,
        &plan,
        &task,
        &cancel,
    )
    .await
    .unwrap();
    for name in &result.installed {
        assert!(inst.game_dir.join("mods").join(name).exists());
    }
    mod_install::rollback(&paths, &inst, &result.token)
        .await
        .unwrap();
    for name in &result.installed {
        assert!(!inst.game_dir.join("mods").join(name).exists());
    }
    let _ = GamePaths::new(PathBuf::from(&inst.game_dir));
}
