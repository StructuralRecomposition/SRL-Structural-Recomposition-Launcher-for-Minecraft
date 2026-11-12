use crate::error::{AppError, ErrorCode, Result};
use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, OwnedMutexGuard, OwnedSemaphorePermit, RwLock, Semaphore};
use tokio_util::sync::CancellationToken;

/// Official Mojang download hosts. Anything else is rejected unless the user
/// explicitly opts into custom hosts (not implemented in phase 1).
pub const TRUSTED_HOSTS: &[&str] = &[
    "piston-meta.mojang.com",
    "launchermeta.mojang.com",
    "piston-data.mojang.com",
    "libraries.minecraft.net",
    "resources.download.minecraft.net",
    "launcher.mojang.com",
    "meta.fabricmc.net",
    "maven.fabricmc.net",
    "meta.quiltmc.org",
    "maven.quiltmc.org",
    "maven.minecraftforge.net",
    "maven.neoforged.net",
    "login.microsoftonline.com",
    "user.auth.xboxlive.com",
    "xsts.auth.xboxlive.com",
    "api.minecraftservices.com",
    "api.modrinth.com",
    "cdn.modrinth.com",
    "cdn-raw.modrinth.com",
    "github.com",
    "objects.githubusercontent.com",
    "raw.githubusercontent.com",
    "media.forgecdn.net",
    "edge.forgecdn.net",
    "mediafilez.forgecdn.net",
    "bmclapi2.bangbang93.com",
    "mirrors.tuna.tsinghua.edu.cn",
    "cmcc.mirrors.ustc.edu.cn",
    "mirrors.cernet.edu.cn",
];

pub const MAX_RETRIES: u32 = 3;

/// A response stream that produces no bytes for this long is treated as a
/// stalled connection instead of relying on the client-wide timeout.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

pub fn ensure_trusted_url(raw: &str, allow_custom_hosts: bool) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw)
        .map_err(|e| AppError::invalid_input("下载地址格式无效").with_detail(e.to_string()))?;
    if url.scheme() != "https" {
        return Err(AppError::path_unsafe(format!(
            "拒绝非 HTTPS 下载地址：{}",
            url.scheme()
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::path_unsafe("下载地址不得包含凭据"));
    }
    if allow_custom_hosts {
        return Ok(url);
    }
    let host = url
        .host_str()
        .ok_or_else(|| AppError::path_unsafe("下载地址缺少主机名"))?;
    if !TRUSTED_HOSTS.contains(&host) {
        return Err(AppError::path_unsafe(format!(
            "下载地址不在可信域名列表中：{host}"
        )));
    }
    Ok(url)
}

/// Test-only URL validator that permits loopback HTTP.
#[doc(hidden)]
pub fn ensure_test_url(raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw)
        .map_err(|e| AppError::invalid_input("下载地址格式无效").with_detail(e.to_string()))?;
    let host_ok = matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"));
    if !(url.scheme() == "http" && host_ok) {
        return Err(AppError::path_unsafe("测试下载地址必须是 loopback HTTP"));
    }
    Ok(url)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Queued,
    Running,
    Verifying,
    Completed,
    Cancelled,
    Retrying,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub status: TaskStatus,
    pub total_bytes: u64,
    pub done_bytes: u64,
    pub total_files: u32,
    pub done_files: u32,
    pub current_file: Option<String>,
    pub error: Option<AppError>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub speed_bps: u64,
    pub eta_seconds: Option<u64>,
}

pub type TaskSink = Arc<dyn Fn(TaskView) + Send + Sync>;

struct TaskInner {
    id: String,
    kind: String,
    title: String,
    status: TaskStatus,
    total_bytes: u64,
    done_bytes: u64,
    total_files: u32,
    done_files: u32,
    current_file: Option<String>,
    error: Option<AppError>,
    started_at: String,
    finished_at: Option<String>,
    cancel: CancellationToken,
    sample_at: Instant,
    sample_bytes: u64,
    speed_bps: u64,
}

impl TaskInner {
    fn view(&self) -> TaskView {
        let eta = if self.speed_bps > 0 && self.total_bytes > self.done_bytes {
            Some((self.total_bytes - self.done_bytes) / self.speed_bps)
        } else {
            None
        };
        TaskView {
            id: self.id.clone(),
            kind: self.kind.clone(),
            title: self.title.clone(),
            status: self.status.clone(),
            total_bytes: self.total_bytes,
            done_bytes: self.done_bytes,
            total_files: self.total_files,
            done_files: self.done_files,
            current_file: self.current_file.clone(),
            error: self.error.clone(),
            started_at: self.started_at.clone(),
            finished_at: self.finished_at.clone(),
            speed_bps: self.speed_bps,
            eta_seconds: eta,
        }
    }
}

#[derive(Clone)]
pub struct TaskHandle {
    pub id: String,
    inner: Arc<Mutex<TaskInner>>,
    sink: TaskSink,
    cancel: CancellationToken,
}

