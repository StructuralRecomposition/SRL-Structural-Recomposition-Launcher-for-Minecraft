//! Real-network Fabric integration test. Ignored by default:
//!   cargo test -p sr-core --test fabric_install -- --ignored --nocapture
//! Installs base 1.20.4 (without assets) and Fabric on top, then validates the
//! shared jar and classpath. Downloads ~100 MB from Mojang and Fabric.

use sr_core::download::DownloadManager;
use sr_core::fabric::FabricService;
use sr_core::install::{InstallOptions, Installer};
use sr_core::paths::{AppPaths, GamePaths};
use sr_core::version::{build_plan, LaunchOptions, PlayerProfile, RuleContext, VersionService};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::test]
#[ignore = "downloads real files from Mojang and Fabric"]
async fn installs_base_and_fabric_1_20_4() {
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
    let options = InstallOptions {
        include_assets: false,
        concurrency: 6,
        allow_custom_hosts: false,
    };

    let base_task = downloads.create("install", "base", 0, 0).await;
    Installer {
        versions: &versions,
        http: &http,
        paths: &paths,
        game: &game,
        downloads: &downloads,
        prepared: None,
        options: InstallOptions {
            include_assets: false,
            concurrency: 6,
            allow_custom_hosts: false,
        },
    }
    .install(base_task, "1.20.4")
    .await
    .expect("base install");

    let fabric = FabricService::new(http.clone());
    let resolved = fabric
        .resolve_for_install(&versions, &game, "1.20.4", "0.16.0")
        .await
        .expect("fabric resolve");
    assert_eq!(resolved.jar_id(), "1.20.4");

    let fabric_task = downloads.create("install", "fabric", 0, 0).await;
    Installer {
        versions: &versions,
        http: &http,
        paths: &paths,
        game: &game,
        downloads: &downloads,
        prepared: Some(resolved.clone()),
        options,
    }
    .install(fabric_task, &resolved.id)
    .await
    .expect("fabric install");

    let installed = versions.installed_versions(&game).await.unwrap();
    let fabric_row = installed
        .iter()
        .find(|v| v.id == resolved.id)
        .expect("fabric version listed");
    assert!(fabric_row.has_jar, "shared base jar should be detected");

    let meta = versions.load_installed(&game, &resolved.id).await.unwrap();
    let libs = meta.effective_libraries(&RuleContext::default());
    let loader_lib = libs
        .iter()
        .find(|l| l.name.starts_with("net.fabricmc:fabric-loader"))
        .expect("loader library");
    assert!(
        std::path::Path::new(&game.libraries_dir())
            .join(&loader_lib.path)
            .exists(),
        "loader jar must be downloaded"
    );

    // Classpath must use the base client jar, not a non-existent child jar.
    let ctx = RuleContext::default();
    let mut classpath: Vec<PathBuf> = vec![game.version_jar(&meta.jar_id())];
    classpath.extend(
        libs.iter()
            .filter(|l| !l.is_native)
            .map(|l| game.libraries_dir().join(&l.path)),
    );
    let plan = build_plan(
        &meta,
        &ctx,
        &LaunchOptions {
            game_dir: dir.path().join("instance"),
            assets_dir: game.assets_dir(),
            libraries_dir: game.libraries_dir(),
            natives_dir: game.natives_dir(&meta.jar_id()),
            log_config: None,
            classpath,
            asset_index: meta.asset_index().unwrap().id.clone(),
            version_type: "release".into(),
            player: PlayerProfile {
                name: "Steve".into(),
                uuid: sr_core::util::offline_uuid("Steve"),
                access_token: "0".into(),
                user_type: "legacy".into(),
            },
            memory_mb: 2048,
            extra_jvm_args: vec![],
            extra_game_args: vec![],
            resolution: None,
            launcher_name: "SRLauncher".into(),
            launcher_version: "0.1.0".into(),
        },
    )
    .unwrap();
    let cp = plan
        .arguments
        .iter()
        .find(|a| a.contains("fabric-loader"))
        .expect("classpath contains fabric loader");
    assert!(cp.contains("1.20.4.jar"), "base jar in classpath: {cp}");
    assert!(!cp.contains(&format!("{}.jar", resolved.id)));

    let report = versions
        .verify_installed(&game, &meta, false, false, &ctx)
        .await
        .unwrap();
    assert!(report.ok, "{:?}", report.issues);
    println!(
        "fabric {} installed on 1.20.4: {} libs, shared jar ok",
        resolved.id,
        libs.len()
    );

    // Quilt uses the same profile shape with a different meta service.
    let quilt = FabricService::quilt(http.clone());
    let quilt_resolved = quilt
        .resolve_for_install(&versions, &game, "1.20.4", "0.20.0-beta.9")
        .await
        .expect("quilt resolve");
    assert_eq!(quilt_resolved.jar_id(), "1.20.4");
    let quilt_task = downloads.create("install", "quilt", 0, 0).await;
    Installer {
        versions: &versions,
        http: &http,
        paths: &paths,
        game: &game,
        downloads: &downloads,
        prepared: Some(quilt_resolved.clone()),
        options: InstallOptions {
            include_assets: false,
            concurrency: 6,
            allow_custom_hosts: false,
        },
    }
    .install(quilt_task, &quilt_resolved.id)
    .await
    .expect("quilt install");

    let quilt_meta = versions
        .load_installed(&game, &quilt_resolved.id)
        .await
        .unwrap();
    let quilt_libs = quilt_meta.effective_libraries(&RuleContext::default());
    let quilt_loader = quilt_libs
        .iter()
        .find(|l| l.name.starts_with("org.quiltmc:quilt-loader"))
        .expect("quilt loader library");
    assert!(
        game.libraries_dir().join(&quilt_loader.path).exists(),
        "quilt loader jar must be downloaded"
    );
    let quilt_report = versions
        .verify_installed(&game, &quilt_meta, false, false, &ctx)
        .await
        .unwrap();
    assert!(quilt_report.ok, "{:?}", quilt_report.issues);
    println!(
        "quilt {} installed on 1.20.4: {} libs, shared jar ok",
        quilt_resolved.id,
        quilt_libs.len()
    );
}
