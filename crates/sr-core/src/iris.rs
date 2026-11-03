//! Iris shader support installer backed by the official Modrinth API.
//!
//! Only the public Modrinth HTTP API and CDN are used; no installer binaries
//! or third-party mirrors. Every downloaded file is size-limited and, when
//! Modrinth provides a SHA-1, verified before it is written into `mods/`.

use crate::download::ensure_trusted_url;
use crate::error::{AppError, ErrorCode, Result};
use crate::instance::Instance;
use crate::mods::{self, ModEntry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MODRINTH_API: &str = "https://api.modrinth.com/v2";
pub const IRIS_PROJECT: &str = "iris";
const MAX_META_BYTES: usize = 8 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IrisVersion {
    pub id: String,
    pub version_number: String,
    pub name: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub date_published: String,
    pub file_name: String,
    pub url: String,
    pub sha1: Option<String>,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
struct RawVersion {
    id: String,
    version_number: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    date_published: String,
    #[serde(default)]
    files: Vec<RawFile>,
}

#[derive(Debug, Deserialize)]
struct RawFile {
    #[serde(default)]
    hashes: BTreeMap<String, String>,
    url: String,
    filename: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    primary: bool,
}

fn validate_segment(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
    {
        return Err(AppError::invalid_input(format!("{label} 格式非法")));
    }
    Ok(())
}

pub fn parse_versions(bytes: &[u8]) -> Result<Vec<IrisVersion>> {
    let raw: Vec<RawVersion> = serde_json::from_slice(bytes).map_err(|e| {
        AppError::new(ErrorCode::VersionInvalid, "Iris 版本元数据格式无效")
            .with_detail(e.to_string())
    })?;
    let mut out = Vec::new();
    for version in raw {
        let file = version
            .files
            .iter()
            .find(|f| f.primary)
            .or_else(|| version.files.first());
        let Some(file) = file else { continue };
        out.push(IrisVersion {
            id: version.id,
            version_number: version.version_number,
            name: version.name,
            game_versions: version.game_versions,
            loaders: version.loaders,
            date_published: version.date_published,
            file_name: file.filename.clone(),
            url: file.url.clone(),
            sha1: file.hashes.get("sha1").cloned(),
            size: file.size,
        });
    }
    out.sort_by(|a, b| b.date_published.cmp(&a.date_published));
    Ok(out)
}

/// Pick the newest Iris release that explicitly supports the given Minecraft
/// version and loader. Modrinth already filters, but the check is repeated so
/// a malformed or cached response can never install an incompatible jar.
pub fn select_latest<'a>(
    versions: &'a [IrisVersion],
    game_version: &str,
    loader: &str,
) -> Option<&'a IrisVersion> {
    versions.iter().find(|v| {
        v.game_versions.iter().any(|g| g == game_version)
            && v.loaders.iter().any(|l| l.eq_ignore_ascii_case(loader))
    })
}

pub async fn fetch_versions(
    http: &reqwest::Client,
    game_version: &str,
    loader: &str,
) -> Result<Vec<IrisVersion>> {
    validate_segment(game_version, "Minecraft 版本")?;
    validate_segment(loader, "加载器")?;
    let mut url = reqwest::Url::parse(&format!("{MODRINTH_API}/project/{IRIS_PROJECT}/version"))
        .map_err(|_| AppError::invalid_input("Modrinth API 地址无效"))?;
    url.query_pairs_mut()
        .append_pair("game_versions", &format!("[\"{game_version}\"]"))
        .append_pair("loaders", &format!("[\"{loader}\"]"));
    let url = ensure_trusted_url(url.as_str(), false)?;
    let resp = http
        .get(url)
        .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::new(
            ErrorCode::Http,
            format!("Modrinth 返回 HTTP {}", status.as_u16()),
        )
        .retryable(status.is_server_error() || status.as_u16() == 429));
    }
    let bytes = resp.bytes().await?;
    if bytes.len() > MAX_META_BYTES {
        return Err(AppError::new(
            ErrorCode::VersionInvalid,
            "Iris 版本元数据异常过大",
        ));
    }
    parse_versions(&bytes)
}

