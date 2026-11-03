//! Quilt integration test with a synthetic base version. This avoids
//! re-downloading a full Minecraft client on every run while still using the
//! real Quilt meta service and real Quilt/Fabric Maven artifacts.
//!
//!   cargo test -p sr-core --test quilt_install -- --ignored --nocapture

use sr_core::download::DownloadManager;
use sr_core::fabric::FabricService;
use sr_core::install::{InstallOptions, Installer};
use sr_core::paths::{AppPaths, GamePaths};
use sr_core::version::{build_plan, LaunchOptions, PlayerProfile, RuleContext, VersionService};
use std::sync::Arc;

#[tokio::test]
#[ignore = "downloads real Quilt artifacts"]
async fn installs_quilt_on_synthetic_base() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(dir.path().join("data"));
    let game = GamePaths::new(dir.path().join("mc"));
    paths.ensure_layout().await.unwrap();
    game.ensure_layout().await.unwrap();
    std::fs::create_dir_all(game.version_dir("1.20.4")).unwrap();

    let jar_bytes = b"fake base client jar for quilt test";
    std::fs::write(game.version_jar("1.20.4"), jar_bytes).unwrap();
    let sha = sr_core::util::sha1_bytes(jar_bytes);
    let base = serde_json::json!({
        "id": "1.20.4",
        "type": "release",
        "mainClass": "net.minecraft.client.main.Main",
        "assets": "12",
        "assetIndex": { "id": "12", "url": "https://piston-meta.mojang.com/v1/packages/x/12.json" },
        "downloads": { "client": {
            "sha1": sha,
            "size": jar_bytes.len(),
            "url": "https://piston-data.mojang.com/v1/objects/x/client.jar"
        }},
        "javaVersion": { "majorVersion": 17 },
        "libraries": [],
        "arguments": { "game": ["--username", "${auth_player_name}"], "jvm": [] }
    });
    std::fs::write(
        game.version_json("1.20.4"),
        serde_json::to_vec_pretty(&base).unwrap(),
    )
    .unwrap();

    let http = reqwest::Client::builder()
        .user_agent(sr_core::USER_AGENT)
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap();
    let versions = VersionService::new(http.clone());
    let downloads = DownloadManager::new(Arc::new(|_| {}));
    let service = FabricService::quilt(http.clone());
    // Use a captured real Quilt profile so the test does not depend on the
    // meta endpoint being reachable; libraries are still downloaded for real.
    let profile: sr_core::version::VersionJson =
        serde_json::from_str(include_str!("fixtures/quilt_profile_1.20.4.json")).unwrap();
    let base_meta = versions.load_installed(&game, "1.20.4").await.unwrap();
    let resolved = service
        .resolve_profile(base_meta, "1.20.4", profile)
        .expect("quilt resolve");

    assert!(resolved.id.starts_with("quilt-loader-"));
    assert_eq!(resolved.jar_id(), "1.20.4");
    assert_eq!(
        resolved.main_class.as_deref(),
        Some("org.quiltmc.loader.impl.launch.knot.KnotClient")
    );

    let task = downloads.create("install", "quilt", 0, 0).await;
    Installer {
        versions: &versions,
        http: &http,
        paths: &paths,
        game: &game,
        downloads: &downloads,
        prepared: Some(resolved.clone()),
        options: InstallOptions {
            include_assets: false,
            concurrency: 4,
            allow_custom_hosts: false,
        },
    }
    .install(task, &resolved.id)
    .await
    .expect("quilt install");

    let meta = versions.load_installed(&game, &resolved.id).await.unwrap();
    let libs = meta.effective_libraries(&RuleContext::default());
    let loader = libs
        .iter()
        .find(|l| l.name.starts_with("org.quiltmc:quilt-loader"))
        .expect("quilt loader");
    assert!(game.libraries_dir().join(&loader.path).exists());
    assert!(
        libs.iter()
            .filter(|l| l.name.starts_with("org.quiltmc"))
            .all(|l| {
                let p = game.libraries_dir().join(&l.path);
                p.exists()
            }),
        "all quilt maven artifacts must be present"
    );

    let mut classpath = vec![game.version_jar(&meta.jar_id())];
    classpath.extend(
        libs.iter()
            .filter(|l| !l.is_native)
            .map(|l| game.libraries_dir().join(&l.path)),
    );
    let plan = build_plan(
        &meta,
        &RuleContext::default(),
        &LaunchOptions {
            game_dir: dir.path().join("instance"),
            assets_dir: game.assets_dir(),
            libraries_dir: game.libraries_dir(),
            natives_dir: game.natives_dir(&meta.jar_id()),
            log_config: None,
            classpath,
            asset_index: "12".into(),
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
        .find(|a| a.contains("quilt-loader"))
        .expect("quilt loader on classpath");
    assert!(cp.contains("1.20.4.jar"), "shared base jar: {cp}");
    assert!(!cp.contains(&format!("{}.jar", resolved.id)));

    println!(
        "quilt {} installed on synthetic base: {} libs, shared jar ok",
        resolved.id,
        libs.len()
    );
}
