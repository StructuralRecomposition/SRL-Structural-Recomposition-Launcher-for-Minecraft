//! Real-network installation smoke test. Ignored by default because it
//! downloads ~60 MB from Mojang. Run with:
//!   cargo test -p sr-core --test real_install -- --ignored --nocapture

use sr_core::download::DownloadManager;
use sr_core::install::{InstallOptions, Installer};
use sr_core::paths::{AppPaths, GamePaths};
use sr_core::version::{RuleContext, VersionService};
use std::sync::Arc;

#[tokio::test]
#[ignore = "downloads real files from Mojang"]
async fn installs_real_1_12_2_without_assets() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(dir.path().join("data"));
    let game = GamePaths::new(dir.path().join("mc"));
    paths.ensure_layout().await.unwrap();
    game.ensure_layout().await.unwrap();

    let http = reqwest::Client::builder()
        .user_agent(sr_core::USER_AGENT)
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap();
    let versions = VersionService::new(http.clone());
    let downloads = DownloadManager::new(Arc::new(|_| {}));
    let task = downloads.create("install", "安装 1.12.2", 0, 0).await;

    let options = InstallOptions {
        include_assets: false,
        concurrency: 6,
        allow_custom_hosts: false,
    };
    Installer {
        versions: &versions,
        http: &http,
        paths: &paths,
        game: &game,
        downloads: &downloads,
        prepared: None,
        options,
    }
    .install(task.clone(), "1.12.2")
    .await
    .expect("install should succeed");

    let json = std::fs::read(game.version_json("1.12.2")).expect("version json");
    let meta = sr_core::version::VersionJson::parse(&json).unwrap();
    assert_eq!(meta.id, "1.12.2");
    assert!(meta.uses_legacy_arguments());

    let jar = game.version_jar("1.12.2");
    assert!(jar.exists());
    let sha = sr_core::util::sha1_file(&jar).await.unwrap();
    assert_eq!(sha, meta.client_download().unwrap().sha1);

    let natives = game.natives_dir("1.12.2");
    let native_count = std::fs::read_dir(&natives).unwrap().count();
    assert!(native_count > 0, "natives should be extracted");

    let report = versions
        .verify_installed(&game, &meta, false, false, &RuleContext::default())
        .await
        .unwrap();
    assert!(report.ok, "{:?}", report.issues);
    assert!(!report.assets_checked);

    let tasks = downloads.list().await;
    assert_eq!(tasks[0].status, sr_core::download::TaskStatus::Completed);
    println!(
        "installed 1.12.2: {} native entries, {} files checked",
        native_count, report.checked
    );
}
