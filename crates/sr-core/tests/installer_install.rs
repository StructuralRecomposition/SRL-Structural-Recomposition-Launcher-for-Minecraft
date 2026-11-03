//! Real official-installer test (ignored by default). Prepare a game tree with
//! an installed base version, then:
//!   SR_TEST_GAME=/tmp/sr-installer-test/mc \
//!   SR_TEST_DATA=/tmp/sr-installer-test/data \
//!   SR_TEST_JAVA=/path/to/java \
//!   SR_TEST_FAMILY=forge|neoforge \
//!   cargo test -p sr-core --test installer_install -- --ignored --nocapture

use sr_core::download::DownloadManager;
use sr_core::installer::{self, InstallerKind};
use sr_core::paths::{AppPaths, GamePaths};
use sr_core::version::{RuleContext, VersionService};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::test]
#[ignore = "runs the official Forge/NeoForge installer"]
async fn installs_loader_with_official_installer() {
    let game_dir = std::env::var("SR_TEST_GAME").expect("SR_TEST_GAME");
    let data_dir = std::env::var("SR_TEST_DATA").expect("SR_TEST_DATA");
    let java = PathBuf::from(std::env::var("SR_TEST_JAVA").expect("SR_TEST_JAVA"));
    let family = std::env::var("SR_TEST_FAMILY").unwrap_or_else(|_| "forge".into());
    let kind = if family == "neoforge" {
        InstallerKind::NeoForge
    } else {
        InstallerKind::Forge
    };
    let game_version = std::env::var("SR_TEST_VERSION").unwrap_or_else(|_| "1.21.1".into());

    let paths = AppPaths::new(data_dir);
    paths.ensure_layout().await.unwrap();
    let game = GamePaths::new(game_dir);
    game.ensure_layout().await.unwrap();
    let http = reqwest::Client::builder()
        .user_agent(sr_core::USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .unwrap();
    let versions = VersionService::new(http.clone());
    let downloads = DownloadManager::new(Arc::new(|_| {}));
    let plan = installer::plan(&http, &game_version, kind, None)
        .await
        .expect("plan");
    println!(
        "{} {} -> {}",
        kind.name(),
        plan.loader_version,
        plan.version_id
    );
    let task = downloads.create("install", "installer", 0, 0).await;
    let cancel = task.cancel_token();
    let version_id =
        installer::install(&http, &versions, &paths, &game, &java, &plan, task, cancel)
            .await
            .expect("install");

    let meta = versions.load_installed(&game, &version_id).await.unwrap();
    assert_eq!(meta.inherits_from, None);
    assert_eq!(
        meta.extra.get("srBaseVersion").and_then(|v| v.as_str()),
        Some(game_version.as_str())
    );
    assert_eq!(meta.jar_id(), game_version);
    let report = versions
        .verify_installed(&game, &meta, false, false, &RuleContext::default())
        .await
        .unwrap();
    assert!(report.ok, "{:?}", report.issues);
    println!(
        "{} installed: {} libs",
        version_id,
        meta.effective_libraries(&RuleContext::default()).len()
    );
}