/// Download and install the latest compatible Iris jar into the instance's
/// `mods/` directory. Existing Iris jars are never overwritten.
pub async fn install_latest(
    http: &reqwest::Client,
    instance: &Instance,
    game_version: &str,
    loader: &str,
) -> Result<ModEntry> {
    let loader = loader.to_ascii_lowercase();
    if loader != "fabric" && loader != "quilt" {
        return Err(AppError::new(
            ErrorCode::Unsupported,
            "Iris 需要 Fabric 或 Quilt 实例",
        ));
    }
    for entry in mods::list(instance).await? {
        if entry.file_name.to_lowercase().contains("iris") {
            if !entry.enabled {
                // A disabled Iris jar already exists: enabling it is the
                // expected outcome and avoids a second copy in mods/.
                return mods::toggle(instance, &entry.file_name, true).await;
            }
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                format!("该实例已存在 Iris：{}", entry.file_name),
            ));
        }
    }
    let versions = fetch_versions(http, game_version, &loader).await?;
    let version = select_latest(&versions, game_version, &loader).ok_or_else(|| {
        AppError::not_found(format!(
            "Modrinth 上没有适用于 Minecraft {game_version} / {loader} 的 Iris 版本"
        ))
    })?;
    if version.size > MAX_FILE_BYTES {
        return Err(AppError::new(
            ErrorCode::VersionInvalid,
            "Iris 文件异常过大，已拒绝下载",
        ));
    }
    let url = ensure_trusted_url(&version.url, false)?;
    let resp = http
        .get(url)
        .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::new(
            ErrorCode::Http,
            format!("Iris 下载返回 HTTP {}", status.as_u16()),
        )
        .retryable(status.is_server_error() || status.as_u16() == 429));
    }
    if let Some(len) = resp.content_length() {
        if len > MAX_FILE_BYTES {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                "Iris 文件异常过大，已拒绝下载",
            ));
        }
    }
    let bytes = resp.bytes().await?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(AppError::new(
            ErrorCode::VersionInvalid,
            "Iris 文件异常过大，已拒绝下载",
        ));
    }
    let expected = version.sha1.as_ref().ok_or_else(|| {
        AppError::new(
            ErrorCode::VersionInvalid,
            "Modrinth 未提供 Iris 文件的 SHA-1，已拒绝安装",
        )
    })?;
    let actual = crate::util::sha1_bytes(&bytes);
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(AppError::checksum(expected, &actual));
    }
    mods::import_bytes(instance, &version.file_name, &bytes, false).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/iris_versions.json");

    #[test]
    fn parses_primary_files_and_sorts_newest_first() {
        let list = parse_versions(FIXTURE.as_bytes()).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].version_number, "1.8.0");
        assert_eq!(list[0].file_name, "iris-1.8.0.jar");
        assert_eq!(
            list[0].sha1.as_deref(),
            Some("1111111111111111111111111111111111111111")
        );
        assert!(list[0].size > 0);
    }

    #[test]
    fn selects_only_matching_game_and_loader() {
        let list = parse_versions(FIXTURE.as_bytes()).unwrap();
        let fabric = select_latest(&list, "1.20.4", "fabric").unwrap();
        assert_eq!(fabric.version_number, "1.8.0");
        let quilt = select_latest(&list, "1.20.4", "quilt").unwrap();
        assert_eq!(quilt.version_number, "1.7.9");
        assert_eq!(
            select_latest(&list, "1.19.2", "fabric")
                .unwrap()
                .version_number,
            "1.6.0"
        );
        assert!(select_latest(&list, "1.20.4", "forge").is_none());
    }

    #[test]
    fn rejects_invalid_metadata_and_segments() {
        assert!(parse_versions(b"{}").is_err());
        assert!(validate_segment("../x", "版本").is_err());
        assert!(validate_segment("1.20.4", "版本").is_ok());
    }
}
