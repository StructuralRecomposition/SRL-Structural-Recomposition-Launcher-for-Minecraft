use crate::error::{AppError, Result};
use sha1::{Digest, Sha1};
use std::path::{Component, Path, PathBuf};
use tokio::io::AsyncReadExt;

/// Join an untrusted relative path onto a trusted base directory.
///
/// Rejects absolute paths, parent traversal, Windows drive/UNC syntax and NUL
/// bytes. This is the only sanctioned way to turn metadata-provided paths into
/// filesystem paths.
pub fn safe_join(base: &Path, relative: &str) -> Result<PathBuf> {
    if relative.trim().is_empty() {
        return Err(AppError::path_unsafe("路径为空"));
    }
    if relative.contains('\0') {
        return Err(AppError::path_unsafe("路径包含非法字符"));
    }
    // Reject Windows-style separators and drive prefixes on every platform so
    // that a manifest authored for one OS cannot escape on another.
    if relative.contains('\\') {
        return Err(AppError::path_unsafe(format!("路径包含反斜杠：{relative}")));
    }
    let rel = Path::new(relative);
    if rel.is_absolute() {
        return Err(AppError::path_unsafe(format!("拒绝绝对路径：{relative}")));
    }
    for comp in rel.components() {
        match comp {
            Component::Normal(part) => {
                // Reject Windows drive prefixes and alternate data streams.
                if part.to_string_lossy().contains(':') {
                    return Err(AppError::path_unsafe(format!(
                        "路径包含盘符或非法冒号：{relative}"
                    )));
                }
            }
            Component::CurDir => {}
            _ => {
                return Err(AppError::path_unsafe(format!(
                    "路径包含非法片段：{relative}"
                )))
            }
        }
    }
    let joined = base.join(rel);
    if !joined.starts_with(base) {
        return Err(AppError::path_unsafe(format!("路径越界：{relative}")));
    }
    Ok(joined)
}

/// Best-effort sanitisation for a single filename component (instance names,
/// log names). Never used for untrusted relative paths.
pub fn safe_component(input: &str, fallback: &str) -> String {
    let cleaned: String = input
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

pub async fn ensure_dir(path: &Path) -> Result<()> {
    tokio::fs::create_dir_all(path).await.map_err(|e| {
        AppError::new(
            crate::error::ErrorCode::Io,
            format!("无法创建目录 {}", path.display()),
        )
        .with_detail(e.to_string())
    })
}

/// Write bytes atomically: temp file in the same directory, fsync, rename.
pub async fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::path_unsafe("目标路径没有父目录"))?;
    ensure_dir(parent).await?;
    let tmp = parent.join(format!(".{}.tmp-{}", file_name(path), uuid::Uuid::new_v4()));
    {
        use tokio::io::AsyncWriteExt;
        let mut f = tokio::fs::File::create(&tmp).await?;
        f.write_all(bytes).await?;
        f.flush().await?;
        f.sync_all().await?;
    }
    #[cfg(windows)]
    {
        // std::fs::rename cannot replace an existing file on Windows.
        if tokio::fs::try_exists(path).await.unwrap_or(false) {
            tokio::fs::remove_file(path).await?;
        }
    }
    if let Err(e) = tokio::fs::rename(&tmp, path).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(AppError::from(e));
    }
    Ok(())
}

pub async fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let value = serde_json::from_slice(&bytes).map_err(|e| {
                AppError::internal(format!("配置文件损坏：{}", path.display()))
                    .with_detail(e.to_string())
            })?;
            Ok(Some(value))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    atomic_write(path, &bytes).await
}

pub async fn sha1_file(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn sha1_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string())
}

/// Offline UUID, matching the vanilla convention:
/// md5("OfflinePlayer:<name>") with RFC 4122 version/variant bits applied.
pub fn offline_uuid(name: &str) -> String {
    use md5::{Digest as _, Md5};
    let mut h = Md5::new();
    h.update(format!("OfflinePlayer:{name}").as_bytes());
    let mut bytes = h.finalize();
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let s = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &s[0..8],
        &s[8..12],
        &s[12..16],
        &s[16..20],
        &s[20..32]
    )
}

