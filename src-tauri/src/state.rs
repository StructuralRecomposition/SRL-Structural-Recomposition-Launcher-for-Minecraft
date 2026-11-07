use sr_core::account::AccountStore;
use sr_core::download::DownloadManager;
use sr_core::java::JavaInfo;
use sr_core::launch::{LaunchManager, LaunchSink};
use sr_core::modrinth::ModrinthService;
use sr_core::paths::{AppPaths, GamePaths};
use sr_core::settings::Settings;
use sr_core::skin::SkinLibrary;
use sr_core::version::VersionService;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub paths: AppPaths,
    pub settings: Arc<RwLock<Settings>>,
    pub game: Arc<RwLock<GamePaths>>,
    pub versions: VersionService,
    pub downloads: DownloadManager,
    pub accounts: AccountStore,
    pub skins: SkinLibrary,
    pub modrinth: ModrinthService,
    pub java: Arc<RwLock<Vec<JavaInfo>>>,
    pub launches: Arc<RwLock<LaunchManager>>,
    pub http: reqwest::Client,
    /// Prevents two concurrent installs of the same version writing the same
    /// files. Cleared when the install task ends.
    pub installing: Arc<Mutex<HashSet<String>>>,
}

impl AppState {
    pub async fn game_snapshot(&self) -> GamePaths {
        self.game.read().await.clone()
    }

    pub async fn settings_snapshot(&self) -> Settings {
        self.settings.read().await.clone()
    }

    pub async fn rebuild_launch_manager(&self) {
        let game = self.game_snapshot().await;
        // Preserve the running-process registry; only future launches move to
        // the new game directory.
        self.launches.write().await.set_game(game);
    }
}

pub fn task_sink(app: AppHandle) -> sr_core::download::TaskSink {
    Arc::new(move |view| {
        let _ = app.emit("task://update", view);
    })
}

pub fn launch_sink(app: AppHandle) -> LaunchSink {
    Arc::new(move |event| {
        let _ = app.emit("launch://event", event);
    })
}
