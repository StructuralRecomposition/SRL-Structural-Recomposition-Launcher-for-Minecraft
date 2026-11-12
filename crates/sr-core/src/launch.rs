use crate::error::{AppError, ErrorCode, Result};
use crate::instance::Instance;
use crate::java::{check_compatibility, JavaCompatibility, JavaInfo};
use crate::paths::GamePaths;
use crate::util;
use crate::version::{
    build_plan, classpath_separator, LaunchOptions, LaunchPlan, PlayerProfile, RuleContext,
    VersionJson, VersionService,
};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchEvent {
    pub instance_id: String,
    pub kind: String,
    pub stream: Option<String>,
    pub line: Option<String>,
    pub code: Option<i32>,
    pub message: Option<String>,
}

pub type LaunchSink = Arc<dyn Fn(LaunchEvent) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    pub instance_id: String,
    pub pid: u32,
    pub log_path: PathBuf,
    pub warnings: Vec<String>,
}

const LOG_SHARD_BYTES: u64 = 32 * 1024 * 1024;
const LOG_SHARDS: u32 = 5;

struct SessionLogWriter {
    file: Option<tokio::fs::File>,
    path: PathBuf,
    bytes: u64,
}

impl SessionLogWriter {
    async fn open(path: PathBuf) -> Result<Self> {
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        let bytes = file.metadata().await.map(|meta| meta.len()).unwrap_or(0);
        Ok(Self {
            file: Some(file),
            path,
            bytes,
        })
    }

    async fn write_line(&mut self, line: &str) -> Result<()> {
        if self.bytes >= LOG_SHARD_BYTES {
            self.rotate().await?;
        }
        if let Some(file) = self.file.as_mut() {
            file.write_all(line.as_bytes()).await?;
            file.flush().await?;
            self.bytes = self.bytes.saturating_add(line.len() as u64);
        }
        Ok(())
    }

    async fn rotate(&mut self) -> Result<()> {
        if let Some(mut file) = self.file.take() {
            let _ = file.flush().await;
            drop(file);
        }
        let _ = tokio::fs::remove_file(self.path.with_extension(format!("log.{LOG_SHARDS}"))).await;
        for index in (1..LOG_SHARDS).rev() {
            let from = self.path.with_extension(format!("log.{index}"));
            let to = self.path.with_extension(format!("log.{}", index + 1));
            if tokio::fs::try_exists(&from).await.unwrap_or(false) {
                let _ = tokio::fs::rename(&from, &to).await;
            }
        }
        let _ = tokio::fs::rename(&self.path, self.path.with_extension("log.1")).await;
        self.file = Some(
            tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .await?,
        );
        self.bytes = 0;
        Ok(())
    }
}

#[allow(dead_code)]
struct RunningProcess {
    child: Arc<Mutex<Child>>,
    pid: u32,
}

pub struct LaunchManager {
    game: GamePaths,
    versions: VersionService,
    logs_dir: PathBuf,
    running: Arc<RwLock<HashMap<String, RunningProcess>>>,
    sink: LaunchSink,
}

impl LaunchManager {
    pub fn new(
        game: GamePaths,
        versions: VersionService,
        logs_dir: PathBuf,
        sink: LaunchSink,
    ) -> Self {
        Self {
            game,
            versions,
            logs_dir,
            running: Arc::new(RwLock::new(HashMap::new())),
            sink,
        }
    }

    pub fn game(&self) -> &GamePaths {
        &self.game
    }

    /// Point future launches at a new game directory without disturbing
    /// processes that are already running.
    pub fn set_game(&mut self, game: GamePaths) {
        self.game = game;
    }

    pub async fn is_running(&self, instance_id: &str) -> bool {
        self.running.read().await.contains_key(instance_id)
    }

    pub async fn running_instances(&self) -> Vec<String> {
        self.running.read().await.keys().cloned().collect()
    }

    pub async fn pid(&self, instance_id: &str) -> Option<u32> {
        self.running.read().await.get(instance_id).map(|p| p.pid)
    }