impl TaskHandle {
    fn from_view(view: TaskView, sink: TaskSink) -> Self {
        let interrupted = matches!(
            &view.status,
            TaskStatus::Queued | TaskStatus::Running | TaskStatus::Verifying | TaskStatus::Retrying
        );
        let (status, error) = if interrupted {
            (
                TaskStatus::Failed,
                Some(AppError::new(
                    crate::error::ErrorCode::Interrupted,
                    "上次应用退出时任务尚未完成",
                )),
            )
        } else {
            (view.status, view.error)
        };
        let inner = TaskInner {
            id: view.id.clone(),
            kind: view.kind,
            title: view.title,
            status,
            total_bytes: view.total_bytes,
            // Older builds could persist over-counted progress; clamp on load.
            done_bytes: if view.total_bytes > 0 {
                view.done_bytes.min(view.total_bytes)
            } else {
                view.done_bytes
            },
            total_files: view.total_files,
            done_files: view.done_files.min(view.total_files),
            current_file: None,
            error,
            started_at: view.started_at,
            finished_at: view.finished_at.or_else(|| Some(util::now_rfc3339())),
            cancel: CancellationToken::new(),
            sample_at: Instant::now(),
            sample_bytes: 0,
            speed_bps: 0,
        };
        Self {
            id: view.id,
            inner: Arc::new(Mutex::new(inner)),
            sink,
            cancel: CancellationToken::new(),
        }
    }

    async fn emit(&self) {
        let view = self.inner.lock().await.view();
        (self.sink)(view);
    }

    pub async fn set_total_bytes(&self, total: u64) {
        self.inner.lock().await.total_bytes = total;
        self.emit().await;
    }

    pub async fn set_total_files(&self, total: u32) {
        self.inner.lock().await.total_files = total;
        self.emit().await;
    }

    pub async fn set_current_file(&self, name: &str) {
        {
            let mut inner = self.inner.lock().await;
            inner.current_file = Some(name.to_string());
        }
        self.emit().await;
    }

    pub async fn set_done_bytes(&self, bytes: u64) {
        {
            let mut inner = self.inner.lock().await;
            inner.done_bytes = bytes;
            let elapsed = inner.sample_at.elapsed();
            if elapsed >= Duration::from_millis(400) {
                let delta = inner.done_bytes.saturating_sub(inner.sample_bytes);
                inner.speed_bps = (delta as f64 / elapsed.as_secs_f64()) as u64;
                inner.sample_at = Instant::now();
                inner.sample_bytes = inner.done_bytes;
            }
        }
        self.emit().await;
    }

    pub async fn add_bytes(&self, bytes: u64) {
        {
            let mut inner = self.inner.lock().await;
            inner.done_bytes = inner.done_bytes.saturating_add(bytes);
            let elapsed = inner.sample_at.elapsed();
            if elapsed >= Duration::from_millis(400) {
                let delta = inner.done_bytes.saturating_sub(inner.sample_bytes);
                inner.speed_bps = (delta as f64 / elapsed.as_secs_f64()) as u64;
                inner.sample_at = Instant::now();
                inner.sample_bytes = inner.done_bytes;
            }
        }
        self.emit().await;
    }

    pub async fn file_finished(&self) {
        {
            let mut inner = self.inner.lock().await;
            inner.done_files = inner.done_files.saturating_add(1);
        }
        self.emit().await;
    }

    pub async fn set_status(&self, status: TaskStatus) {
        {
            let mut inner = self.inner.lock().await;
            inner.status = status.clone();
            if matches!(status, TaskStatus::Verifying) {
                inner.current_file = Some("校验文件…".into());
            }
        }
        self.emit().await;
    }

    pub async fn retrying(&self, attempt: u32, reason: &str) {
        {
            let mut inner = self.inner.lock().await;
            inner.status = TaskStatus::Retrying;
            inner.error = Some(AppError::network(format!(
                "下载失败，正在第 {attempt} 次重试：{reason}"
            )));
        }
        self.emit().await;
    }

    pub async fn complete(&self) {
        {
            let mut inner = self.inner.lock().await;
            inner.status = TaskStatus::Completed;
            inner.finished_at = Some(util::now_rfc3339());
            inner.speed_bps = 0;
            inner.error = None;
            if inner.total_bytes > 0 {
                inner.done_bytes = inner.total_bytes;
            }
        }
        self.emit().await;
    }

    pub async fn fail(&self, error: AppError) {
        {
            let mut inner = self.inner.lock().await;
            inner.status = TaskStatus::Failed;
            inner.finished_at = Some(util::now_rfc3339());
            inner.speed_bps = 0;
            inner.error = Some(error);
        }
        self.emit().await;
    }

    pub async fn cancelled(&self) {
        {
            let mut inner = self.inner.lock().await;
            inner.status = TaskStatus::Cancelled;
            inner.finished_at = Some(util::now_rfc3339());
            inner.speed_bps = 0;
        }
        self.emit().await;
    }

    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}

#[derive(Clone)]
pub struct DownloadManager {
    tasks: Arc<RwLock<HashMap<String, TaskHandle>>>,
    sink: TaskSink,
    store: Option<PathBuf>,
    permits: Arc<std::sync::RwLock<Arc<Semaphore>>>,
    limit: Arc<AtomicU64>,
    target_locks: Arc<std::sync::Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>>,
}

/// Guard for a per-destination download lock. Dropping the last user removes
/// the map entry so installing many versions does not grow the manager's
/// memory forever.
pub struct TargetGuard {
    guard: Option<OwnedMutexGuard<()>>,
    locks: Arc<std::sync::Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>>,
    dest: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl Drop for TargetGuard {
    fn drop(&mut self) {
        self.guard.take();
        if let Ok(mut map) = self.locks.lock() {
            // map + self.lock are the only remaining owners when nobody is
            // waiting; a waiter would still hold an Arc through its guard.
            if Arc::strong_count(&self.lock) <= 2
                && map
                    .get(&self.dest)
                    .is_some_and(|value| Arc::ptr_eq(value, &self.lock))
            {
                map.remove(&self.dest);
            }
        }
    }
}

impl DownloadManager {
    pub fn new(sink: TaskSink) -> Self {
        Self::build(sink, None, 6)
    }

