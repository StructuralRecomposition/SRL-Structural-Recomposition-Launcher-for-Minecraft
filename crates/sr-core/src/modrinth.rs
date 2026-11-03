//! Modrinth resource discovery.
//!
//! Only the official Modrinth API (`api.modrinth.com`) is used. File URLs are
//! returned by the API and are verified by SHA-1/SHA-512 at download time; the
//! launcher never accepts a URL from the renderer.

use crate::error::{AppError, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const BASE: &str = "https://api.modrinth.com/v2";
const MAX_JSON_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSearchQuery {
    #[serde(default)]
    pub query: String,
    /// `mod` | `modpack` | `shader` | `resourcepack`
    pub kind: String,
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub index: Option<String>,
    #[serde(default)]
    pub offset: Option<u32>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSearchPage {
    pub hits: Vec<ResourceHit>,
    pub offset: u32,
    pub limit: u32,
    pub total_hits: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceHit {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub categories: Vec<String>,
    pub versions: Vec<String>,
    pub downloads: u64,
    pub follows: u64,
    pub icon_url: Option<String>,
    pub date_modified: Option<String>,
    pub latest_version: Option<String>,
    pub license: Option<String>,
    pub client_side: Option<String>,
    pub server_side: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceProject {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub categories: Vec<String>,
    pub downloads: u64,
    pub follows: u64,
    pub icon_url: Option<String>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub license: Option<String>,
    pub source_url: Option<String>,
    pub issues_url: Option<String>,
    pub wiki_url: Option<String>,
    pub website_url: String,
    pub published: Option<String>,
    pub updated: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameVersionInfo {
    pub version: String,
    pub version_type: String,
    pub date: Option<String>,
    pub major: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceVersion {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    pub version_type: String,
    pub date_published: Option<String>,
    pub downloads: u64,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<ResourceFile>,
    pub dependencies: Vec<ResourceDependency>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceFile {
    pub filename: String,
    pub url: String,
    pub size: u64,
    pub primary: bool,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub file_type: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDependency {
    pub version_id: Option<String>,
    pub project_id: Option<String>,
    pub dependency_type: String,
}

#[derive(Debug, Clone)]
pub struct ModrinthService {
    http: reqwest::Client,
}

impl ModrinthService {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: reqwest::Url) -> Result<T> {
        let resp = self
            .http
            .get(url)
            .timeout(Duration::from_secs(25))
            .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
            .send()
            .await?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::not_found("Modrinth 资源不存在"));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(
                AppError::new(ErrorCode::Http, "Modrinth 请求过于频繁，请稍后再试").retryable(true),
            );
        }
        if !status.is_success() {
            return Err(AppError::new(
                ErrorCode::Http,
                format!("Modrinth 返回 HTTP {}", status.as_u16()),
            ));
        }
        let bytes = resp.bytes().await?;
        if bytes.len() > MAX_JSON_BYTES {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                "Modrinth 返回数据异常过大",
            ));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            AppError::new(ErrorCode::InvalidInput, "Modrinth 返回数据格式无效")
                .with_detail(e.to_string())
        })
    }

    pub async fn search(&self, query: &ResourceSearchQuery) -> Result<ResourceSearchPage> {
        let kind = normalize_kind(&query.kind)?;
        let mut url = reqwest::Url::parse(&format!("{BASE}/search"))
            .map_err(|e| AppError::internal(format!("Modrinth URL 无效：{e}")))?;
        {
            let mut pairs = url.query_pairs_mut();
            if !query.query.trim().is_empty() {
                pairs.append_pair("query", query.query.trim());
            }
            pairs.append_pair("limit", &query.limit.unwrap_or(20).clamp(1, 50).to_string());
            pairs.append_pair("offset", &query.offset.unwrap_or(0).to_string());
            let index = match query.index.as_deref() {
                Some("downloads") => "downloads",
                Some("follows") => "follows",
                Some("newest") => "newest",
                Some("updated") => "updated",
                _ => "relevance",
            };
            pairs.append_pair("index", index);
            let mut facets: Vec<Vec<String>> = vec![vec![format!("project_type:{kind}")]];
            if let Some(version) = query.game_version.as_deref().filter(|v| valid_token(v)) {
                facets.push(vec![format!("versions:{version}")]);
            }
            if let Some(loader) = query.loader.as_deref().filter(|v| valid_token(v)) {
                facets.push(vec![format!("categories:{loader}")]);
            }
            if let Some(category) = query.category.as_deref().filter(|v| valid_token(v)) {
                facets.push(vec![format!("categories:{category}")]);
            }
            pairs.append_pair("facets", &serde_json::to_string(&facets)?);
        }
        let raw: RawSearchPage = self.get_json(url).await?;
        Ok(ResourceSearchPage {
            hits: raw.hits.into_iter().map(ResourceHit::from).collect(),
            offset: raw.offset,
            limit: raw.limit,
            total_hits: raw.total_hits,
        })
    }

    pub async fn project(&self, id: &str) -> Result<ResourceProject> {
        validate_id(id)?;
        let url = reqwest::Url::parse(&format!("{BASE}/project/{id}"))
            .map_err(|e| AppError::internal(format!("Modrinth URL 无效：{e}")))?;
        let raw: RawProject = self.get_json(url).await?;
        Ok(raw.into())
    }

    pub async fn versions(
        &self,
        id: &str,
        game_version: Option<&str>,
        loader: Option<&str>,
    ) -> Result<Vec<ResourceVersion>> {
        validate_id(id)?;
        let mut url = reqwest::Url::parse(&format!("{BASE}/project/{id}/version"))
            .map_err(|e| AppError::internal(format!("Modrinth URL 无效：{e}")))?;
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(version) = game_version.filter(|v| valid_token(v)) {
                pairs.append_pair("game_versions", &serde_json::to_string(&[version])?);
            }
            if let Some(loader) = loader.filter(|v| valid_token(v)) {
                pairs.append_pair("loaders", &serde_json::to_string(&[loader])?);
            }
        }
        let mut raw: Vec<RawVersion> = self.get_json(url).await?;
        raw.sort_by(|a, b| b.date_published.cmp(&a.date_published));
        Ok(raw.into_iter().map(ResourceVersion::from).collect())
    }

    pub async fn game_versions(&self) -> Result<Vec<GameVersionInfo>> {
        let url = reqwest::Url::parse(&format!("{BASE}/tag/game_version"))
            .map_err(|e| AppError::internal(format!("Modrinth URL 无效：{e}")))?;
        let raw: Vec<RawGameVersion> = self.get_json(url).await?;
        Ok(raw
            .into_iter()
            .map(|v| GameVersionInfo {
                version: v.version,
                version_type: v.version_type,
                date: v.date,
                major: v.major,
            })
            .collect())
    }

    /// Fetch one version and verify it belongs to the expected project.
    pub async fn version(&self, version_id: &str, project_id: &str) -> Result<ResourceVersion> {
        validate_id(version_id)?;
        let url = reqwest::Url::parse(&format!("{BASE}/version/{version_id}"))
            .map_err(|e| AppError::internal(format!("Modrinth URL 无效：{e}")))?;
        let raw: RawVersion = self.get_json(url).await?;
        if raw.project_id != project_id {
            return Err(AppError::invalid_input("资源版本与项目不匹配"));
        }
        Ok(raw.into())
    }
}

fn normalize_kind(kind: &str) -> Result<&'static str> {
    match kind {
        "mod" => Ok("mod"),
        "modpack" => Ok("modpack"),
        "shader" => Ok("shader"),
        "resourcepack" => Ok("resourcepack"),
        _ => Err(AppError::invalid_input("不支持的资源类型")),
    }
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn validate_id(value: &str) -> Result<()> {
    if valid_token(value) {
        Ok(())
    } else {
        Err(AppError::invalid_input("Modrinth 资源 ID 非法"))
    }
}

// ---------------------------------------------------------------------------
// Raw API shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
struct RawGameVersion {
    #[serde(default)]
    version: String,
    #[serde(default)]
    version_type: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    major: bool,
}

#[derive(Debug, Deserialize)]
struct RawSearchPage {
    #[serde(default)]
    hits: Vec<RawHit>,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    limit: u32,
    #[serde(default)]
    total_hits: u32,
}

#[derive(Debug, Deserialize, Default)]
struct RawHit {
    #[serde(default)]
    project_id: String,
    #[serde(default)]
    project_type: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    versions: Vec<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    follows: u64,
    #[serde(default)]
    icon_url: Option<String>,
    #[serde(default)]
    date_modified: Option<String>,
    #[serde(default)]
    latest_version: Option<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    client_side: Option<String>,
    #[serde(default)]
    server_side: Option<String>,
}

impl From<RawHit> for ResourceHit {
    fn from(h: RawHit) -> Self {
        Self {
            id: h.project_id,
            kind: h.project_type,
            source: "modrinth".into(),
            slug: h.slug,
            title: h.title,
            description: h.description,
            author: h.author,
            categories: h.categories,
            versions: h.versions,
            downloads: h.downloads,
            follows: h.follows,
            icon_url: h.icon_url,
            date_modified: h.date_modified,
            latest_version: h.latest_version,
            license: h.license,
            client_side: h.client_side,
            server_side: h.server_side,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct RawProject {
    #[serde(default)]
    id: String,
    #[serde(default)]
    project_type: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    followers: u64,
    #[serde(default)]
    icon_url: Option<String>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    license: Option<RawLicense>,
    #[serde(default)]
    source_url: Option<String>,
    #[serde(default)]
    issues_url: Option<String>,
    #[serde(default)]
    wiki_url: Option<String>,
    #[serde(default)]
    published: Option<String>,
    #[serde(default)]
    updated: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawLicense {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
}

impl From<RawProject> for ResourceProject {
    fn from(p: RawProject) -> Self {
        let website_url = format!(
            "https://modrinth.com/{}/{}",
            kind_path(&p.project_type),
            p.slug
        );
        Self {
            id: p.id,
            kind: p.project_type,
            source: "modrinth".into(),
            slug: p.slug,
            title: p.title,
            description: p.description,
            author: String::new(),
            categories: p.categories,
            downloads: p.downloads,
            follows: p.followers,
            icon_url: p.icon_url,
            game_versions: p.game_versions,
            loaders: p.loaders,
            license: p
                .license
                .map(|l| if l.name.is_empty() { l.id } else { l.name }),
            source_url: p.source_url,
            issues_url: p.issues_url,
            wiki_url: p.wiki_url,
            website_url,
            published: p.published,
            updated: p.updated,
        }
    }
}

fn kind_path(kind: &str) -> &'static str {
    match kind {
        "modpack" => "modpack",
        "shader" => "shader",
        "resourcepack" => "resourcepack",
        _ => "mod",
    }
}

#[derive(Debug, Deserialize, Default)]
struct RawVersion {
    #[serde(default)]
    id: String,
    #[serde(default)]
    project_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version_number: String,
    #[serde(default)]
    version_type: String,
    #[serde(default)]
    date_published: Option<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    files: Vec<RawFile>,
    #[serde(default)]
    dependencies: Vec<RawDependency>,
}

#[derive(Debug, Deserialize, Default)]
struct RawFile {
    #[serde(default)]
    filename: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    file_type: Option<String>,
    #[serde(default)]
    hashes: RawHashes,
}

#[derive(Debug, Deserialize, Default)]
struct RawHashes {
    #[serde(default)]
    sha1: Option<String>,
    #[serde(default)]
    sha512: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawDependency {
    #[serde(default)]
    version_id: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    dependency_type: String,
}

impl From<RawVersion> for ResourceVersion {
    fn from(v: RawVersion) -> Self {
        Self {
            id: v.id,
            project_id: v.project_id,
            name: v.name,
            version_number: v.version_number,
            version_type: if v.version_type.is_empty() {
                "release".into()
            } else {
                v.version_type
            },
            date_published: v.date_published,
            downloads: v.downloads,
            game_versions: v.game_versions,
            loaders: v.loaders,
            files: v
                .files
                .into_iter()
                .map(|f| ResourceFile {
                    filename: f.filename,
                    url: f.url,
                    size: f.size,
                    primary: f.primary,
                    sha1: f.hashes.sha1,
                    sha512: f.hashes.sha512,
                    file_type: f.file_type,
                })
                .collect(),
            dependencies: v
                .dependencies
                .into_iter()
                .map(|d| ResourceDependency {
                    version_id: d.version_id,
                    project_id: d.project_id,
                    dependency_type: d.dependency_type,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_resource_kinds_and_tokens() {
        assert!(normalize_kind("mod").is_ok());
        assert!(normalize_kind("modpack").is_ok());
        assert!(normalize_kind("world").is_err());
        assert!(validate_id("AANobbMI").is_ok());
        assert!(validate_id("../evil").is_err());
        assert!(validate_id("a/b").is_err());
        assert!(valid_token("1.21.1"));
        assert!(!valid_token("1.21.1 "));
    }

    #[test]
    fn maps_website_urls() {
        let p: RawProject = serde_json::from_value(serde_json::json!({
            "id": "x", "project_type": "modpack", "slug": "adrenaslin",
            "title": "t", "description": "d", "categories": [], "downloads": 1,
            "followers": 2, "game_versions": [], "loaders": []
        }))
        .unwrap();
        let out: ResourceProject = p.into();
        assert_eq!(out.website_url, "https://modrinth.com/modpack/adrenaslin");
    }

    #[test]
    fn parses_search_hit_shapes() {
        let page: RawSearchPage = serde_json::from_value(serde_json::json!({
            "hits": [{
                "project_id": "AANobbMI", "project_type": "mod", "slug": "sodium",
                "title": "Sodium", "description": "fast", "author": "jellysquid3",
                "categories": ["fabric", "optimization"], "versions": ["1.21.1"],
                "downloads": 100, "follows": 10, "icon_url": null,
                "date_modified": "2026-01-01T00:00:00Z", "latest_version": "v1"
            }],
            "offset": 0, "limit": 20, "total_hits": 1
        }))
        .unwrap();
        let out: Vec<ResourceHit> = page.hits.into_iter().map(Into::into).collect();
        assert_eq!(out[0].id, "AANobbMI");
        assert_eq!(out[0].kind, "mod");
        assert_eq!(out[0].categories.len(), 2);
    }
}
