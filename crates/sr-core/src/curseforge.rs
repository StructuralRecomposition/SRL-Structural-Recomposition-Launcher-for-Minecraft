//! Minimal CurseForge API client.
//!
//! A shared public API key ships with the launcher so every user can search
//! CurseForge without applying for their own key. Users who want an independent
//! quota can still save a personal key in the OS credential store; that key
//! overrides the bundled one.

use crate::account::SecretStore;
use crate::error::{AppError, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

const API: &str = "https://api.curseforge.com/v1";
const KEY_ENTRY: &str = "curseforge:api_key";
const GAME_ID: u64 = 432;

/// Public key bundled with the launcher (shared configuration).
pub const PUBLIC_API_KEY: &str = "$2a$10$ZWcN506DaY8yzfcEY6GinuNn1uMxkLZkgyPd5Zmg/hIYUj/3qBBaS";

pub const CLASS_ID_MODS: u64 = 6;
pub const CLASS_ID_MODPACKS: u64 = 4471;
pub const CLASS_ID_RESOURCEPACKS: u64 = 12;
pub const CLASS_ID_SHADERS: u64 = 6552;
pub const CLASS_ID_WORLDS: u64 = 17;

pub fn class_id_for_kind(kind: &str) -> Result<u64> {
    match kind {
        "mod" => Ok(CLASS_ID_MODS),
        "modpack" => Ok(CLASS_ID_MODPACKS),
        "resourcepack" => Ok(CLASS_ID_RESOURCEPACKS),
        "shader" => Ok(CLASS_ID_SHADERS),
        "map" => Ok(CLASS_ID_WORLDS),
        _ => Err(AppError::invalid_input("不支持的 CurseForge 资源类型")),
    }
}

pub fn mod_loader_type(loader: &str) -> Option<u32> {
    match loader.trim().to_ascii_lowercase().as_str() {
        "forge" => Some(1),
        "fabric" => Some(4),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    }
}

/// CurseForge stamps loader/side tokens into `gameVersions`. Split them out so
/// compatibility checks only see real Minecraft versions.
pub fn split_versions(values: &[String]) -> (Vec<String>, Vec<String>) {
    let mut versions = Vec::new();
    let mut loaders = Vec::new();
    for value in values {
        let lower = value.to_ascii_lowercase();
        let is_loader = matches!(
            lower.as_str(),
            "forge"
                | "neoforge"
                | "fabric"
                | "quilt"
                | "rift"
                | "liteloader"
                | "iris"
                | "optifine"
                | "client"
                | "server"
                | "datapack"
        );
        if is_loader {
            if !matches!(lower.as_str(), "client" | "server" | "datapack") {
                loaders.push(lower);
            }
        } else {
            versions.push(value.clone());
        }
    }
    loaders.sort();
    loaders.dedup();
    (versions, loaders)
}

pub fn load_key(store: &dyn SecretStore) -> Result<Option<String>> {
    Ok(store
        .get(KEY_ENTRY)?
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty()))
}

/// Personal key if configured, otherwise the bundled public key.
pub fn effective_key(store: &dyn SecretStore) -> Result<String> {
    Ok(load_key(store)?.unwrap_or_else(|| PUBLIC_API_KEY.to_string()))
}

/// Built-in key only. Callers that want zero keychain access should use this.
pub fn builtin_key() -> &'static str {
    PUBLIC_API_KEY
}

/// `"custom"` when the user saved a personal key, `"builtin"` otherwise.
pub fn key_source(store: &dyn SecretStore) -> Result<&'static str> {
    Ok(if load_key(store)?.is_some() {
        "custom"
    } else {
        "builtin"
    })
}

pub fn save_key(store: &dyn SecretStore, key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() || key.len() > 256 || key.chars().any(|c| c.is_control()) {
        return Err(AppError::invalid_input("CurseForge API key 格式非法"));
    }
    store.set(KEY_ENTRY, key)
}

pub fn clear_key(store: &dyn SecretStore) -> Result<()> {
    store.delete(KEY_ENTRY)
}

