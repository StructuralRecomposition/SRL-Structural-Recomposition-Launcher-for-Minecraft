//! End-to-end launch pipeline with a fake Java executable.
//! This verifies real metadata loading, classpath construction, argument
//! substitution, process spawning, output capture and exit reporting without
//! requiring a Minecraft account or a real JVM.

#![cfg(unix)]

use sr_core::account::{offline_player, Account, AccountKind, AccountStatus};
use sr_core::instance::Instance;
use sr_core::java::JavaInfo;
use sr_core::launch::{LaunchEvent, LaunchManager, LaunchSink};
use sr_core::paths::GamePaths;
use sr_core::util;
use sr_core::version::VersionService;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[tokio::test]
async fn launches_fake_java_with_resolved_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let game = GamePaths::new(dir.path().join("mc"));
    game.ensure_layout().await.unwrap();
    std::fs::create_dir_all(game.version_dir("1.20.4")).unwrap();

    let jar_bytes = b"fake-client-jar";
    std::fs::write(game.version_jar("1.20.4"), jar_bytes).unwrap();
    let jar_sha = util::sha1_bytes(jar_bytes);

    let lib_rel = "g/a/1.0/a-1.0.jar";
    let lib_path = game.libraries_dir().join(lib_rel);
    std::fs::create_dir_all(lib_path.parent().unwrap()).unwrap();
    let lib_bytes = b"fake-library";
    std::fs::write(&lib_path, lib_bytes).unwrap();
    let lib_sha = util::sha1_bytes(lib_bytes);

    let version = serde_json::json!({
        "id": "1.20.4",
        "type": "release",
        "mainClass": "net.minecraft.client.main.Main",
        "assets": "12",
        "assetIndex": { "id": "12", "url": "https://piston-meta.mojang.com/v1/packages/x/12.json" },
        "downloads": { "client": { "sha1": jar_sha, "size": jar_bytes.len(), "url": "https://piston-data.mojang.com/v1/objects/x/client.jar" } },
        "javaVersion": { "majorVersion": 17 },
        "libraries": [
            { "name": "g:a:1.0", "downloads": { "artifact": {
                "path": lib_rel, "sha1": lib_sha, "size": lib_bytes.len(),
                "url": "https://libraries.minecraft.net/g/a/1.0/a-1.0.jar" } } }
        ],
        "arguments": {
            "jvm": ["-Djava.library.path=${natives_directory}", "-cp", "${classpath}"],
            "game": ["--username", "${auth_player_name}", "--version", "${version_name}",
                     "--gameDir", "${game_directory}", "--accessToken", "${auth_access_token}",
                     "--assetIndex", "${assets_index_name}"]
        }
    });
    std::fs::write(
        game.version_json("1.20.4"),
        serde_json::to_vec_pretty(&version).unwrap(),
    )
    .unwrap();

    let instance_dir = game.instances_dir().join("testinst");
    std::fs::create_dir_all(&instance_dir).unwrap();
    let instance = Instance {
        id: "testinst".into(),
        name: "Test".into(),
        version_id: "1.20.4".into(),
        game_dir: instance_dir.clone(),
        java_path: None,
        memory_mb: 1024,
        jvm_args: vec![],
        game_args: vec!["--demo".into()],
        created_at: util::now_rfc3339(),
        last_launched_at: None,
        account_id: None,
        resolution_width: None,
        resolution_height: None,
        mod_loader: None,
    };
    let account = Account {
        id: "a".into(),
        username: "Steve".into(),
        uuid: util::offline_uuid("Steve"),
        kind: AccountKind::Offline,
        status: AccountStatus::Offline,
        created_at: util::now_rfc3339(),
        last_used_at: None,
        skin_id: None,
        skin_model: None,
    };
    let java = JavaInfo {
        // /usr/bin/echo prints every argument it receives.
        path: PathBuf::from("/bin/echo"),
        version_string: "17.0.1".into(),
        major: 17,
        vendor: Some("fake".into()),
        arch: Some("x86_64".into()),
        source: "test".into(),
    };

    let events: Arc<Mutex<Vec<LaunchEvent>>> = Arc::new(Mutex::new(vec![]));
    let sink_events = events.clone();
    let sink: LaunchSink = Arc::new(move |e| sink_events.lock().unwrap().push(e));
    let mgr = LaunchManager::new(
        game.clone(),
        VersionService::new(reqwest::Client::new()),
        dir.path().join("logs"),
        sink,
    );

    let profile = offline_player(&account).unwrap();
    let result = mgr.launch(&instance, &profile, &java).await.unwrap();
    assert!(result.pid > 0);
    assert!(result.log_path.exists());
    assert!(result.warnings.is_empty());

    for _ in 0..60 {
        if !mgr.is_running("testinst").await {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!mgr.is_running("testinst").await);

    let text = std::fs::read_to_string(&result.log_path).unwrap();
    assert!(text.contains("--username"), "{text}");
    assert!(text.contains("Steve"), "{text}");
    assert!(text.contains("1.20.4"), "{text}");
    assert!(text.contains("--demo"), "{text}");
    assert!(text.contains("-Xmx1024M"), "{text}");
    assert!(
        text.contains("a-1.0.jar"),
        "classpath must include libraries: {text}"
    );
    assert!(
        text.contains("1.20.4.jar"),
        "classpath must include the client jar: {text}"
    );

    let evs = events.lock().unwrap();
    let exit = evs.iter().find(|e| e.kind == "exit").expect("exit event");
    assert_eq!(exit.code, Some(0));
    assert!(evs.iter().any(|e| e.kind == "log"));
}