/// Remove well-known secret values from arbitrary text before logging.
pub fn redact_secrets(input: &str) -> String {
    let keys = [
        "accessToken",
        "access_token",
        "refreshToken",
        "refresh_token",
        "authorization",
        "Authorization",
        "clientSecret",
        "client_secret",
    ];
    let mut out = redact_bearer(input.to_string());
    for key in keys {
        let mut search_from = 0usize;
        while let Some(pos) = out[search_from..].find(key) {
            let start = search_from + pos;
            let after_key = start + key.len();
            // Find the value start: skip whitespace, quotes, ':' and '='
            let bytes = out.as_bytes();
            let mut i = after_key;
            while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'"' | b'\'' | b':' | b'=') {
                i += 1;
            }
            let value_start = i;
            let mut end = value_start;
            while end < bytes.len() {
                let b = bytes[end];
                if b.is_ascii_whitespace() || matches!(b, b'"' | b'\'' | b',' | b'}' | b'&' | b';')
                {
                    break;
                }
                end += 1;
            }
            if end > value_start {
                out.replace_range(value_start..end, "<redacted>");
                search_from = value_start + "<redacted>".len();
            } else {
                search_from = after_key;
            }
        }
    }
    // Mask `Authorization: Bearer <token>` style values first so the generic
    // key/value pass cannot stop at the word "Bearer".
    out
}

fn redact_bearer(input: String) -> String {
    let mut out = input;
    let mut search = 0usize;
    while let Some(pos) = out[search..].find("Bearer ") {
        let start = search + pos + "Bearer ".len();
        let end_rel = out[start..]
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | '}' | '&' | ';'))
            .unwrap_or(out.len() - start);
        if end_rel > 0 {
            out.replace_range(start..start + end_rel, "<redacted>");
            search = start + "<redacted>".len();
        } else {
            search = start;
        }
    }
    out
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn short_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_accepts_normal_paths() {
        let base = Path::new("/tmp/root");
        assert_eq!(
            safe_join(base, "libraries/foo/bar.jar").unwrap(),
            base.join("libraries/foo/bar.jar")
        );
        assert_eq!(safe_join(base, "a/./b").unwrap(), base.join("a/./b"));
    }

    #[test]
    fn safe_join_rejects_traversal_and_absolute() {
        let base = Path::new("/tmp/root");
        for bad in [
            "../etc/passwd",
            "a/../../b",
            "/etc/passwd",
            "C:/Windows/system32",
            "a\\b",
            "..",
            "",
            "a/\0b",
        ] {
            assert!(safe_join(base, bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn component_is_sanitised() {
        assert_eq!(safe_component("My/Inst:ance", "x"), "My_Inst_ance");
        assert_eq!(safe_component("...", "fallback"), "fallback");
        assert_eq!(safe_component("  ", "fallback"), "fallback");
    }

    #[test]
    fn offline_uuid_is_stable_and_versioned() {
        let a = offline_uuid("Steve");
        let b = offline_uuid("Steve");
        assert_eq!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "3");
        assert!(matches!(&a[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(a, offline_uuid("Alex"));
    }

    #[test]
    fn redaction_handles_json_and_query() {
        let s = r#"{"accessToken":"abc123","refresh_token":"ref456","name":"ok"}"#;
        let r = redact_secrets(s);
        assert!(!r.contains("abc123"));
        assert!(!r.contains("ref456"));
        assert!(r.contains("<redacted>"));
        assert!(r.contains("\"name\":\"ok\""));
        let q = "https://x/y?access_token=secretvalue&a=1";
        assert!(!redact_secrets(q).contains("secretvalue"));
    }

    #[tokio::test]
    async fn atomic_write_replaces_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nested/settings.json");
        atomic_write(&p, br#"{"a":1}"#).await.unwrap();
        atomic_write(&p, br#"{"a":2}"#).await.unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&tokio::fs::read(&p).await.unwrap()).unwrap();
        assert_eq!(v["a"], 2);
        let leftovers: Vec<_> = std::fs::read_dir(p.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[tokio::test]
    async fn sha1_matches_known_vector() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x");
        tokio::fs::write(&p, b"abc").await.unwrap();
        assert_eq!(
            sha1_file(&p).await.unwrap(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }
}