/// CurseForge file fingerprint: MurmurHash2 (seed `1`) over the file with ASCII
/// whitespace bytes `0x09`, `0x0A`, `0x0D` and `0x20` removed.
///
/// CurseForge's exact parameters are a length seed of `1`, a final multiply for
/// the last partial word, and the usual `>> 13` / `>> 15` finalisation.
pub fn fingerprint_file(path: &Path) -> Result<u32> {
    const M: u32 = 0x5b_d1_e9_95;
    const R: u32 = 24;
    let mut file = std::fs::File::open(path)?;
    let mut buffer = [0u8; 64 * 1024];

    // Pass 1: count bytes after whitespace removal (the hash length seed).
    let mut filtered_len: u64 = 0;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        filtered_len += buffer[..read]
            .iter()
            .filter(|b| !is_fingerprint_whitespace(**b))
            .count() as u64;
    }
    if filtered_len > u32::MAX as u64 {
        return Err(AppError::invalid_input(
            "文件超过 4GB，无法生成 CurseForge 指纹",
        ));
    }

    // Pass 2: hash the filtered stream.
    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(0))?;
    let mut h = 1u32 ^ filtered_len as u32;
    let mut word = [0u8; 4];
    let mut word_len = 0usize;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        for &byte in &buffer[..read] {
            if is_fingerprint_whitespace(byte) {
                continue;
            }
            word[word_len] = byte;
            word_len += 1;
            if word_len == 4 {
                let mut k = u32::from_le_bytes(word);
                k = k.wrapping_mul(M);
                k ^= k >> R;
                k = k.wrapping_mul(M);
                h = h.wrapping_mul(M);
                h ^= k;
                word_len = 0;
            }
        }
    }
    if word_len >= 3 {
        h ^= (word[2] as u32) << 16;
    }
    if word_len >= 2 {
        h ^= (word[1] as u32) << 8;
    }
    if word_len >= 1 {
        h ^= word[0] as u32;
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    Ok(h)
}