    /// Create a manager that restores task history from `store` and writes
    /// snapshots on demand. Active tasks from a previous run are marked as
    /// interrupted instead of being silently replayed.
    pub fn with_store(sink: TaskSink, store: PathBuf, concurrency: u32) -> Self {
        Self::build(sink, Some(store), concurrency)
    }

    fn build(sink: TaskSink, store: Option<PathBuf>, concurrency: u32) -> Self {
        let concurrency = concurrency.clamp(1, 16);
        let mut map = HashMap::new();
        if let Some(path) = &store {
            for view in load_stored_tasks(path) {
                let handle = TaskHandle::from_view(view, sink.clone());
                map.insert(handle.id.clone(), handle);
            }
        }
        Self {
            tasks: Arc::new(RwLock::new(map)),
            sink,
            store,
            permits: Arc::new(std::sync::RwLock::new(Arc::new(Semaphore::new(
                concurrency as usize,
            )))),
            limit: Arc::new(AtomicU64::new(concurrency as u64)),
            target_locks: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Serialize downloads that target the same path so the deterministic
    /// `.srpart` file cannot be written by two tasks at once.
    pub async fn lock_target(&self, dest: &Path) -> TargetGuard {
        let lock = {
            let mut map = self
                .target_locks
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            map.entry(dest.to_path_buf())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let guard = lock.clone().lock_owned().await;
        TargetGuard {
            guard: Some(guard),
            locks: self.target_locks.clone(),
            dest: dest.to_path_buf(),
            lock,
        }
    }

    /// Acquire a permit from the global download limiter. The permit is
    /// released when dropped.
    pub async fn acquire(&self) -> OwnedSemaphorePermit {
        let sem = self.permits.read().unwrap().clone();
        sem.acquire_owned()
            .await
            .expect("download semaphore is never closed")
    }

    pub fn set_limit(&self, concurrency: u32) {
        let n = concurrency.clamp(1, 16);
        *self.permits.write().unwrap() = Arc::new(Semaphore::new(n as usize));
        self.limit.store(n as u64, Ordering::Relaxed);
    }

    pub fn limit(&self) -> u32 {
        self.limit.load(Ordering::Relaxed) as u32
    }

    /// Persist the current task list. No-op when constructed without a store.
    pub async fn save(&self) -> Result<()> {
        if let Some(path) = &self.store {
            let views = self.list().await;
            util::write_json(path, &views).await?;
        }
        Ok(())
    }

    pub async fn create(
        &self,
        kind: &str,
        title: &str,
        total_files: u32,
        total_bytes: u64,
    ) -> TaskHandle {
        let id = uuid::Uuid::new_v4().to_string();
        let inner = TaskInner {
            id: id.clone(),
            kind: kind.into(),
            title: title.into(),
            status: TaskStatus::Queued,
            total_bytes,
            done_bytes: 0,
            total_files,
            done_files: 0,
            current_file: None,
            error: None,
            started_at: util::now_rfc3339(),
            finished_at: None,
            cancel: CancellationToken::new(),
            sample_at: Instant::now(),
            sample_bytes: 0,
            speed_bps: 0,
        };
        let handle = TaskHandle {
            id: id.clone(),
            inner: Arc::new(Mutex::new(inner)),
            sink: self.sink.clone(),
            cancel: CancellationToken::new(),
        };
        {
            let mut tasks = self.tasks.write().await;
            tasks.insert(id, handle.clone());
            if tasks.len() > 50 {
                let mut finished: Vec<(String, String)> = {
                    let mut rows = Vec::new();
                    for (tid, h) in tasks.iter() {
                        if let Ok(inner) = h.inner.try_lock() {
                            if matches!(
                                inner.status,
                                TaskStatus::Completed | TaskStatus::Cancelled | TaskStatus::Failed
                            ) {
                                rows.push((tid.clone(), inner.started_at.clone()));
                            }
                        }
                    }
                    rows
                };
                finished.sort_by(|a, b| a.1.cmp(&b.1));
                while tasks.len() > 50 {
                    if let Some((oldest, _)) = finished.first().cloned() {
                        tasks.remove(&oldest);
                        finished.remove(0);
                    } else {
                        break;
                    }
                }
            }
        }
        handle.set_status(TaskStatus::Running).await;
        handle
    }

    pub async fn get(&self, id: &str) -> Option<TaskHandle> {
        self.tasks.read().await.get(id).cloned()
    }

    pub async fn cancel(&self, id: &str) -> Result<()> {
        let handle = self
            .get(id)
            .await
            .ok_or_else(|| AppError::not_found("任务不存在"))?;
        {
            let inner = handle.inner.lock().await;
            inner.cancel.cancel();
        }
        handle.cancel.cancel();
        Ok(())
    }

    pub async fn cancel_all(&self) {
        let tasks = self.tasks.read().await;
        for handle in tasks.values() {
            // `try_lock` can miss a task that is emitting progress at exit;
            // shutdown must not leave a download running.
            let inner = handle.inner.lock().await;
            inner.cancel.cancel();
            handle.cancel.cancel();
        }
    }

    pub async fn list(&self) -> Vec<TaskView> {
        let tasks = self.tasks.read().await;
        let mut out = Vec::new();
        for handle in tasks.values() {
            out.push(handle.inner.lock().await.view());
        }
        out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        out
    }

    /// Remove finished tasks from the in-memory list.
    pub async fn clear_finished(&self) {
        let mut tasks = self.tasks.write().await;
        let finished: Vec<String> = {
            let mut ids = Vec::new();
            for (id, handle) in tasks.iter() {
                let status = handle.inner.lock().await.status.clone();
                if matches!(
                    status,
                    TaskStatus::Completed | TaskStatus::Cancelled | TaskStatus::Failed
                ) {
                    ids.push(id.clone());
                }
            }
            ids
        };
        for id in finished {
            tasks.remove(&id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadOutcome {
    Skipped,
    Downloaded(u64),
}

/// Description of a single file download.
#[derive(Debug, Clone, Copy)]
pub struct DownloadSpec<'a> {
    pub url: &'a str,
    pub dest: &'a Path,
    pub sha1: Option<&'a str>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PartialMeta {
    url: String,
    etag: Option<String>,
    last_modified: Option<String>,
    total: Option<u64>,
}

fn partial_meta_path(tmp: &Path) -> PathBuf {
    PathBuf::from(format!("{}.meta", tmp.display()))
}

async fn read_partial_meta(path: &Path) -> Option<PartialMeta> {
    let bytes = tokio::fs::read(path).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

async fn write_partial_meta(path: &Path, meta: &PartialMeta) -> Result<()> {
    let bytes = serde_json::to_vec(meta)?;
    util::atomic_write(path, &bytes).await
}

struct ResumeState {
    offset: u64,
    validator: String,
}

/// Decide whether an existing `.srpart` can be resumed. Requires both a
/// matching URL and a strong validator; otherwise the download starts over.
async fn resume_state(
    tmp: &Path,
    meta_path: &Path,
    url: &str,
    size: Option<u64>,
) -> Option<ResumeState> {
    let meta = read_partial_meta(meta_path).await?;
    if meta.url != url {
        return None;
    }
    let offset = tokio::fs::metadata(tmp).await.ok()?.len();
    if offset == 0 || size.is_some_and(|s| offset >= s) {
        return None;
    }
    let validator = meta.etag.clone().or_else(|| meta.last_modified.clone())?;
    Some(ResumeState { offset, validator })
}

async fn cleanup_partial(tmp: &Path) {
    let _ = tokio::fs::remove_file(tmp).await;
    let _ = tokio::fs::remove_file(partial_meta_path(tmp)).await;
}

async fn replace_file(tmp: &Path, dest: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        // MoveFileExW replaces the destination in one operation. Deleting the
        // old file first loses it when the rename fails (antivirus lock,
        // permission change), so never do that on Windows.
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let from: Vec<u16> = tmp
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let to: Vec<u16> = dest
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let ok = unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if ok == 0 {
            let error = std::io::Error::last_os_error();
            let _ = tokio::fs::remove_file(tmp).await;
            return Err(AppError::from(error));
        }
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        tokio::fs::rename(tmp, dest).await.map_err(AppError::from)
    }
}

fn header_string(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Parse `bytes <start>-<end>/<total>`.
fn parse_content_range(value: &str) -> Option<(u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let (start, _end) = range.split_once('-')?;
    let start = start.trim().parse::<u64>().ok()?;
    let total = if total.trim() == "*" {
        None
    } else {
        Some(total.trim().parse::<u64>().ok()?)
    };
    Some((start, total))
}

/// Download one file with retries, safe resume, optional SHA-1 verification
/// and atomic rename.
///
/// `progress` receives the **absolute** number of bytes written for this file
/// (including a resumed prefix), not an increment.
pub async fn download_file<F>(
    http: &reqwest::Client,
    spec: &DownloadSpec<'_>,
    cancel: &CancellationToken,
    allow_custom_hosts: bool,
    mut progress: F,
) -> Result<DownloadOutcome>
where
    F: FnMut(u64),
{
    let DownloadSpec {
        url,
        dest,
        sha1: expected_sha1,
        size: size_hint,
    } = *spec;
    if cancel.is_cancelled() {
        return Err(AppError::cancelled());
    }
    let parsed = match ensure_trusted_url(url, allow_custom_hosts) {
        Ok(u) => u,
        Err(e) => {
            #[cfg(test)]
            {
                if let Ok(u) = ensure_test_url(url) {
                    u
                } else {
                    return Err(e);
                }
            }
            #[cfg(not(test))]
            {
                return Err(e);
            }
        }
    };

    if tokio::fs::try_exists(dest).await.unwrap_or(false) {
        if let Some(expected) = expected_sha1 {
            let actual = util::sha1_file(dest).await.unwrap_or_default();
            if actual.eq_ignore_ascii_case(expected) {
                return Ok(DownloadOutcome::Skipped);
            }
        } else if let Some(size) = size_hint {
            if let Ok(meta) = tokio::fs::metadata(dest).await {
                if meta.len() == size {
                    return Ok(DownloadOutcome::Skipped);
                }
            }
        } else {
            return Ok(DownloadOutcome::Skipped);
        }
    }

    if let Some(parent) = dest.parent() {
        util::ensure_dir(parent).await?;
    }
    let file_name = dest
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".into());
    // Deterministic name so an interrupted download can be resumed after a
    // restart. Callers must hold `DownloadManager::lock_target` for this path.
    let tmp = dest.with_file_name(format!(".{file_name}.srpart"));
    let meta_path = partial_meta_path(&tmp);

    let mut last_error: Option<AppError> = None;
    for attempt in 0..=MAX_RETRIES {
        if cancel.is_cancelled() {
            return Err(AppError::cancelled());
        }
        let resume = resume_state(&tmp, &meta_path, url, size_hint).await;
        match try_download_once(
            http,
            parsed.clone(),
            &tmp,
            &meta_path,
            cancel,
            resume,
            &mut progress,
        )
        .await
        {
            Ok(written) => {
                if cancel.is_cancelled() {
                    return Err(AppError::cancelled());
                }
                if let Some(expected) = expected_sha1 {
                    let actual = util::sha1_file(&tmp).await?;
                    if !actual.eq_ignore_ascii_case(expected) {
                        cleanup_partial(&tmp).await;
                        progress(0);
                        last_error = Some(AppError::checksum(expected, &actual));
                        continue_after_backoff(attempt, cancel).await?;
                        continue;
                    }
                }
                if let Some(size) = size_hint {
                    if size > 0 {
                        let meta = tokio::fs::metadata(&tmp).await?;
                        if meta.len() != size {
                            cleanup_partial(&tmp).await;
                            progress(0);
                            last_error = Some(
                                AppError::new(
                                    ErrorCode::ChecksumMismatch,
                                    "下载文件大小与元数据不一致",
                                )
                                .with_detail(format!("期望 {size} 字节，实际 {}", meta.len())),
                            );
                            continue_after_backoff(attempt, cancel).await?;
                            continue;
                        }
                    }
                }
                replace_file(&tmp, dest).await?;
                let _ = tokio::fs::remove_file(&meta_path).await;
                return Ok(DownloadOutcome::Downloaded(written));
            }
            Err(e) if e.code == ErrorCode::Cancelled => {
                // Keep the partial file for a later resume.
                return Err(e);
            }
            Err(e) => {
                tracing::warn!(url = %parsed, attempt, error = %e, "download attempt failed");
                let retryable = e.retryable;
                last_error = Some(e);
                if !retryable || attempt == MAX_RETRIES {
                    break;
                }
                continue_after_backoff(attempt, cancel).await?;
            }
        }
    }
    Err(last_error.unwrap_or_else(|| AppError::network("下载失败")))
}

async fn continue_after_backoff(attempt: u32, cancel: &CancellationToken) -> Result<()> {
    let base = 400u64.saturating_mul(1u64 << attempt.min(5));
    let jitter = (attempt as u64 * 137) % 250;
    let delay = Duration::from_millis((base + jitter).min(6_000));
    tokio::select! {
        _ = tokio::time::sleep(delay) => Ok(()),
        _ = cancel.cancelled() => Err(AppError::cancelled()),
    }
}

async fn try_download_once<F>(
    http: &reqwest::Client,
    url: reqwest::Url,
    tmp: &Path,
    meta_path: &Path,
    cancel: &CancellationToken,
    resume: Option<ResumeState>,
    progress: &mut F,
) -> Result<u64>
where
    F: FnMut(u64),
{
    use futures_util::StreamExt;

    let mut request = http
        .get(url.clone())
        .header(reqwest::header::ACCEPT_ENCODING, "identity");
    if let Some(state) = &resume {
        request = request
            .header(reqwest::header::RANGE, format!("bytes={}-", state.offset))
            .header(reqwest::header::IF_RANGE, &state.validator);
    }
    let response = tokio::select! {
        result = request.send() => result?,
        _ = cancel.cancelled() => return Err(AppError::cancelled()),
    };
    let status = response.status();

    if status.as_u16() == 416 {
        // Our partial file is ahead of the server; restart from scratch.
        cleanup_partial(tmp).await;
        progress(0);
        return Err(
            AppError::new(ErrorCode::Http, "服务器拒绝续传范围，已重新开始").retryable(true),
        );
    }
    if !status.is_success() {
        let retryable = status.as_u16() == 429 || status.is_server_error();
        let error = AppError::new(
            ErrorCode::Http,
            format!("服务器返回 HTTP {}", status.as_u16()),
        )
        .retryable(retryable);
        if status.as_u16() == 429 {
            if let Some(secs) = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
            {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(secs.min(30))) => {}
                    _ = cancel.cancelled() => return Err(AppError::cancelled()),
                }
            }
        }
        return Err(error);
    }

    let mut offset = 0u64;
    let mut total: Option<u64> = None;
    if status.as_u16() == 206 {
        let Some(state) = &resume else {
            cleanup_partial(tmp).await;
            return Err(
                AppError::new(ErrorCode::Http, "服务器返回了意外的 206 响应").retryable(true),
            );
        };
        let content_range = header_string(response.headers(), reqwest::header::CONTENT_RANGE);
        let Some((start, range_total)) = content_range.as_deref().and_then(parse_content_range)
        else {
            cleanup_partial(tmp).await;
            return Err(
                AppError::new(ErrorCode::Http, "续传响应缺少有效的 Content-Range").retryable(true),
            );
        };
        if start != state.offset {
            cleanup_partial(tmp).await;
            return Err(
                AppError::new(ErrorCode::Http, "续传响应起点与本地文件不一致").retryable(true),
            );
        }
        offset = state.offset;
        total = range_total;
        progress(offset);
    } else {
        // Full response: truncate any partial data.
        progress(0);
        if let Some(len) = response.content_length() {
            total = Some(len);
        }
    }

    let etag = header_string(response.headers(), reqwest::header::ETAG);
    let last_modified = header_string(response.headers(), reqwest::header::LAST_MODIFIED);
    let _ = write_partial_meta(
        meta_path,
        &PartialMeta {
            url: url.to_string(),
            etag,
            last_modified,
            total,
        },
    )
    .await;

    let mut file = if offset > 0 {
        tokio::fs::OpenOptions::new().append(true).open(tmp).await?
    } else {
        tokio::fs::File::create(tmp).await?
    };

    let mut written = offset;
    let mut stream = response.bytes_stream();
    loop {
        tokio::select! {
            chunk = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next()) => {
                match chunk {
                    Err(_) => {
                        file.flush().await.ok();
                        return Err(AppError::network(
                            "下载连接超过 60 秒没有收到数据，已中断并等待重试",
                        )
                        .retryable(true));
                    }
                    Ok(Some(Ok(bytes))) => {
                        file.write_all(&bytes).await?;
                        written += bytes.len() as u64;
                        progress(written);
                    }
                    Ok(Some(Err(e))) => return Err(AppError::from(e)),
                    Ok(None) => break,
                }
            }
            _ = cancel.cancelled() => {
                file.flush().await.ok();
                return Err(AppError::cancelled());
            }
        }
    }
    file.flush().await?;
    file.sync_all().await?;
    Ok(written)
}

/// Aggregate progress across parallel downloads (for installs).
#[derive(Clone, Default)]
pub struct AtomicProgress(pub Arc<AtomicU64>);

impl AtomicProgress {
    pub fn add(&self, n: u64) -> u64 {
        self.0.fetch_add(n, Ordering::Relaxed) + n
    }
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Ensure a downloaded file matches its hash (used by verification code).
pub async fn verify_sha1(path: &Path, expected: &str) -> Result<bool> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(false);
    }
    Ok(util::sha1_file(path).await?.eq_ignore_ascii_case(expected))
}

pub fn sanitized_target(base: &Path, relative: &str) -> Result<PathBuf> {
    util::safe_join(base, relative)
}

fn load_stored_tasks(path: &Path) -> Vec<TaskView> {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Vec<TaskView>>(&bytes) {
            Ok(mut tasks) => {
                tasks.truncate(50);
                tasks
            }
            Err(e) => {
                tracing::warn!(error = %e, "ignoring corrupt task history");
                Vec::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            tracing::warn!(error = %e, "failed to read task history");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn url_policy_rejects_untrusted_and_http() {
        assert!(ensure_trusted_url("https://piston-meta.mojang.com/x.json", false).is_ok());
        assert!(ensure_trusted_url("http://piston-meta.mojang.com/x.json", false).is_err());
        assert!(ensure_trusted_url("https://evil.example/x.jar", false).is_err());
        assert!(ensure_trusted_url("https://evil.example/x.jar", true).is_ok());
        assert!(ensure_trusted_url("https://user:pass@piston-meta.mojang.com/x", false).is_err());
    }

    #[tokio::test]
    async fn cancel_interrupts_a_pending_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // Accept the connection but never send a response header.
        let server = tokio::spawn(async move {
            let mut sockets = Vec::new();
            for _ in 0..4 {
                let Ok((socket, _)) = listener.accept().await else {
                    break;
                };
                sockets.push(socket);
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
            drop(sockets);
        });

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("pending.bin");
        let cancel = CancellationToken::new();
        let url = format!("http://{addr}/file");
        let started = Instant::now();
        let task_cancel = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            task_cancel.cancel();
        });
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            download_file(
                &reqwest::Client::new(),
                &DownloadSpec {
                    url: &url,
                    dest: &dest,
                    sha1: None,
                    size: None,
                },
                &cancel,
                false,
                |_| {},
            ),
        )
        .await
        .expect("cancel must interrupt a request waiting for headers");
        assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(3));
        server.abort();
    }

    #[tokio::test]
    async fn target_lock_entries_are_removed_after_use() {
        let sink: TaskSink = Arc::new(|_| {});
        let manager = DownloadManager::new(sink);
        let dest = PathBuf::from("/tmp/srl-target-lock-test");
        {
            let _guard = manager.lock_target(&dest).await;
            assert_eq!(manager.target_locks.lock().unwrap().len(), 1);
        }
        assert!(manager.target_locks.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn downloads_and_verifies() {
        // Minimal HTTP server: serves a fixed body once.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = b"hello sr launcher".to_vec();
        let sha = util::sha1_bytes(&body);
        let body_for_server = body.clone();
        tokio::spawn(async move {
            for _ in 0..4 {
                let (mut socket, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => break,
                };
                let body_inner = body_for_server.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 1024];
                    let _ = socket.read(&mut buf).await;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body_inner.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.write_all(&body_inner).await;
                });
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("sub/file.bin");
        let client = reqwest::Client::new();
        let cancel = CancellationToken::new();
        let mut seen = 0u64;
        let out = download_file(
            &client,
            &DownloadSpec {
                url: &format!("http://{addr}/file.bin"),
                dest: &dest,
                sha1: Some(&sha),
                size: Some(body.len() as u64),
            },
            &cancel,
            false,
            |n| seen += n,
        )
        .await
        .unwrap();
        assert_eq!(out, DownloadOutcome::Downloaded(body.len() as u64));
        assert_eq!(seen, body.len() as u64);
        assert_eq!(tokio::fs::read(&dest).await.unwrap(), body);

        // Second run skips.
        let out = download_file(
            &client,
            &DownloadSpec {
                url: &format!("http://{addr}/file.bin"),
                dest: &dest,
                sha1: Some(&sha),
                size: None,
            },
            &cancel,
            false,
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(out, DownloadOutcome::Skipped);
    }

    #[tokio::test]
    async fn checksum_mismatch_leaves_no_file() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 1024];
                    let _ = socket.read(&mut buf).await;
                    let _ = socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nbad",
                        )
                        .await;
                });
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("file.bin");
        let cancel = CancellationToken::new();
        let err = download_file(
            &reqwest::Client::new(),
            &DownloadSpec {
                url: &format!("http://{addr}/f"),
                dest: &dest,
                sha1: Some("0000000000000000000000000000000000000000"),
                size: None,
            },
            &cancel,
            false,
            |_| {},
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::ChecksumMismatch);
        assert!(!tokio::fs::try_exists(&dest).await.unwrap());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[tokio::test]
    async fn cancellation_is_honoured() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = download_file(
            &reqwest::Client::new(),
            &DownloadSpec {
                url: "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
                dest: &dir.path().join("x"),
                sha1: None,
                size: None,
            },
            &cancel,
            false,
            |_| {},
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
    }

