//! Real Modrinth `.mrpack` export smoke test.
//!
//! Run with:
//!   cargo test -p sr-core --test modpack_export_network -- --ignored --nocapture

use sr_core::instance::{Instance, ModLoaderInfo};
use sr_core::modpack_export::export_mrpack;
use sr_core::paths::GamePaths;
use std::io::Read;

#[tokio::test]
#[ignore = "downloads one small mod from Modrinth"]
async fn exports_real_mrpack_with_modrinth_resolution() {
    let temp = tempfile::tempdir().unwrap();
    let game = GamePaths::new(temp.path().join("game"));
    game.ensure_layout().await.unwrap();
    std::fs::create_dir_all(game.version_dir("1.21.1")).unwrap();
    std::fs::write(
        game.version_json("1.21.1"),
        br#"{"id":"1.21.1","type":"release"}"#,
    )
    .unwrap();

    let instance_dir = temp.path().join("instance");
    std::fs::create_dir_all(instance_dir.join("mods")).unwrap();
    std::fs::create_dir_all(instance_dir.join("config")).unwrap();
    std::fs::write(
        instance_dir.join("config/example.toml"),
        b"enabled = true\n",
    )
    .unwrap();

    let http = reqwest::Client::builder()
        .user_agent("SRL-test")
        .build()
        .unwrap();
    let url = "https://api.modrinth.com/v2/project/sodium/version?game_versions=%5B%221.21.1%22%5D&loaders=%5B%22fabric%22%5D";
    let versions: serde_json::Value = http.get(url).send().await.unwrap().json().await.unwrap();
    let file = versions
        .as_array()
        .and_then(|list| list.first())
        .and_then(|version| version.get("files"))
        .and_then(|files| files.as_array())
        .and_then(|files| files.first())
        .expect("sodium version file");
    let download = file["url"].as_str().unwrap();
    let name = file["filename"].as_str().unwrap();
    let bytes = http
        .get(download)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    std::fs::write(instance_dir.join("mods").join(name), &bytes).unwrap();
    std::fs::write(instance_dir.join("mods/manual-only.jar"), b"not a real mod").unwrap();

    let instance = Instance {
        id: "test".into(),
        name: "Test Pack".into(),
        version_id: "fabric-loader-0.16.9-1.21.1".into(),
        game_dir: instance_dir,
        java_path: None,
        memory_mb: 4096,
        jvm_args: vec![],
        game_args: vec![],
        created_at: "2026-01-01T00:00:00Z".into(),
        last_launched_at: None,
        account_id: None,
        resolution_width: None,
        resolution_height: None,
        mod_loader: Some(ModLoaderInfo {
            kind: "Fabric".into(),
            version: "0.16.9".into(),
            installed: true,
        }),
    };

    let dest = temp.path().join("pack.mrpack");
    let summary = export_mrpack(
        &game,
        &instance,
        &dest,
        false,
        Some("Test Pack".into()),
        Some("1.0.0".into()),
        &http,
    )
    .await
    .unwrap();
    assert_eq!(summary.minecraft_version, "1.21.1");
    assert_eq!(summary.resolved_mods, 1);
    assert_eq!(summary.bundled_mods, 1);
    assert!(summary.bytes > 0);

    let file = std::fs::File::open(&dest).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut index = String::new();
    zip.by_name("modrinth.index.json")
        .unwrap()
        .read_to_string(&mut index)
        .unwrap();
    let index: serde_json::Value = serde_json::from_str(&index).unwrap();
    assert_eq!(index["dependencies"]["minecraft"], "1.21.1");
    assert_eq!(index["dependencies"]["fabric-loader"], "0.16.9");
    assert_eq!(index["files"].as_array().unwrap().len(), 1);
    assert!(zip.by_name("overrides/mods/manual-only.jar").is_ok());
    assert!(zip.by_name("overrides/config/example.toml").is_ok());
}
