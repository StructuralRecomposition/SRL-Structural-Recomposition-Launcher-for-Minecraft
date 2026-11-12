//! Bounded JSONL journal for room history and the unified room log.
//!
//! Sensitive values are redacted before writing. The file is capped by size and
//! trimmed to the newest entries so a long-running room cannot grow without
//! bound.

use super::types::RoomLogEntry;
use crate::error::Result;
use crate::paths::AppPaths;
use crate::util;
use std::io::Write;

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

type FileLocks = std::sync::Mutex<
    std::collections::HashMap<std::path::PathBuf, std::sync::Arc<std::sync::Mutex<()>>>,
>;
static FILE_LOCKS: std::sync::OnceLock<FileLocks> = std::sync::OnceLock::new();

fn file_lock(path: &std::path::Path) -> std::sync::Arc<std::sync::Mutex<()>> {
    let locks = FILE_LOCKS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut map = locks.lock().unwrap_or_else(|error| error.into_inner());
    map.entry(path.to_path_buf())
        .or_insert_with(|| std::sync::Arc::new(std::sync::Mutex::new(())))
        .clone()
}

pub fn journal_path(paths: &AppPaths) -> std::path::PathBuf {
    paths.multiplayer_dir().join("room-log.jsonl")
}

pub async fn append(
    paths: &AppPaths,
    source: &str,
    level: &str,
    event_type: &str,
    message: &str,
    tunnel_id: Option<u64>,
) -> Result<()> {
    let entry = RoomLogEntry {
        timestamp: util::now_rfc3339(),
        source: source.into(),
        level: level.into(),
        event_type: event_type.into(),
        message: util::redact_secrets(message),
        tunnel_id,
    };
    append_entry(paths, &entry).await
}

pub async fn append_entry(paths: &AppPaths, entry: &RoomLogEntry) -> Result<()> {
    util::ensure_dir(&paths.multiplayer_dir()).await?;
    let path = journal_path(paths);
    let line = serde_json::to_string(entry)?;
    let lock = file_lock(&path);
    tokio::task::spawn_blocking(move || -> Result<()> {
        let _guard = lock.lock().unwrap_or_else(|error| error.into_inner());
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(file, "{line}")?;
        file.flush()?;
        let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        if size > MAX_FILE_BYTES {
            drop(file);
            trim(&path)?;
        }
        Ok(())
    })
    .await
    .map_err(|error| crate::error::AppError::internal(format!("写入房间日志失败：{error}")))??;
    Ok(())
}

fn trim(path: &std::path::Path) -> Result<()> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut keep: Vec<&str> = Vec::new();
    let mut bytes = 0usize;
    for line in text.lines().rev() {
        let line_bytes = line.len() + 1;
        if bytes + line_bytes > MAX_FILE_BYTES as usize {
            break;
        }
        keep.push(line);
        bytes += line_bytes;
    }
    keep.reverse();
    let body = if keep.is_empty() {
        String::new()
    } else {
        keep.join("\n") + "\n"
    };
    crate::util::write_private_file_sync(path, body.as_bytes())
}

pub async fn read(
    paths: &AppPaths,
    limit: usize,
    level: Option<&str>,
) -> Result<Vec<RoomLogEntry>> {
    let path = journal_path(paths);
    let text = match tokio::fs::read_to_string(&path).await {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let limit = limit.clamp(1, 2000);
    let mut entries = Vec::new();
    for line in text.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<RoomLogEntry>(line) else {
            continue;
        };
        if let Some(level) = level {
            if !entry.level.eq_ignore_ascii_case(level) {
                continue;
            }
        }
        entries.push(entry);
        if entries.len() >= limit {
            break;
        }
    }
    entries.reverse();
    Ok(entries)
}

pub async fn clear(paths: &AppPaths) -> Result<u64> {
    let path = journal_path(paths);
    let count = tokio::fs::read_to_string(&path)
        .await
        .map(|text| text.lines().filter(|line| !line.trim().is_empty()).count())
        .unwrap_or(0);
    match tokio::fs::remove_file(&path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(count as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writes_reads_and_redacts() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        append(
            &paths,
            "frpc",
            "info",
            "frpc.start",
            "token=secret-value password=abc",
            Some(7),
        )
        .await
        .unwrap();
        let entries = read(&paths, 10, None).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].message.contains("secret-value"));
        assert!(!entries[0].message.contains("password=abc"));
        assert_eq!(entries[0].tunnel_id, Some(7));
    }

    #[tokio::test]
    async fn concurrent_appends_keep_every_line() {
        let dir = tempfile::tempdir().unwrap();
        let paths = std::sync::Arc::new(AppPaths::new(dir.path()));
        let mut tasks = Vec::new();
        for index in 0..40 {
            let paths = paths.clone();
            tasks.push(tokio::spawn(async move {
                append(
                    &paths,
                    "room",
                    "info",
                    "room.test",
                    &format!("event-{index}"),
                    None,
                )
                .await
                .unwrap();
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        let entries = read(&paths, 100, None).await.unwrap();
        assert_eq!(entries.len(), 40);
    }

    #[tokio::test]
    async fn filters_by_level_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        append(&paths, "room", "info", "room.create", "ok", None)
            .await
            .unwrap();
        append(&paths, "room", "error", "room.error", "bad", None)
            .await
            .unwrap();
        let errors = read(&paths, 10, Some("error")).await.unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(clear(&paths).await.unwrap(), 2);
        assert!(read(&paths, 10, None).await.unwrap().is_empty());
    }
}
