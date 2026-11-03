//! End-to-end Fabric install using the BMCLAPI mirror and an existing base
//! version. Ignored by default; requires a prepared game tree:
//!
//!   SR_TEST_GAME=/tmp/sr-fabric-test/mc \
//!   SR_TEST_DATA=/tmp/sr-fabric-test/data \
//!   cargo test -p sr-core --test fabric_mirror_install -- --ignored --nocapture
//!
//! The tree must contain `versions/<game>/<game>.json`, the client jar, the
//! base libraries and the asset index. No base files are downloaded by this
//! test; only Fabric artifacts are fetched (via BMCLAPI).

use sr_core::download::DownloadManager;
use sr_core::fabric::FabricService;
use sr_core::install::{InstallOptions, Installer};
use sr_core::paths::{AppPaths, GamePaths};
use sr_core::version::{RuleContext, VersionService};
use std::sync::Arc;

#[tokio::test]
#[ignore = "requires SR_TEST_GAME/SR_TEST_DATA and network access to BMCLAPI"]
async fn installs_fabric_on_existing_base_via_mirror() {
    let game_dir = std::env::var("SR_TEST_GAME").expect("SR_TEST_GAME");
    let data_dir = std::env::var("SR_TEST_DATA").expect("SR_TEST_DATA");
    let game_version = std::env::var("SR_TEST_VERSION").unwrap_or_else(|_| "1.21.1".into());

    let paths = AppPaths::new(data_dir);
    paths.ensure_layout().await.unwrap();
    let game = GamePaths::new(game_dir);
    game.ensure_layout().await.unwrap();

    let http = reqwest::Client::builder()
        .user_agent(sr_core::USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .unwrap();
    let versions = VersionService::new(http.clone());
    let downloads = DownloadManager::new(Arc::new(|_| {}));
    let fabric = FabricService::new(http.clone()).with_mirror(true);

    let loaders = fabric.list_loaders(&game_version).await.expect("loaders");
    let loader = loaders
        .iter()
        .find(|l| l.stable)
        .unwrap_or(&loaders[0])
        .version
        .clone();
    println!("installing Fabric {loader} on {game_version}");
    let resolved = fabric
        .resolve_for_install(&versions, &game, &game_version, &loader)
        .await
        .expect("resolve fabric");
    assert_eq!(resolved.jar_id(), game_version);
    let version_id = resolved.id.clone();

    let task = downloads.create("install", "fabric-mirror", 0, 0).await;
    Installer {
        versions: &versions,
        http: &http,
        paths: &paths,
        game: &game,
        downloads: &downloads,
        prepared: Some(resolved),
        options: InstallOptions {
            include_assets: false,
            concurrency: 4,
            allow_custom_hosts: false,
        },
    }
    .install(task, &version_id)
    .await
    .expect("fabric install");

    let meta = versions
        .load_installed(&game, &version_id)
        .await
        .expect("saved metadata");
    let libs = meta.effective_libraries(&RuleContext::default());
    let loader_lib = libs
        .iter()
        .find(|l| l.name.starts_with("net.fabricmc:fabric-loader"))
        .expect("fabric-loader lib");
    assert!(game.libraries_dir().join(&loader_lib.path).exists());
    assert!(
        libs.iter()
            .filter(|l| l.name.starts_with("net.fabricmc:") || l.name.starts_with("org.ow2.asm:"))
            .all(|l| game.libraries_dir().join(&l.path).exists()),
        "all Fabric/ASM artifacts must be on disk"
    );
    let report = versions
        .verify_installed(&game, &meta, false, false, &RuleContext::default())
        .await
        .expect("verify");
    assert!(report.ok, "{:?}", report.issues);
    println!(
        "fabric {version_id} installed via mirror: {} libs, shared jar {}",
        libs.len(),
        meta.jar_id()
    );
}

#[tokio::test]
#[ignore = "requires SR_TEST_GAME/SR_TEST_DATA and network access to Quilt meta/BMCLAPI"]
async fn installs_quilt_on_existing_base_via_mirror() {
    let game_dir = std::env::var("SR_TEST_GAME").expect("SR_TEST_GAME");
    let data_dir = std::env::var("SR_TEST_DATA").expect("SR_TEST_DATA");
    let game_version = std::env::var("SR_TEST_VERSION").unwrap_or_else(|_| "1.21.1".into());

    let paths = AppPaths::new(data_dir);
    paths.ensure_layout().await.unwrap();
    let game = GamePaths::new(game_dir);
    game.ensure_layout().await.unwrap();

    let http = reqwest::Client::builder()
        .user_agent(sr_core::USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .unwrap();
    let versions = VersionService::new(http.clone());
    let downloads = DownloadManager::new(Arc::new(|_| {}));
    let quilt = FabricService::quilt(http.clone()).with_mirror(true);

    let loaders = quilt
        .list_loaders(&game_version)
        .await
        .expect("quilt loaders");
    let loader = loaders[0].version.clone();
    println!("installing Quilt {loader} on {game_version}");
    let resolved = quilt
        .resolve_for_install(&versions, &game, &game_version, &loader)
        .await
        .expect("resolve quilt");
    assert_eq!(resolved.jar_id(), game_version);
    let version_id = resolved.id.clone();

    let task = downloads.create("install", "quilt-mirror", 0, 0).await;
    Installer {
        versions: &versions,
        http: &http,
        paths: &paths,
        game: &game,
        downloads: &downloads,
        prepared: Some(resolved),
        options: InstallOptions {
            include_assets: false,
            concurrency: 4,
            allow_custom_hosts: false,
        },
    }
    .install(task, &version_id)
    .await
    .expect("quilt install");

    let meta = versions
        .load_installed(&game, &version_id)
        .await
        .expect("saved metadata");
    let libs = meta.effective_libraries(&RuleContext::default());
    let loader_lib = libs
        .iter()
        .find(|l| l.name.starts_with("org.quiltmc:quilt-loader"))
        .expect("quilt-loader lib");
    assert!(game.libraries_dir().join(&loader_lib.path).exists());
    let report = versions
        .verify_installed(&game, &meta, false, false, &RuleContext::default())
        .await
        .expect("verify");
    assert!(report.ok, "{:?}", report.issues);
    println!(
        "quilt {version_id} installed via mirror: {} libs, shared jar {}",
        libs.len(),
        meta.jar_id()
    );
}