fn is_fingerprint_whitespace(byte: u8) -> bool {
    matches!(byte, 0x09 | 0x0a | 0x0d | 0x20)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeFile {
    pub id: u64,
    pub mod_id: u64,
    pub file_name: String,
    pub download_url: String,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub size: u64,
    pub game_versions: Vec<String>,
    pub file_date: Option<String>,
    pub release_type: u32,
    pub dependencies: Vec<CurseForgeDependency>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeDependency {
    pub mod_id: u64,
    pub relation_type: u32,
}

/// CurseForge relationType values as documented by the official REST API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CurseForgeRelation {
    EmbeddedLibrary,
    OptionalDependency,
    RequiredDependency,
    Tool,
    Incompatible,
    Include,
    Unknown,
}

pub fn curseforge_relation(relation_type: u32) -> CurseForgeRelation {
    match relation_type {
        1 => CurseForgeRelation::EmbeddedLibrary,
        2 => CurseForgeRelation::OptionalDependency,
        3 => CurseForgeRelation::RequiredDependency,
        4 => CurseForgeRelation::Tool,
        5 => CurseForgeRelation::Incompatible,
        6 => CurseForgeRelation::Include,
        _ => CurseForgeRelation::Unknown,
    }
}

#[derive(Debug, Deserialize, Default)]
struct CfFingerprintResponse {
    #[serde(default)]
    data: CfFingerprintData,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CfFingerprintData {
    #[serde(default)]
    exact_matches: Vec<CfFingerprintEntry>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CfFingerprintEntry {
    #[serde(default)]
    file: CfFingerprintFile,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CfFingerprintFile {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    mod_id: u64,
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    file_fingerprint: u32,
    #[serde(default)]
    is_available: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeMatch {
    pub project_id: u64,
    pub file_id: u64,
    pub file_name: String,
    pub fingerprint: u32,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeCategory {
    pub id: u64,
    pub name: String,
    pub parent_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeMod {
    pub id: u64,
    pub name: String,
    pub slug: String,
    pub summary: String,
    pub download_count: u64,
    pub logo_url: Option<String>,
    pub category_names: Vec<String>,
    pub category_ids: Vec<u64>,
    pub date_modified: Option<String>,
    pub external_url: Option<String>,
    pub author: Option<String>,
    pub latest_file: Option<CurseForgeFile>,
    pub files: Vec<CurseForgeFile>,
}

#[derive(Debug, Clone)]
pub struct CurseForgeSearchPage {
    pub mods: Vec<CurseForgeMod>,
    pub total: u32,
    pub offset: u32,
    pub limit: u32,
}

pub struct CurseForgeService {
    http: reqwest::Client,
    key: String,
}

impl CurseForgeService {
    pub fn new(http: reqwest::Client, key: impl Into<String>) -> Self {
        Self {
            http,
            key: key.into(),
        }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: reqwest::Url) -> Result<T> {
        let resp = self
            .http
            .get(url)
            .timeout(Duration::from_secs(25))
            .header("x-api-key", &self.key)
            .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
            .send()
            .await?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::new(
                ErrorCode::AuthenticationRequired,
                "CurseForge API key 无效或无权限",
            ));
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::not_found("CurseForge 资源不存在"));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::new(ErrorCode::Http, "CurseForge 请求过于频繁").retryable(true));
        }
        if !status.is_success() {
            return Err(AppError::new(
                ErrorCode::Http,
                format!("CurseForge 返回 HTTP {}", status.as_u16()),
            ));
        }
        let bytes = resp.bytes().await?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                "CurseForge 返回数据异常过大",
            ));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            AppError::new(ErrorCode::InvalidInput, "CurseForge 返回数据格式无效")
                .with_detail(e.to_string())
        })
    }

    pub async fn test(&self) -> Result<()> {
        let url = reqwest::Url::parse(&format!("{API}/games/{GAME_ID}"))
            .map_err(|e| AppError::internal(format!("CurseForge URL 无效：{e}")))?;
        let _: serde_json::Value = self.get_json(url).await?;
        Ok(())
    }

    /// Resolve local mod fingerprints to exact CurseForge project/file IDs.
    /// Unknown hashes are simply absent from the returned map.
    pub async fn fingerprint_matches(
        &self,
        fingerprints: &[u32],
    ) -> Result<HashMap<u32, CurseForgeMatch>> {
        if fingerprints.is_empty() {
            return Ok(HashMap::new());
        }
        let url = reqwest::Url::parse(&format!("{API}/fingerprints"))
            .map_err(|e| AppError::internal(format!("CurseForge URL 无效：{e}")))?;
        let response = self
            .http
            .post(url)
            .timeout(Duration::from_secs(25))
            .header("x-api-key", &self.key)
            .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
            .json(&serde_json::json!({ "fingerprints": fingerprints }))
            .send()
            .await?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::new(
                ErrorCode::AuthenticationRequired,
                "CurseForge API key 无效或无权限",
            ));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::new(ErrorCode::Http, "CurseForge 请求过于频繁").retryable(true));
        }
        if !status.is_success() {
            return Err(AppError::new(
                ErrorCode::Http,
                format!("CurseForge 返回 HTTP {}", status.as_u16()),
            ));
        }
        let bytes = response.bytes().await?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                "CurseForge 指纹返回数据异常过大",
            ));
        }
        let raw: CfFingerprintResponse = serde_json::from_slice(&bytes).map_err(|e| {
            AppError::new(ErrorCode::InvalidInput, "CurseForge 指纹数据格式无效")
                .with_detail(e.to_string())
        })?;
        let mut out = HashMap::new();
        for entry in raw.data.exact_matches {
            let file = entry.file;
            if file.mod_id == 0 || file.id == 0 || file.file_fingerprint == 0 {
                continue;
            }
            out.insert(
                file.file_fingerprint,
                CurseForgeMatch {
                    project_id: file.mod_id,
                    file_id: file.id,
                    file_name: file.file_name,
                    fingerprint: file.file_fingerprint,
                    available: file.is_available,
                },
            );
        }
        Ok(out)
    }

    pub async fn categories(&self, class_id: u64) -> Result<Vec<CurseForgeCategory>> {
        let url = reqwest::Url::parse(&format!(
            "{API}/categories?gameId={GAME_ID}&classId={class_id}"
        ))
        .map_err(|e| AppError::internal(format!("CurseForge URL 无效：{e}")))?;
        let raw: CfCategoriesResponse = self.get_json(url).await?;
        Ok(raw
            .data
            .into_iter()
            .map(|c| CurseForgeCategory {
                id: c.id,
                name: c.name,
                parent_id: c.parent_category_id,
            })
            .collect())
    }

    /// Search any CurseForge Minecraft class (mods, modpacks, resource packs,
    /// shaders, worlds).
    #[allow(clippy::too_many_arguments)]
    pub async fn search(
        &self,
        class_id: u64,
        query: &str,
        game_version: Option<&str>,
        category_id: Option<u64>,
        mod_loader_type: Option<u32>,
        sort_field: u32,
        offset: u32,
        limit: u32,
    ) -> Result<CurseForgeSearchPage> {
        let mut url = reqwest::Url::parse(&format!("{API}/mods/search"))
            .map_err(|e| AppError::internal(format!("CurseForge URL 无效：{e}")))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("gameId", &GAME_ID.to_string());
            pairs.append_pair("classId", &class_id.to_string());
            pairs.append_pair("pageSize", &limit.clamp(1, 50).to_string());
            pairs.append_pair("index", &offset.to_string());
            pairs.append_pair("sortField", &sort_field.to_string());
            pairs.append_pair("sortOrder", "desc");
            if !query.trim().is_empty() {
                pairs.append_pair("searchFilter", query.trim());
            }
            if let Some(version) = game_version.filter(|v| valid_token(v)) {
                pairs.append_pair("gameVersion", version);
            }
            if let Some(id) = category_id {
                pairs.append_pair("categoryId", &id.to_string());
            }
            if let Some(loader) = mod_loader_type {
                pairs.append_pair("modLoaderType", &loader.to_string());
            }
        }
        let raw: CfSearchResponse = self.get_json(url).await?;
        let total = raw.pagination.total_count;
        let offset = raw.pagination.index;
        let limit = raw.pagination.page_size;
        let mut mods = Vec::new();
        for m in raw.data {
            mods.push(map_mod(m, self).await?);
        }
        Ok(CurseForgeSearchPage {
            mods,
            total,
            offset,
            limit,
        })
    }

    pub async fn project(&self, id: u64) -> Result<CurseForgeMod> {
        let url = reqwest::Url::parse(&format!("{API}/mods/{id}"))
            .map_err(|e| AppError::internal(format!("CurseForge URL 无效：{e}")))?;
        let raw: CfModResponse = self.get_json(url).await?;
        map_mod(raw.data, self).await
    }

    pub async fn files(
        &self,
        mod_id: u64,
        game_version: Option<&str>,
        mod_loader_type: Option<u32>,
    ) -> Result<Vec<CurseForgeFile>> {
        let mut url = reqwest::Url::parse(&format!("{API}/mods/{mod_id}/files"))
            .map_err(|e| AppError::internal(format!("CurseForge URL 无效：{e}")))?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("pageSize", "50");
            if let Some(version) = game_version.filter(|v| valid_token(v)) {
                pairs.append_pair("gameVersion", version);
            }
            if let Some(loader) = mod_loader_type {
                pairs.append_pair("modLoaderType", &loader.to_string());
            }
        }
        let raw: CfFilesResponse = self.get_json(url).await?;
        let mut files = Vec::new();
        for f in raw.data {
            files.push(map_file(f, mod_id, self).await?);
        }
        Ok(files)
    }

    pub async fn file(&self, mod_id: u64, file_id: u64) -> Result<CurseForgeFile> {
        let url = reqwest::Url::parse(&format!("{API}/mods/{mod_id}/files/{file_id}"))
            .map_err(|e| AppError::internal(format!("CurseForge URL 无效：{e}")))?;
        let raw: CfFileResponse = self.get_json(url).await?;
        map_file(raw.data, mod_id, self).await
    }

    async fn resolve_download_url(
        &self,
        mod_id: u64,
        file_id: u64,
        raw: &CfFileData,
    ) -> Result<String> {
        if let Some(url) = raw.download_url.as_deref().filter(|u| !u.trim().is_empty()) {
            return Ok(url.to_string());
        }
        let url = reqwest::Url::parse(&format!("{API}/mods/{mod_id}/files/{file_id}/download-url"))
            .map_err(|e| AppError::internal(format!("CurseForge URL 无效：{e}")))?;
        if let Ok(resolved) = self.get_json::<CfUrlResponse>(url).await {
            if !resolved.data.trim().is_empty() {
                return Ok(resolved.data);
            }
        }
        let base = format!(
            "https://mediafilez.forgecdn.net/files/{}/{}",
            file_id / 1000,
            file_id % 1000
        );
        let mut url = reqwest::Url::parse(&base)
            .unwrap_or_else(|_| reqwest::Url::parse("https://mediafilez.forgecdn.net/").unwrap());
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.push(&raw.file_name);
        }
        Ok(url.to_string())
    }
}

