use sr_core::download::{DownloadManager, TaskSink};
use sr_core::java_runtime;
use sr_core::paths::AppPaths;
use std::sync::Arc;

#[tokio::test]
#[ignore = "downloads ~190MB from Adoptium"]
async fn installs_temurin_21() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(dir.path());
    paths.ensure_layout().await.unwrap();
    let sink: TaskSink = Arc::new(|_| {});
    let manager = DownloadManager::new(sink);
    let task = manager.create("java", "test", 1, 0).await;
    let cancel = task.cancel_token();
    let info = java_runtime::install(&reqwest::Client::new(), &paths, 21, false, &task, &cancel)
        .await
        .unwrap();
    assert_eq!(info.major, 21);
    assert!(info.path.exists());
    assert!(dir.path().join("java").is_dir());
}