    #[tokio::test]
    async fn persists_history_and_marks_interrupted_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let sink: TaskSink = Arc::new(|_| {});
        let mgr = DownloadManager::with_store(sink.clone(), path.clone(), 2);
        let t = mgr.create("install", "安装测试版本", 3, 100).await;
        t.add_bytes(40).await;
        t.file_finished().await;
        mgr.save().await.unwrap();

        let restored = DownloadManager::with_store(sink.clone(), path.clone(), 2);
        let list = restored.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "安装测试版本");
        assert_eq!(list[0].status, TaskStatus::Failed);
        assert_eq!(list[0].error.as_ref().unwrap().code, ErrorCode::Interrupted);
        assert_eq!(list[0].done_bytes, 40);
        assert!(list[0].finished_at.is_some());
    }

    #[tokio::test]
    async fn persists_completed_tasks_without_rewriting_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let sink: TaskSink = Arc::new(|_| {});
        let mgr = DownloadManager::with_store(sink.clone(), path.clone(), 2);
        let t = mgr.create("install", "done", 1, 10).await;
        t.add_bytes(10).await;
        t.complete().await;
        mgr.save().await.unwrap();

        let restored = DownloadManager::with_store(sink, path, 2);
        let list = restored.list().await;
        assert_eq!(list[0].status, TaskStatus::Completed);
        assert!(list[0].error.is_none());
    }

    #[tokio::test]
    async fn corrupt_history_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        tokio::fs::write(&path, b"{not json").await.unwrap();
        let mgr = DownloadManager::with_store(Arc::new(|_| {}), path, 2);
        assert!(mgr.list().await.is_empty());
    }

    #[tokio::test]
    async fn global_semaphore_limits_across_tasks() {
        let mgr = DownloadManager::new(Arc::new(|_| {}));
        mgr.set_limit(1);
        assert_eq!(mgr.limit(), 1);
        let first = mgr.acquire().await;
        let acquired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = acquired.clone();
        let mgr2 = mgr.clone();
        let waiter = tokio::spawn(async move {
            let permit = mgr2.acquire().await;
            flag.store(true, Ordering::Relaxed);
            drop(permit);
        });
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(!acquired.load(Ordering::Relaxed));
        drop(first);
        tokio::time::timeout(Duration::from_secs(2), waiter)
            .await
            .unwrap()
            .unwrap();
        assert!(acquired.load(Ordering::Relaxed));
    }

    struct RangeServer {
        addr: std::net::SocketAddr,
        requests: Arc<std::sync::Mutex<Vec<String>>>,
    }

    async fn spawn_range_server(
        body: Vec<u8>,
        etag: &'static str,
        support_range: bool,
    ) -> RangeServer {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let body = body.clone();
                let recorded = recorded.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let mut raw = Vec::new();
                    loop {
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let request = String::from_utf8_lossy(&raw).into_owned();
                    recorded.lock().unwrap().push(request.clone());
                    let lower = request.to_ascii_lowercase();
                    let range_start = if support_range {
                        lower
                            .lines()
                            .find_map(|l| l.strip_prefix("range: bytes="))
                            .and_then(|v| v.split('-').next())
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    } else {
                        None
                    };
                    let if_range_ok = lower
                        .lines()
                        .find_map(|l| l.strip_prefix("if-range:"))
                        .map(|v| v.trim().trim_matches('"') == etag.trim_matches('"'))
                        .unwrap_or(false);
                    if let Some(start) = range_start
                        .filter(|s| *s < body.len())
                        .filter(|_| if_range_ok)
                    {
                        let slice = &body[start..];
                        let resp = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nETag: {}\r\nConnection: close\r\n\r\n",
                            slice.len(),
                            start,
                            body.len() - 1,
                            body.len(),
                            etag
                        );
                        let _ = socket.write_all(resp.as_bytes()).await;
                        let _ = socket.write_all(slice).await;
                    } else {
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                            body.len(),
                            etag
                        );
                        let _ = socket.write_all(resp.as_bytes()).await;
                        let _ = socket.write_all(&body).await;
                    }
                });
            }
        });
        RangeServer { addr, requests }
    }

    async fn seed_partial(dest: &Path, body: &[u8], prefix_len: usize, etag: Option<&str>) {
        let name = dest.file_name().unwrap().to_string_lossy();
        let tmp = dest.with_file_name(format!(".{name}.srpart"));
        tokio::fs::write(&tmp, &body[..prefix_len]).await.unwrap();
        let meta = PartialMeta {
            url: format!("http://127.0.0.1/{}", name),
            etag: etag.map(|s| s.to_string()),
            last_modified: None,
            total: Some(body.len() as u64),
        };
        tokio::fs::write(partial_meta_path(&tmp), serde_json::to_vec(&meta).unwrap())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn resumes_with_if_range_and_validates() {
        let body = b"hello world".to_vec();
        let sha = util::sha1_bytes(&body);
        let server = spawn_range_server(body.clone(), "\"v1\"", true).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("file.bin");
        // seed uses a placeholder URL; rewrite it to the real one below.
        seed_partial(&dest, &body, 6, Some("\"v1\"")).await;
        let name = dest.file_name().unwrap().to_string_lossy();
        let tmp = dest.with_file_name(format!(".{name}.srpart"));
        let meta = PartialMeta {
            url: format!("http://{}/file.bin", server.addr),
            etag: Some("\"v1\"".into()),
            last_modified: None,
            total: Some(body.len() as u64),
        };
        tokio::fs::write(partial_meta_path(&tmp), serde_json::to_vec(&meta).unwrap())
            .await
            .unwrap();

        let cancel = CancellationToken::new();
        let mut seen = 0u64;
        let out = download_file(
            &reqwest::Client::new(),
            &DownloadSpec {
                url: &format!("http://{}/file.bin", server.addr),
                dest: &dest,
                sha1: Some(&sha),
                size: Some(body.len() as u64),
            },
            &cancel,
            false,
            |n| seen = n,
        )
        .await
        .unwrap();
        assert_eq!(out, DownloadOutcome::Downloaded(body.len() as u64));
        assert_eq!(seen, body.len() as u64);
        assert_eq!(tokio::fs::read(&dest).await.unwrap(), body);
        assert!(!tokio::fs::try_exists(&tmp).await.unwrap());
        let requests = server.requests.lock().unwrap();
        let req = requests.first().unwrap().to_ascii_lowercase();
        assert!(req.contains("range: bytes=6-"), "{req}");
        assert!(req.contains("if-range:"), "{req}");
    }

    #[tokio::test]
    async fn restarts_when_server_ignores_range() {
        let body = b"hello world".to_vec();
        let sha = util::sha1_bytes(&body);
        let server = spawn_range_server(body.clone(), "\"v1\"", false).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("file.bin");
        seed_partial(&dest, &body, 6, Some("\"v1\"")).await;
        let name = dest.file_name().unwrap().to_string_lossy();
        let tmp = dest.with_file_name(format!(".{name}.srpart"));
        let meta = PartialMeta {
            url: format!("http://{}/file.bin", server.addr),
            etag: Some("\"v1\"".into()),
            last_modified: None,
            total: Some(body.len() as u64),
        };
        tokio::fs::write(partial_meta_path(&tmp), serde_json::to_vec(&meta).unwrap())
            .await
            .unwrap();

        let cancel = CancellationToken::new();
        let mut seen = 0u64;
        download_file(
            &reqwest::Client::new(),
            &DownloadSpec {
                url: &format!("http://{}/file.bin", server.addr),
                dest: &dest,
                sha1: Some(&sha),
                size: Some(body.len() as u64),
            },
            &cancel,
            false,
            |n| seen = n,
        )
        .await
        .unwrap();
        assert_eq!(seen, body.len() as u64);
        assert_eq!(tokio::fs::read(&dest).await.unwrap(), body);
    }

    #[tokio::test]
    async fn does_not_resume_without_validator() {
        let body = b"hello world".to_vec();
        let sha = util::sha1_bytes(&body);
        let server = spawn_range_server(body.clone(), "\"v1\"", true).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("file.bin");
        seed_partial(&dest, &body, 6, None).await;
        let name = dest.file_name().unwrap().to_string_lossy();
        let tmp = dest.with_file_name(format!(".{name}.srpart"));
        let meta = PartialMeta {
            url: format!("http://{}/file.bin", server.addr),
            etag: None,
            last_modified: None,
            total: Some(body.len() as u64),
        };
        tokio::fs::write(partial_meta_path(&tmp), serde_json::to_vec(&meta).unwrap())
            .await
            .unwrap();

        let cancel = CancellationToken::new();
        download_file(
            &reqwest::Client::new(),
            &DownloadSpec {
                url: &format!("http://{}/file.bin", server.addr),
                dest: &dest,
                sha1: Some(&sha),
                size: Some(body.len() as u64),
            },
            &cancel,
            false,
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(tokio::fs::read(&dest).await.unwrap(), body);
        let requests = server.requests.lock().unwrap();
        assert!(!requests[0].to_ascii_lowercase().contains("range:"));
    }

    #[tokio::test]
    async fn task_manager_tracks_progress_and_cancel() {
        let updates = Arc::new(AtomicUsize::new(0));
        let u = updates.clone();
        let manager = DownloadManager::new(Arc::new(move |_view| {
            u.fetch_add(1, Ordering::Relaxed);
        }));
        let task = manager.create("install", "测试", 2, 100).await;
        task.add_bytes(40).await;
        task.file_finished().await;
        let list = manager.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].done_bytes, 40);
        assert_eq!(list[0].done_files, 1);
        assert!(updates.load(Ordering::Relaxed) >= 3);
        manager.cancel(&task.id).await.unwrap();
        assert!(task.is_cancelled());
        task.cancelled().await;
        assert_eq!(manager.list().await[0].status, TaskStatus::Cancelled);
    }
}