    /// Stop a running instance. The first attempt asks the game to exit
    /// gracefully (SIGTERM on Unix, WM_CLOSE via taskkill on Windows). If it
    /// is still alive after the bounded wait, `force=false` returns an error
    /// so the UI can ask before discarding unsaved progress; `force=true`
    /// terminates only this launcher-owned process.
    pub async fn stop(&self, instance_id: &str, force: bool) -> Result<()> {
        let (pid, child) = {
            let map = self.running.read().await;
            let Some(proc) = map.get(instance_id) else {
                return Err(AppError::not_found("该实例当前未在运行"));
            };
            (proc.pid, proc.child.clone())
        };

        // If the child already exited, there is nothing to signal.
        {
            let mut c = child.lock().await;
            if c.try_wait().map_err(AppError::from)?.is_some() {
                return Ok(());
            }
        }

        #[cfg(unix)]
        {
            let rc = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            if rc != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    tracing::warn!(pid, error = %error, "SIGTERM failed");
                }
            }
        }
        #[cfg(windows)]
        {
            // taskkill without /F posts WM_CLOSE to GUI applications. It is
            // scoped to the single PID created by this launcher.
            let _ = tokio::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string()])
                .output()
                .await;
        }

        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let exited = {
                let mut c = child.lock().await;
                c.try_wait().map_err(AppError::from)?.is_some()
            };
            if exited {
                return Ok(());
            }
        }

        if !force {
            return Err(AppError::new(
                ErrorCode::InvalidState,
                "游戏未在 10 秒内退出。可以强制结束，但未保存的世界进度可能丢失。",
            ));
        }

        #[cfg(windows)]
        {
            let _ = tokio::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output()
                .await;
        }
        let mut c = child.lock().await;
        c.start_kill().map_err(|e| {
            AppError::new(ErrorCode::ProcessFailed, "无法强制终止游戏进程")
                .with_detail(e.to_string())
        })?;
        Ok(())
    }

    /// Launch an installed instance with a selected Java.
    pub async fn launch(
        &self,
        instance: &Instance,
        player: &PlayerProfile,
        java: &JavaInfo,
    ) -> Result<LaunchResult> {
        if self.is_running(&instance.id).await {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                "该实例已经在运行中",
            ));
        }
        let resolved = self
            .versions
            .load_installed(&self.game, &instance.version_id)
            .await?;
        resolved.validate_for_launch()?;

        let required = resolved.java_major();
        let check = check_compatibility(required, java.major);
        if check.compatibility == JavaCompatibility::Incompatible {
            return Err(AppError::java_incompatible(check.message));
        }
        let mut warnings = Vec::new();
        if check.compatibility == JavaCompatibility::Warning {
            warnings.push(check.message);
        }

        util::ensure_dir(&instance.game_dir).await?;
        let ctx = RuleContext::default();
        let libs = resolved.effective_libraries(&ctx);

        let mut classpath: Vec<PathBuf> = Vec::new();
        let mut missing: Vec<String> = Vec::new();
        let jar = self.game.version_jar(&resolved.jar_id());
        if !tokio::fs::try_exists(&jar).await.unwrap_or(false) {
            missing.push(format!("{}.jar", resolved.id));
        }
        classpath.push(jar);
        for lib in libs.iter().filter(|l| !l.is_native) {
            let path = util::safe_join(&self.game.libraries_dir(), &lib.path)?;
            if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
                missing.push(lib.path.clone());
            }
            classpath.push(path);
        }
        if !missing.is_empty() {
            return Err(AppError::new(
                ErrorCode::NotFound,
                format!("版本文件不完整，缺少 {} 个文件", missing.len()),
            )
            .with_detail(missing.into_iter().take(5).collect::<Vec<_>>().join(", ")));
        }

        let natives_dir = self.game.natives_dir(&resolved.jar_id());
        if libs.iter().any(|l| l.is_native) {
            let has_natives = match tokio::fs::read_dir(&natives_dir).await {
                Ok(mut rd) => rd.next_entry().await.ok().flatten().is_some(),
                Err(_) => false,
            };
            if !has_natives {
                return Err(AppError::new(
                    ErrorCode::NotFound,
                    "natives 尚未解压，请重新安装该版本",
                ));
            }
        }

        let log_config = resolved
            .logging()
            .and_then(|l| l.client.as_ref())
            .and_then(|c| c.file.as_ref())
            .map(|f| self.game.root().join("log_configs").join(&f.sha1))
            .filter(|p| p.exists());

        let options = LaunchOptions {
            game_dir: instance.game_dir.clone(),
            assets_dir: self.game.assets_dir(),
            libraries_dir: self.game.libraries_dir(),
            natives_dir: natives_dir.clone(),
            log_config,
            classpath,
            asset_index: resolved.asset_index()?.id.clone(),
            version_type: resolved
                .version_type
                .clone()
                .unwrap_or_else(|| "release".into()),
            player: player.clone(),
            memory_mb: instance.memory_mb,
            extra_jvm_args: instance.jvm_args.clone(),
            extra_game_args: instance.game_args.clone(),
            resolution: match (instance.resolution_width, instance.resolution_height) {
                (Some(w), Some(h)) => Some((w, h)),
                _ => None,
            },
            launcher_name: "SRLauncher".into(),
            launcher_version: env!("CARGO_PKG_VERSION").into(),
        };
        let plan = build_plan(&resolved, &ctx, &options)?;
        warnings.extend(plan.warnings.clone());

        let log_path = self.new_log_path(&instance.id);
        if let Some(parent) = log_path.parent() {
            util::ensure_dir(parent).await?;
        }
        let log_file = SessionLogWriter::open(log_path.clone()).await?;

        let (pid, child) = self
            .spawn_plan(
                &instance.id,
                &java.path,
                &plan,
                &instance.game_dir,
                log_file,
            )
            .await?;

        self.running
            .write()
            .await
            .insert(instance.id.clone(), RunningProcess { child, pid });
        self.spawn_waiter(instance.id.clone());

        tracing::info!(
            instance = %instance.id,
            version = %instance.version_id,
            java = %java.path.display(),
            pid,
            "game process started"
        );

        Ok(LaunchResult {
            instance_id: instance.id.clone(),
            pid,
            log_path,
            warnings,
        })
    }

    fn new_log_path(&self, instance_id: &str) -> PathBuf {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        self.logs_dir
            .join("instances")
            .join(format!("{instance_id}-{stamp}.log"))
    }

    async fn spawn_plan(
        &self,
        instance_id: &str,
        java: &Path,
        plan: &LaunchPlan,
        cwd: &Path,
        log_file: SessionLogWriter,
    ) -> Result<(u32, Arc<Mutex<Child>>)> {
        let mut cmd = tokio::process::Command::new(java);
        cmd.args(&plan.arguments)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false);
        #[cfg(windows)]
        {
            // Do not open a console window for the child JVM.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn().map_err(|e| {
            AppError::new(
                ErrorCode::ProcessFailed,
                format!("无法启动 Java 进程：{}", java.display()),
            )
            .with_detail(e.to_string())
        })?;
        let pid = child
            .id()
            .ok_or_else(|| AppError::new(ErrorCode::ProcessFailed, "无法获取游戏进程 ID"))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let log_file = Arc::new(Mutex::new(log_file));

        if let Some(out) = stdout {
            self.spawn_reader(instance_id.to_string(), "stdout", out, log_file.clone());
        }
        if let Some(err) = stderr {
            self.spawn_reader(instance_id.to_string(), "stderr", err, log_file.clone());
        }

        let child = Arc::new(Mutex::new(child));
        Ok((pid, child))
    }

    fn spawn_reader<R>(
        &self,
        instance_id: String,
        stream: &'static str,
        reader: R,
        log_file: Arc<Mutex<SessionLogWriter>>,
    ) where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let sink = self.sink.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let line = util::redact_secrets(&line);
                        {
                            let mut writer = log_file.lock().await;
                            if let Err(error) =
                                writer.write_line(&format!("[{stream}] {line}\n")).await
                            {
                                tracing::warn!(stream, error = %error, "game log write failed");
                            }
                        }
                        (sink)(LaunchEvent {
                            instance_id: instance_id.clone(),
                            kind: "log".into(),
                            stream: Some(stream.into()),
                            line: Some(line),
                            code: None,
                            message: None,
                        });
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!(stream, error = %e, "failed reading game output");
                        break;
                    }
                }
            }
        });
    }

    fn spawn_waiter(&self, instance_id: String) {
        let running = self.running.clone();
        let sink = self.sink.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                let status = {
                    let map = running.read().await;
                    let Some(proc) = map.get(&instance_id) else {
                        return;
                    };
                    let mut child = proc.child.lock().await;
                    match child.try_wait() {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!(error = %e, "try_wait failed");
                            None
                        }
                    }
                };
                if let Some(status) = status {
                    running.write().await.remove(&instance_id);
                    (sink)(LaunchEvent {
                        instance_id: instance_id.clone(),
                        kind: "exit".into(),
                        stream: None,
                        line: None,
                        code: status.code(),
                        message: Some(match status.code() {
                            Some(0) => "游戏已正常退出".into(),
                            Some(c) => format!("游戏进程退出，代码 {c}"),
                            None => "游戏进程被信号终止".into(),
                        }),
                    });
                    return;
                }
            }
        });
    }
}