async fn map_mod(raw: CfModData, service: &CurseForgeService) -> Result<CurseForgeMod> {
    let mut files = Vec::new();
    for f in raw.latest_files {
        files.push(map_file(f, raw.id, service).await?);
    }
    let latest_file = files.first().cloned();
    Ok(CurseForgeMod {
        id: raw.id,
        name: raw.name,
        slug: raw.slug,
        summary: raw.summary,
        download_count: raw.download_count,
        logo_url: raw.logo.and_then(|l| l.url),
        category_names: raw.categories.iter().map(|c| c.name.clone()).collect(),
        category_ids: raw.categories.iter().map(|c| c.id).collect(),
        date_modified: raw.date_modified,
        external_url: raw.links.and_then(|l| l.website_url),
        author: raw.authors.first().map(|a| a.name.clone()),
        latest_file,
        files,
    })
}

async fn map_file(
    raw: CfFileData,
    mod_id: u64,
    service: &CurseForgeService,
) -> Result<CurseForgeFile> {
    let url = service.resolve_download_url(mod_id, raw.id, &raw).await?;
    let sha1 = raw
        .hashes
        .iter()
        .find(|h| h.algo == 1)
        .map(|h| h.value.clone());
    let sha512 = raw
        .hashes
        .iter()
        .find(|h| h.algo == 2)
        .map(|h| h.value.clone());
    Ok(CurseForgeFile {
        id: raw.id,
        mod_id,
        file_name: raw.file_name,
        download_url: url,
        sha1,
        sha512,
        size: raw.file_length,
        game_versions: raw.game_versions,
        file_date: raw.file_date,
        release_type: raw.release_type,
        dependencies: raw
            .dependencies
            .into_iter()
            .map(|dep| CurseForgeDependency {
                mod_id: dep.mod_id,
                relation_type: dep.relation_type,
            })
            .collect(),
    })
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

// ---------------------------------------------------------------------------
// Raw API shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CfSearchResponse {
    #[serde(default)]
    data: Vec<CfModData>,
    #[serde(default)]
    pagination: CfPagination,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CfPagination {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    page_size: u32,
    #[serde(default)]
    total_count: u32,
}

#[derive(Debug, Deserialize)]
struct CfModResponse {
    data: CfModData,
}

#[derive(Debug, Deserialize)]
struct CfFilesResponse {
    #[serde(default)]
    data: Vec<CfFileData>,
}

#[derive(Debug, Deserialize)]
struct CfFileResponse {
    data: CfFileData,
}

#[derive(Debug, Deserialize)]
struct CfCategoriesResponse {
    #[serde(default)]
    data: Vec<CfCategoryData>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CfModData {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    download_count: u64,
    #[serde(default)]
    logo: Option<CfLogo>,
    #[serde(default)]
    categories: Vec<CfCategoryData>,
    #[serde(default)]
    latest_files: Vec<CfFileData>,
    #[serde(default)]
    date_modified: Option<String>,
    #[serde(default)]
    links: Option<CfLinks>,
    #[serde(default)]
    authors: Vec<CfAuthor>,
}

#[derive(Debug, Deserialize, Default)]
struct CfLogo {
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CfLinks {
    #[serde(default)]
    website_url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CfAuthor {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CfCategoryData {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    parent_category_id: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CfFileData {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    file_length: u64,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    hashes: Vec<CfHash>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    file_date: Option<String>,
    #[serde(default)]
    release_type: u32,
    #[serde(default)]
    dependencies: Vec<CfFileDependencyData>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CfFileDependencyData {
    #[serde(default)]
    mod_id: u64,
    #[serde(default)]
    relation_type: u32,
}

#[derive(Debug, Deserialize)]
struct CfHash {
    #[serde(default)]
    value: String,
    #[serde(default)]
    algo: u32,
}

#[derive(Debug, Deserialize)]
struct CfUrlResponse {
    data: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::MemorySecretStore;

    #[test]
    fn key_lifecycle_uses_secret_store() {
        let store = MemorySecretStore::default();
        assert!(load_key(&store).unwrap().is_none());
        save_key(&store, "  abc123  ").unwrap();
        assert_eq!(load_key(&store).unwrap().as_deref(), Some("abc123"));
        clear_key(&store).unwrap();
        assert!(load_key(&store).unwrap().is_none());
        assert!(save_key(&store, "").is_err());
    }

    #[test]
    #[ignore = "requires a real CurseForge jar in SRL_TEST_JAR"]
    fn fingerprint_matches_known_curseforge_file() {
        let path = std::env::var("SRL_TEST_JAR").unwrap_or_else(|_| "/tmp/geckolib.jar".into());
        let path = std::path::Path::new(&path);
        if !path.is_file() {
            return;
        }
        // GeckoLib file 8945638 (geckolib-neoforge-26.3-5.5.7.jar).
        assert_eq!(fingerprint_file(path).unwrap(), 1_550_434_642);
    }

    #[test]
    fn bundled_key_is_available_without_user_config() {
        let store = MemorySecretStore::default();
        assert_eq!(effective_key(&store).unwrap(), PUBLIC_API_KEY);
        assert_eq!(key_source(&store).unwrap(), "builtin");
        save_key(&store, "personal-key").unwrap();
        assert_eq!(effective_key(&store).unwrap(), "personal-key");
        assert_eq!(key_source(&store).unwrap(), "custom");
    }

    #[test]
    fn maps_kinds_to_class_ids() {
        assert_eq!(class_id_for_kind("mod").unwrap(), 6);
        assert_eq!(class_id_for_kind("modpack").unwrap(), 4471);
        assert_eq!(class_id_for_kind("resourcepack").unwrap(), 12);
        assert_eq!(class_id_for_kind("shader").unwrap(), 6552);
        assert_eq!(class_id_for_kind("map").unwrap(), 17);
        assert!(class_id_for_kind("unknown").is_err());
    }

    #[tokio::test]
    #[ignore = "requires network access to api.curseforge.com"]
    async fn searches_curseforge_live() {
        let service = CurseForgeService::new(reqwest::Client::new(), PUBLIC_API_KEY);
        service.test().await.unwrap();
        let page = service
            .search(
                CLASS_ID_MODS,
                "",
                Some("1.21.1"),
                Some(412),
                Some(6),
                6,
                0,
                5,
            )
            .await
            .unwrap();
        assert!(page.total > 0);
        assert!(!page.mods.is_empty());
        assert!(page.mods[0].latest_file.is_some());
    }

    #[test]
    fn splits_loader_tokens_from_game_versions() {
        let (versions, loaders) = split_versions(
            &["1.21.1", "Client", "NeoForge", "1.21", "Server"]
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>(),
        );
        assert_eq!(versions, vec!["1.21.1", "1.21"]);
        assert_eq!(loaders, vec!["neoforge"]);
        assert_eq!(mod_loader_type("NeoForge"), Some(6));
        assert_eq!(mod_loader_type("unknown"), None);
    }

    #[test]
    fn maps_file_json() {
        let raw: CfFileData = serde_json::from_value(serde_json::json!({
            "id": 9190157,
            "fileName": "All the Mods 10-8.3.zip",
            "fileLength": 200037954,
            "downloadUrl": "https://edge.forgecdn.net/files/9190/157/All%20the%20Mods%2010-8.3.zip",
            "hashes": [{"value":"1379421131ced12a210e2765bb5896d7ecc074c9","algo":1}],
            "gameVersions": ["1.21.1"],
            "fileDate": "2026-10-15T11:33:19.793Z",
            "releaseType": 1
        }))
        .unwrap();
        assert_eq!(raw.id, 9190157);
        assert_eq!(raw.hashes.len(), 1);
        assert_eq!(raw.game_versions, vec!["1.21.1"]);
    }
}