#[tokio::test]
async fn refuses_launch_when_library_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let game = GamePaths::new(dir.path().join("mc"));
    game.ensure_layout().await.unwrap();
    std::fs::create_dir_all(game.version_dir("1.20.4")).unwrap();
    std::fs::write(game.version_jar("1.20.4"), b"jar").unwrap();
    let version = serde_json::json!({
        "id": "1.20.4", "mainClass": "M",
        "assetIndex": { "id": "12", "url": "https://piston-meta.mojang.com/x" },
        "downloads": { "client": { "sha1": util::sha1_bytes(b"jar"), "size": 3, "url": "https://piston-data.mojang.com/x" } },
        "javaVersion": { "majorVersion": 17 },
        "libraries": [{ "name": "g:a:1.0", "downloads": { "artifact": { "path": "g/a/1.0/a-1.0.jar", "sha1": "x", "size": 1, "url": "https://libraries.minecraft.net/x" } } }],
        "arguments": { "jvm": [], "game": [] }
    });
    std::fs::write(
        game.version_json("1.20.4"),
        serde_json::to_vec(&version).unwrap(),
    )
    .unwrap();

    let instance_dir = game.instances_dir().join("i");
    std::fs::create_dir_all(&instance_dir).unwrap();
    let instance = Instance {
        id: "i".into(),
        name: "I".into(),
        version_id: "1.20.4".into(),
        game_dir: instance_dir,
        java_path: None,
        memory_mb: 1024,
        jvm_args: vec![],
        game_args: vec![],
        created_at: util::now_rfc3339(),
        last_launched_at: None,
        account_id: None,
        resolution_width: None,
        resolution_height: None,
        mod_loader: None,
    };
    let account = Account {
        id: "a".into(),
        username: "Steve".into(),
        uuid: util::offline_uuid("Steve"),
        kind: AccountKind::Offline,
        status: AccountStatus::Offline,
        created_at: util::now_rfc3339(),
        last_used_at: None,
        skin_id: None,
        skin_model: None,
    };
    let java = JavaInfo {
        path: PathBuf::from("/bin/echo"),
        version_string: "17".into(),
        major: 17,
        vendor: None,
        arch: None,
        source: "test".into(),
    };
    let mgr = LaunchManager::new(
        game,
        VersionService::new(reqwest::Client::new()),
        dir.path().join("logs"),
        Arc::new(|_| {}),
    );
    let profile = offline_player(&account).unwrap();
    let err = mgr.launch(&instance, &profile, &java).await.unwrap_err();
    assert!(err.message.contains("不完整"), "{err}");
    assert!(!mgr.is_running("i").await);
}