/// Check the minimum local files needed before launching, without starting a
/// process. Used by the UI's pre-flight panel.
pub async fn preflight(
    game: &GamePaths,
    versions: &VersionService,
    resolved: &VersionJson,
    ctx: &RuleContext,
) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    if !tokio::fs::try_exists(&game.version_jar(&resolved.jar_id()))
        .await
        .unwrap_or(false)
    {
        problems.push(format!("缺少客户端 JAR：{}.jar", resolved.id));
    }
    for lib in resolved
        .effective_libraries(ctx)
        .iter()
        .filter(|l| !l.is_native)
    {
        let path = util::safe_join(&game.libraries_dir(), &lib.path)?;
        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            problems.push(format!("缺少库文件：{}", lib.path));
        }
    }
    let _ = versions;
    Ok(problems)
}

/// Describe how a classpath would be built (diagnostics only; never logs
/// account data).
pub fn classpath_preview(entries: &[PathBuf]) -> String {
    entries
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(classpath_separator())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn rotates_session_log_shards() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.log");
        let mut writer = SessionLogWriter::open(path.clone()).await.unwrap();
        writer.write_line("before\n").await.unwrap();
        writer.bytes = LOG_SHARD_BYTES;
        writer.write_line("after\n").await.unwrap();
        assert!(path.with_extension("log.1").exists());
        let current = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(current, "after\n");
    }

    #[tokio::test]
    async fn spawns_process_and_captures_output() {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        game.ensure_layout().await.unwrap();
        let events = Arc::new(std::sync::Mutex::new(Vec::<LaunchEvent>::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let (ev, c) = (events.clone(), count.clone());
        let sink: LaunchSink = Arc::new(move |e| {
            c.fetch_add(1, Ordering::Relaxed);
            ev.lock().unwrap().push(e);
        });
        let mgr = LaunchManager::new(
            game.clone(),
            VersionService::new(reqwest::Client::new()),
            dir.path().join("logs"),
            sink,
        );
        let log_path = dir.path().join("test.log");
        let log = SessionLogWriter::open(log_path.clone()).await.unwrap();
        let plan = LaunchPlan {
            arguments: vec!["hello-sr".into()],
            warnings: vec![],
            classpath_string: String::new(),
        };
        let java = if cfg!(windows) { "cmd" } else { "/bin/echo" };
        let (pid, child) = mgr
            .spawn_plan("inst", Path::new(java), &plan, dir.path(), log)
            .await
            .unwrap();
        assert!(pid > 0);

        mgr.running
            .write()
            .await
            .insert("inst".into(), RunningProcess { child, pid });
        mgr.spawn_waiter("inst".into());
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if mgr.running_instances().await.is_empty() {
                break;
            }
        }
        assert!(
            mgr.running_instances().await.is_empty(),
            "process should exit"
        );
        {
            let evs = events.lock().unwrap();
            assert!(
                evs.iter()
                    .any(|e| e.kind == "log" && e.line.as_deref() == Some("hello-sr")),
                "{evs:?}"
            );
            assert!(evs.iter().any(|e| e.kind == "exit" && e.code == Some(0)));
        }
        let text = tokio::fs::read_to_string(&log_path).await.unwrap();
        assert!(text.contains("hello-sr"));
    }

    #[test]
    fn redacts_token_in_output() {
        let line = "Authorization: Bearer abc.def.ghi";
        let r = util::redact_secrets(line);
        assert!(!r.contains("abc.def.ghi"));
    }

    #[tokio::test]
    async fn preflight_reports_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        game.ensure_layout().await.unwrap();
        let v: VersionJson = serde_json::from_value(serde_json::json!({
            "id":"x","mainClass":"M","assetIndex":{"id":"1","url":"https://x"},
            "downloads":{"client":{"sha1":"a","size":1,"url":"https://x/c.jar"}},
            "libraries":[{"name":"g:a:1","downloads":{"artifact":{"path":"g/a/1/a-1.jar","url":"https://x/a.jar","sha1":"b","size":1}}}],
            "arguments":{"game":[],"jvm":[]}
        }))
        .unwrap();
        let problems = preflight(
            &game,
            &VersionService::new(reqwest::Client::new()),
            &v,
            &RuleContext::default(),
        )
        .await
        .unwrap();
        assert_eq!(problems.len(), 2);
        assert!(problems.iter().any(|p| p.contains("JAR")));
        assert!(problems.iter().any(|p| p.contains("库文件")));
    }
}
