pub mod args;
pub mod model;
pub mod resolve;
pub mod rules;

pub use args::{
    build_plan, classpath_separator, tokenize_legacy_arguments, LaunchOptions, LaunchPlan,
    PlayerProfile,
};
pub use model::*;
pub use resolve::{merge_parent, resolve_inheritance, EffectiveLibrary};
pub use rules::{OsName, RuleContext};

use crate::download::ensure_trusted_url;
use crate::error::{AppError, ErrorCode, Result};
use crate::paths::{AppPaths, GamePaths};
use crate::util;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Duration;

pub const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const MANIFEST_TTL: Duration = Duration::from_secs(6 * 60 * 60);
pub const MAX_METADATA_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ManifestSource {
    Network,
    Cache,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestResult {
    pub manifest: VersionManifest,
    pub source: ManifestSource,
    /// Non-fatal notice, e.g. "network failed, using cached manifest".
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledVersion {
    pub id: String,
    pub version_type: Option<String>,
    pub has_json: bool,
    pub has_jar: bool,
    pub java_major: Option<u32>,
    pub main_class: Option<String>,
    /// Recursive size of `versions/<id>` in bytes.
    pub size_bytes: u64,
    pub modified_at: Option<String>,
    pub inherits_from: Option<String>,
    /// UI-friendly loader label derived from the id / inherited metadata.
    pub loader: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionDeletePlan {
    pub id: String,
    /// Versions whose `inheritsFrom` points at this version (transitive).
    pub dependents: Vec<String>,
    /// Instances currently launching this version or any dependent.
    pub instances: Vec<String>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionDeleteResult {
    pub deleted: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFile {
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub dest: PathBuf,
    pub label: String,
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct NativeExtract {
    pub archive: PathBuf,
    pub dest_dir: PathBuf,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub version: VersionJson,
    pub files: Vec<PlannedFile>,
    pub natives: Vec<NativeExtract>,
    pub total_bytes: u64,
    pub unknown_size_files: u32,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationIssue {
    pub path: String,
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub checked: u32,
    pub ok: bool,
    pub issues: Vec<VerificationIssue>,
    pub checked_assets: u32,
    /// False when the user installed without assets; asset objects were then
    /// not evaluated and must not be reported as corrupt.
    pub assets_checked: bool,
}

#[derive(Clone)]
pub struct VersionService {
    http: reqwest::Client,
}

impl VersionService {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    async fn get_bytes(&self, url: &str, allow_custom_hosts: bool) -> Result<Vec<u8>> {
        let parsed = ensure_trusted_url(url, allow_custom_hosts)?;
        let mut last_error: Option<AppError> = None;
        for attempt in 0..3u32 {
            let result = self
                .http
                .get(parsed.clone())
                .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
                .timeout(Duration::from_secs(25))
                .send()
                .await;
            let resp = match result {
                Ok(resp) => resp,
                Err(e) => {
                    let err = AppError::from(e);
                    if attempt < 2 && err.retryable {
                        last_error = Some(err);
                        tokio::time::sleep(Duration::from_millis(400 * (1 << attempt))).await;
                        continue;
                    }
                    return Err(err);
                }
            };
            let status = resp.status();
            if !status.is_success() {
                let err = AppError::new(
                    ErrorCode::Http,
                    format!("元数据服务器返回 HTTP {}", status.as_u16()),
                )
                .retryable(status.is_server_error() || status.as_u16() == 429);
                if attempt < 2 && err.retryable {
                    last_error = Some(err);
                    tokio::time::sleep(Duration::from_millis(400 * (1 << attempt))).await;
                    continue;
                }
                return Err(err);
            }
            if let Some(len) = resp.content_length() {
                if len as usize > MAX_METADATA_BYTES {
                    return Err(AppError::new(
                        ErrorCode::VersionInvalid,
                        "版本元数据异常过大，已拒绝",
                    ));
                }
            }
            let bytes = resp.bytes().await?;
            if bytes.len() > MAX_METADATA_BYTES {
                return Err(AppError::new(
                    ErrorCode::VersionInvalid,
                    "版本元数据异常过大，已拒绝",
                ));
            }
            return Ok(bytes.to_vec());
        }
        Err(last_error.unwrap_or_else(|| AppError::network("无法获取版本元数据")))
    }

    /// Fetch the official version manifest, using a 6h cache. Network failure
    /// falls back to any cached copy and reports a warning instead of blocking.
    pub async fn manifest(&self, paths: &AppPaths, force_refresh: bool) -> Result<ManifestResult> {
        let cached = tokio::fs::metadata(paths.manifest_cache()).await.ok();
        if !force_refresh {
            if let Some(meta) = &cached {
                if let Ok(modified) = meta.modified() {
                    if modified.elapsed().unwrap_or(Duration::MAX) < MANIFEST_TTL {
                        if let Ok(Some(manifest)) =
                            util::read_json::<VersionManifest>(&paths.manifest_cache()).await
                        {
                            return Ok(ManifestResult {
                                manifest,
                                source: ManifestSource::Cache,
                                warning: None,
                            });
                        }
                    }
                }
            }
        }

        match self.fetch_manifest_network().await {
            Ok((manifest, bytes)) => {
                util::atomic_write(&paths.manifest_cache(), &bytes).await?;
                Ok(ManifestResult {
                    manifest,
                    source: ManifestSource::Network,
                    warning: None,
                })
            }
            Err(e) => {
                if let Ok(Some(manifest)) =
                    util::read_json::<VersionManifest>(&paths.manifest_cache()).await
                {
                    Ok(ManifestResult {
                        manifest,
                        source: ManifestSource::Cache,
                        warning: Some(format!(
                            "无法获取最新版本列表，正在使用本地缓存（{}）",
                            e.message
                        )),
                    })
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn fetch_manifest_network(&self) -> Result<(VersionManifest, Vec<u8>)> {
        let bytes = self.get_bytes(MANIFEST_URL, false).await?;
        let manifest: VersionManifest = serde_json::from_slice(&bytes).map_err(|e| {
            AppError::new(ErrorCode::VersionInvalid, "版本列表格式无效").with_detail(e.to_string())
        })?;
        if manifest.versions.is_empty() {
            return Err(AppError::new(ErrorCode::VersionInvalid, "版本列表为空"));
        }
        Ok((manifest, bytes))
    }

    async fn fetch_manifest_entry(&self, entry: &ManifestVersion) -> Result<VersionJson> {
        let bytes = self.get_bytes(&entry.url, false).await?;
        if let Some(expected) = &entry.sha1 {
            let actual = util::sha1_bytes(&bytes);
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(AppError::checksum(expected, &actual));
            }
        }
        VersionJson::parse(&bytes)
    }

    /// Fetch and fully resolve a version chain from Mojang metadata.
    pub async fn resolve_remote(&self, paths: &AppPaths, id: &str) -> Result<VersionJson> {
        let manifest = self.manifest(paths, false).await?.manifest;
        let entry = manifest
            .versions
            .iter()
            .find(|v| v.id == id)
            .ok_or_else(|| AppError::not_found(format!("版本 {id} 不在官方版本列表中")))?
            .clone();

        let mut chain: Vec<VersionJson> = Vec::new();
        let mut current = entry;
        let mut seen = std::collections::HashSet::new();
        loop {
            if !seen.insert(current.id.clone()) {
                return Err(AppError::new(ErrorCode::VersionInvalid, "版本继承出现循环"));
            }
            let json = self.fetch_manifest_entry(&current).await?;
            let next_id = json.inherits_from.clone();
            chain.push(json);
            match next_id {
                Some(pid) => {
                    current = manifest
                        .versions
                        .iter()
                        .find(|v| v.id == pid)
                        .ok_or_else(|| {
                            AppError::new(
                                ErrorCode::VersionInvalid,
                                format!("父版本 {pid} 不在官方版本列表中"),
                            )
                        })?
                        .clone();
                }
                None => break,
            }
            if chain.len() > resolve::MAX_INHERITANCE_DEPTH {
                return Err(AppError::new(ErrorCode::VersionInvalid, "版本继承层级过深"));
            }
        }

        let child = chain.remove(0);
        let map: std::collections::HashMap<String, VersionJson> =
            chain.into_iter().map(|v| (v.id.clone(), v)).collect();
        resolve_inheritance(child, |pid| {
            map.get(pid).cloned().ok_or_else(|| {
                AppError::new(ErrorCode::VersionInvalid, format!("缺少父版本 {pid}"))
            })
        })
    }

    pub async fn load_installed(&self, game: &GamePaths, id: &str) -> Result<VersionJson> {
        let path = game.version_json(id);
        let bytes = tokio::fs::read(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!("本地未安装版本 {id}"))
            } else {
                e.into()
            }
        })?;
        VersionJson::parse(&bytes)
    }

    /// Download and verify the asset index, returning its parsed content.
    pub async fn ensure_asset_index(
        &self,
        game: &GamePaths,
        resolved: &VersionJson,
        allow_custom_hosts: bool,
    ) -> Result<AssetIndexFile> {
        let index_ref = resolved.asset_index()?;
        let dest = game.indexes_dir().join(format!("{}.json", index_ref.id));
        if tokio::fs::try_exists(&dest).await.unwrap_or(false) {
            if let Some(expected) = &index_ref.sha1 {
                let actual = util::sha1_file(&dest).await?;
                if actual.eq_ignore_ascii_case(expected) {
                    let bytes = tokio::fs::read(&dest).await?;
                    return parse_index(&bytes);
                }
            } else {
                let bytes = tokio::fs::read(&dest).await?;
                return parse_index(&bytes);
            }
        }
        let bytes = self.get_bytes(&index_ref.url, allow_custom_hosts).await?;
        if let Some(expected) = &index_ref.sha1 {
            let actual = util::sha1_bytes(&bytes);
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(AppError::checksum(expected, &actual));
            }
        }
        util::atomic_write(&dest, &bytes).await?;
        parse_index(&bytes)
    }

    /// Build the full download plan. `index` is required when `include_assets`.
    pub fn build_install_plan(
        game: &GamePaths,
        resolved: &VersionJson,
        index: Option<&AssetIndexFile>,
        include_assets: bool,
        ctx: &RuleContext,
    ) -> Result<InstallPlan> {
        resolved.validate_for_launch()?;
        let mut files = Vec::new();
        let mut natives = Vec::new();
        let mut warnings = Vec::new();

        let client = resolved.client_download()?;
        files.push(PlannedFile {
            url: client.url.clone(),
            sha1: Some(client.sha1.clone()),
            size: Some(client.size),
            dest: game.version_jar(&resolved.jar_id()),
            label: format!("客户端 {}.jar", resolved.jar_id()),
            kind: "client".into(),
        });

        for lib in resolved.effective_libraries(ctx) {
            let Some(url) = lib.url.clone() else {
                warnings.push(format!("库 {} 没有下载地址，已跳过", lib.name));
                continue;
            };
            let dest = util::safe_join(&game.libraries_dir(), &lib.path)?;
            if lib.is_native {
                natives.push(NativeExtract {
                    archive: dest.clone(),
                    dest_dir: game.natives_dir(&resolved.id),
                    exclude: lib.extract_exclude.clone(),
                });
            }
            files.push(PlannedFile {
                url,
                sha1: lib.sha1.clone(),
                size: lib.size,
                dest,
                label: lib.name.clone(),
                kind: if lib.is_native { "native" } else { "library" }.into(),
            });
        }

        if include_assets {
            let index = index.ok_or_else(|| AppError::internal("构建安装计划时缺少资源索引"))?;
            let objects = game.objects_dir();
            let mut seen_hashes = std::collections::HashSet::new();
            for (name, obj) in &index.objects {
                if !is_sha1(&obj.hash) {
                    return Err(AppError::new(
                        ErrorCode::VersionInvalid,
                        format!("资源 {name} 的哈希格式无效"),
                    ));
                }
                if !seen_hashes.insert(obj.hash.clone()) {
                    continue;
                }
                let prefix = &obj.hash[..2];
                files.push(PlannedFile {
                    url: format!(
                        "https://resources.download.minecraft.net/{prefix}/{}",
                        obj.hash
                    ),
                    sha1: Some(obj.hash.clone()),
                    size: Some(obj.size),
                    dest: objects.join(prefix).join(&obj.hash),
                    label: name.clone(),
                    kind: "asset".into(),
                });
            }
        }

        if let Some(file) = resolved
            .logging()
            .and_then(|l| l.client.as_ref())
            .and_then(|c| c.file.as_ref())
        {
            let dest = game.root().join("log_configs").join(&file.sha1);
            files.push(PlannedFile {
                url: file.url.clone(),
                sha1: Some(file.sha1.clone()),
                size: Some(file.size),
                dest,
                label: "日志配置".into(),
                kind: "logging".into(),
            });
        }

        let total_bytes: u64 = files.iter().filter_map(|f| f.size).sum();
        let unknown_size_files = files.iter().filter(|f| f.size.is_none()).count() as u32;

        Ok(InstallPlan {
            version: resolved.clone(),
            files,
            natives,
            total_bytes,
            unknown_size_files,
            warnings,
        })
    }

    /// Install metadata + jar to disk. Called after assets are downloaded.
    pub async fn save_installed_metadata(
        &self,
        game: &GamePaths,
        resolved: &VersionJson,
    ) -> Result<()> {
        let dir = game.version_dir(&resolved.id);
        util::ensure_dir(&dir).await?;
        let bytes = serde_json::to_vec_pretty(resolved)?;
        util::atomic_write(&game.version_json(&resolved.id), &bytes).await
    }

    /// Enumerate locally installed version folders (JSON presence is required).
    ///
    /// Besides metadata, this reports the on-disk size and modification time so
    /// the UI can offer real "browse / configure / delete" actions without
    /// guessing.
    pub async fn installed_versions(&self, game: &GamePaths) -> Result<Vec<InstalledVersion>> {
        let dir = game.versions_dir();
        let mut out = Vec::new();
        let mut rd = match tokio::fs::read_dir(&dir).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = rd.next_entry().await? {
            let file_type = match entry.file_type().await {
                Ok(t) => t,
                Err(_) => continue,
            };
            if file_type.is_symlink() || !file_type.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            if util::safe_component(&id, "") != id {
                continue;
            }
            let json_path = entry.path().join(format!("{id}.json"));
            let has_json = tokio::fs::try_exists(&json_path).await.unwrap_or(false);
            if !has_json {
                continue;
            }
            let meta = match tokio::fs::read(&json_path).await {
                Ok(b) => VersionJson::parse(&b).ok(),
                Err(_) => None,
            };
            let jar_id = meta
                .as_ref()
                .map(|m| m.jar_id())
                .unwrap_or_else(|| id.clone());
            let has_jar = tokio::fs::try_exists(&game.version_jar(&jar_id))
                .await
                .unwrap_or(false);
            let inherits_from = meta
                .as_ref()
                .and_then(|m| m.inherits_from.clone())
                .or_else(|| {
                    meta.as_ref().and_then(|m| {
                        m.extra
                            .get("srBaseVersion")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                });
            let modified_at = match tokio::fs::metadata(entry.path()).await {
                Ok(m) => m
                    .modified()
                    .ok()
                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339()),
                Err(_) => None,
            };
            out.push(InstalledVersion {
                id: id.clone(),
                version_type: meta.as_ref().and_then(|m| m.version_type.clone()),
                has_json,
                has_jar,
                java_major: meta.as_ref().map(|m| m.java_major()),
                main_class: meta.as_ref().and_then(|m| m.main_class.clone()),
                size_bytes: dir_size(&entry.path()).await,
                modified_at,
                loader: loader_label(&id, inherits_from.as_deref()),
                inherits_from,
                path: entry.path().display().to_string(),
            });
        }
        out.sort_by(|a, b| {
            b.modified_at
                .cmp(&a.modified_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(out)
    }

    /// Plan a version deletion without touching the disk. The UI can show
    /// dependent loader versions and instances before asking for confirmation.
    pub async fn delete_plan(
        &self,
        game: &GamePaths,
        instances: &[crate::instance::Instance],
        id: &str,
    ) -> Result<VersionDeletePlan> {
        let (dependents, size_bytes) = self.dependents_and_size(game, id).await?;
        let mut affected = dependents.clone();
        affected.push(id.to_string());
        let instances = instances
            .iter()
            .filter(|i| affected.iter().any(|v| v == &i.version_id))
            .map(|i| format!("{} ({})", i.name, i.id))
            .collect();
        Ok(VersionDeletePlan {
            id: id.to_string(),
            dependents,
            instances,
            size_bytes,
        })
    }

    /// Delete a locally installed version directory.
    ///
    /// Safety rules:
    /// - the id must be a single safe path component and a direct child of
    ///   `versions/` that is not a symlink;
    /// - versions used by any instance are never deleted;
    /// - versions inherited by other loader versions require `force`, in which
    ///   case the dependents are removed too (they would be unusable anyway).
    pub async fn delete_installed(
        &self,
        game: &GamePaths,
        instances: &[crate::instance::Instance],
        id: &str,
        force: bool,
    ) -> Result<VersionDeleteResult> {
        let plan = self.delete_plan(game, instances, id).await?;
        if !plan.instances.is_empty() {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                format!(
                    "该版本正被以下实例使用，请先删除实例：{}",
                    plan.instances.join("、")
                ),
            ));
        }
        if !plan.dependents.is_empty() && !force {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                format!(
                    "该版本被以下加载器版本继承：{}。请选择同时删除后再试",
                    plan.dependents.join("、")
                ),
            ));
        }
        let mut deleted = plan.dependents.clone();
        deleted.push(id.to_string());
        for version_id in &deleted {
            remove_version_dir(game, version_id).await?;
        }
        Ok(VersionDeleteResult { deleted })
    }

    async fn dependents_and_size(&self, game: &GamePaths, id: &str) -> Result<(Vec<String>, u64)> {
        if util::safe_component(id, "") != id || id.is_empty() {
            return Err(AppError::invalid_input("版本 ID 非法"));
        }
        let target = game.version_dir(id);
        let meta = tokio::fs::symlink_metadata(&target).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found(format!("本地未安装版本 {id}"))
            } else {
                e.into()
            }
        })?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(AppError::path_unsafe("拒绝删除符号链接或非目录"));
        }

        // Build id -> inheritsFrom, then walk the graph from the target.
        // Loader profiles saved after inheritance resolution no longer carry
        // `inheritsFrom` (the merged file has no unresolved parent), so we also
        // infer the base game from well-known loader id suffixes.
        let mut scanned: Vec<(String, Option<String>)> = Vec::new();
        let mut rd = match tokio::fs::read_dir(game.versions_dir()).await {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
            Err(e) => return Err(e.into()),
        };
        while let Some(entry) = rd.next_entry().await? {
            if !entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let child_id = entry.file_name().to_string_lossy().into_owned();
            if util::safe_component(&child_id, "") != child_id {
                continue;
            }
            let path = entry.path().join(format!("{child_id}.json"));
            let parent = match tokio::fs::read(&path).await {
                Ok(bytes) => VersionJson::parse(&bytes).ok().and_then(|json| {
                    json.inherits_from.or_else(|| {
                        json.extra
                            .get("srBaseVersion")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                }),
                Err(_) => None,
            };
            scanned.push((child_id, parent));
        }
        let mut bases: Vec<String> = scanned
            .iter()
            .filter(|(child, _)| !is_loader_id(child))
            .map(|(child, _)| child.clone())
            .collect();
        bases.sort_by_key(|b| std::cmp::Reverse(b.len()));
        let mut parents = std::collections::HashMap::new();
        for (child, parent) in scanned {
            if let Some(parent) = parent {
                parents.insert(child, parent);
            } else if is_loader_id(&child) {
                if let Some(base) = bases
                    .iter()
                    .find(|base| child.ends_with(&format!("-{base}")))
                {
                    parents.insert(child, base.clone());
                }
            }
        }

        let mut dependents = Vec::new();
        let mut changed = true;
        while changed {
            changed = false;
            for (child, parent) in &parents {
                if dependents.iter().any(|d| d == child) {
                    continue;
                }
                if parent == id || dependents.iter().any(|d| d == parent) {
                    dependents.push(child.clone());
                    changed = true;
                }
            }
        }
        dependents.sort();
        Ok((dependents, dir_size(&target).await))
    }

    /// Flatten an installed loader version (Forge/NeoForge) against its base
    /// version and persist the resolved metadata. The resulting JSON has no
    /// `inheritsFrom`, all base libraries/downloads/assets merged in, a `jar`
    /// pointing at the base client jar, and `srBaseVersion` retained for the UI
    /// and deletion checks.
    pub async fn flatten_installed(&self, game: &GamePaths, id: &str) -> Result<VersionJson> {
        let child = self.load_installed(game, id).await?;
        let Some(base_id) = child.inherits_from.clone() else {
            return Ok(child);
        };
        let base = self.load_installed(game, &base_id).await?;
        let base_for_merge = base.clone();
        let mut resolved = resolve_inheritance(child, |parent| {
            if parent == base_id {
                Ok(base_for_merge.clone())
            } else {
                Err(AppError::not_found(format!("缺少父版本 {parent}")))
            }
        })?;
        if resolved.jar.is_none() {
            resolved.jar = Some(base_id.clone());
        }
        resolved.extra.insert(
            "srBaseVersion".to_string(),
            serde_json::Value::String(base_id),
        );
        self.save_installed_metadata(game, &resolved).await?;
        Ok(resolved)
    }

    /// Resolve the base Minecraft version behind a loader profile. Returns the
    /// explicit `inheritsFrom` when present, otherwise matches the loader id
    /// suffix against installed base versions.
    pub async fn base_version_of(&self, game: &GamePaths, id: &str) -> Result<Option<String>> {
        let installed = self.installed_versions(game).await?;
        if let Some(entry) = installed.iter().find(|v| v.id == id) {
            if let Some(parent) = &entry.inherits_from {
                return Ok(Some(parent.clone()));
            }
        }
        if let Some((base, _)) = id.rsplit_once("-forge-") {
            if installed.iter().any(|v| v.id == base) {
                return Ok(Some(base.to_string()));
            }
        }
        if let Ok(meta) = self.load_installed(game, id).await {
            if let Some(base) = meta.jar.clone() {
                if base != id && installed.iter().any(|v| v.id == base) {
                    return Ok(Some(base));
                }
            }
        }
        if !is_loader_id(id) {
            return Ok(None);
        }
        let mut bases: Vec<String> = installed
            .into_iter()
            .filter(|v| !is_loader_id(&v.id))
            .map(|v| v.id)
            .collect();
        bases.sort_by_key(|b| std::cmp::Reverse(b.len()));
        Ok(bases
            .into_iter()
            .find(|base| id.ends_with(&format!("-{base}"))))
    }

    /// Verify installed files. Checks the client jar, all effective libraries,
    /// and asset presence/size when requested.
    pub async fn verify_installed(
        &self,
        game: &GamePaths,
        resolved: &VersionJson,
        deep_assets: bool,
        check_assets: bool,
        ctx: &RuleContext,
    ) -> Result<VerificationReport> {
        let mut checked = 0u32;
        let mut issues = Vec::new();
        let mut checked_assets = 0u32;

        let jar = game.version_jar(&resolved.jar_id());
        checked += 1;
        if !tokio::fs::try_exists(&jar).await.unwrap_or(false) {
            issues.push(VerificationIssue {
                path: format!("versions/{}/{}.jar", resolved.jar_id(), resolved.jar_id()),
                kind: "client".into(),
                reason: "客户端 JAR 缺失".into(),
            });
        } else if let Ok(client) = resolved.client_download() {
            let actual = util::sha1_file(&jar).await?;
            if !actual.eq_ignore_ascii_case(&client.sha1) {
                issues.push(VerificationIssue {
                    path: format!("versions/{}/{}.jar", resolved.jar_id(), resolved.jar_id()),
                    kind: "client".into(),
                    reason: "客户端 JAR 校验失败".into(),
                });
            }
        }

        for lib in resolved.effective_libraries(ctx) {
            checked += 1;
            let dest = match util::safe_join(&game.libraries_dir(), &lib.path) {
                Ok(p) => p,
                Err(_) => {
                    issues.push(VerificationIssue {
                        path: lib.path.clone(),
                        kind: "library".into(),
                        reason: "库路径非法".into(),
                    });
                    continue;
                }
            };
            if !tokio::fs::try_exists(&dest).await.unwrap_or(false) {
                issues.push(VerificationIssue {
                    path: lib.path.clone(),
                    kind: "library".into(),
                    reason: "库文件缺失".into(),
                });
                continue;
            }
            if let Some(expected) = &lib.sha1 {
                let actual = util::sha1_file(&dest).await?;
                if !actual.eq_ignore_ascii_case(expected) {
                    issues.push(VerificationIssue {
                        path: lib.path.clone(),
                        kind: "library".into(),
                        reason: "库文件校验失败".into(),
                    });
                }
            }
        }

        let index_path = game
            .indexes_dir()
            .join(format!("{}.json", resolved.asset_index()?.id));
        checked += 1;
        if !tokio::fs::try_exists(&index_path).await.unwrap_or(false) {
            issues.push(VerificationIssue {
                path: format!("assets/indexes/{}.json", resolved.asset_index()?.id),
                kind: "asset_index".into(),
                reason: "资源索引缺失".into(),
            });
        } else if check_assets {
            if let Ok(bytes) = tokio::fs::read(&index_path).await {
                if let Ok(index) = parse_index(&bytes) {
                    for obj in index.objects.values() {
                        checked_assets += 1;
                        if !is_sha1(&obj.hash) {
                            continue;
                        }
                        let p = game.objects_dir().join(&obj.hash[..2]).join(&obj.hash);
                        match tokio::fs::metadata(&p).await {
                            Ok(m) if m.len() == obj.size => {
                                if deep_assets {
                                    let actual = util::sha1_file(&p).await?;
                                    if !actual.eq_ignore_ascii_case(&obj.hash) {
                                        issues.push(VerificationIssue {
                                            path: format!(
                                                "assets/objects/{}/{}",
                                                &obj.hash[..2],
                                                obj.hash
                                            ),
                                            kind: "asset".into(),
                                            reason: "资源文件校验失败".into(),
                                        });
                                    }
                                }
                            }
                            Ok(_) => issues.push(VerificationIssue {
                                path: format!("assets/objects/{}/{}", &obj.hash[..2], obj.hash),
                                kind: "asset".into(),
                                reason: "资源文件大小不符".into(),
                            }),
                            Err(_) => issues.push(VerificationIssue {
                                path: format!("assets/objects/{}/{}", &obj.hash[..2], obj.hash),
                                kind: "asset".into(),
                                reason: "资源文件缺失".into(),
                            }),
                        }
                    }
                }
            }
        }

        if issues.len() > 500 {
            issues.truncate(500);
        }
        Ok(VerificationReport {
            checked,
            ok: issues.is_empty(),
            issues,
            checked_assets,
            assets_checked: check_assets,
        })
    }
}

async fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut rd = match tokio::fs::read_dir(&dir).await {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        while let Ok(Some(entry)) = rd.next_entry().await {
            let meta = match tokio::fs::symlink_metadata(entry.path()).await {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn is_loader_id(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    lower.starts_with("fabric-loader-")
        || lower.starts_with("quilt-loader-")
        || lower.starts_with("neoforge-")
        || lower.contains("-forge-")
}

fn loader_label(id: &str, inherits_from: Option<&str>) -> Option<String> {
    let lower = id.to_ascii_lowercase();
    if lower.starts_with("fabric-loader-") {
        Some("Fabric".into())
    } else if lower.starts_with("quilt-loader-") {
        Some("Quilt".into())
    } else if lower.starts_with("neoforge-") {
        Some("NeoForge".into())
    } else if lower.contains("-forge-") {
        Some("Forge".into())
    } else if inherits_from.is_some() {
        Some("继承版本".into())
    } else {
        None
    }
}

async fn remove_version_dir(game: &GamePaths, id: &str) -> Result<()> {
    if util::safe_component(id, "") != id || id.is_empty() {
        return Err(AppError::invalid_input("版本 ID 非法"));
    }
    let dir = game.version_dir(id);
    let meta = tokio::fs::symlink_metadata(&dir).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found(format!("本地未安装版本 {id}"))
        } else {
            e.into()
        }
    })?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(AppError::path_unsafe("拒绝删除符号链接或非目录"));
    }
    let root = tokio::fs::canonicalize(game.versions_dir()).await?;
    let canonical = tokio::fs::canonicalize(&dir).await?;
    if !canonical.starts_with(&root) || canonical.parent() != Some(root.as_path()) {
        return Err(AppError::path_unsafe(
            "版本目录不在 versions/ 下，已拒绝删除",
        ));
    }
    tokio::fs::remove_dir_all(&canonical).await.map_err(|e| {
        AppError::new(ErrorCode::Io, format!("删除版本 {id} 失败")).with_detail(e.to_string())
    })?;
    Ok(())
}

fn parse_index(bytes: &[u8]) -> Result<AssetIndexFile> {
    serde_json::from_slice(bytes).map_err(|e| {
        AppError::new(ErrorCode::VersionInvalid, "资源索引格式无效").with_detail(e.to_string())
    })
}

fn is_sha1(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Materialise legacy virtual assets (assets/virtual/legacy/<name>).
pub async fn materialize_virtual_assets(
    game: &GamePaths,
    index: &AssetIndexFile,
    virtual_id: &str,
) -> Result<u32> {
    if !index.is_virtual.unwrap_or(false) && !index.map_to_resources.unwrap_or(false) {
        return Ok(0);
    }
    let target_root = game
        .virtual_dir()
        .join(util::safe_component(virtual_id, "legacy"));
    let mut copied = 0u32;
    for (name, obj) in &index.objects {
        if !is_sha1(&obj.hash) {
            continue;
        }
        let src = game.objects_dir().join(&obj.hash[..2]).join(&obj.hash);
        if !tokio::fs::try_exists(&src).await.unwrap_or(false) {
            continue;
        }
        let dest = util::safe_join(&target_root, name)?;
        if let Some(parent) = dest.parent() {
            util::ensure_dir(parent).await?;
        }
        tokio::fs::copy(&src, &dest).await?;
        copied += 1;
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_VERSION: &str = include_str!("../../tests/fixtures/version_26.4-snapshot-5.json");
    const FIXTURE_MANIFEST: &str = include_str!("../../tests/fixtures/version_manifest_v2.json");

    #[test]
    fn parses_real_manifest() {
        let m: VersionManifest = serde_json::from_str(FIXTURE_MANIFEST).unwrap();
        assert!(m.versions.len() > 500);
        assert!(m.versions.iter().any(|v| v.id == "1.20.4"));
        assert!(m.versions.iter().any(|v| v.id == m.latest.release));
    }

    #[test]
    fn parses_real_version_json() {
        let v = VersionJson::parse(FIXTURE_VERSION.as_bytes()).unwrap();
        assert_eq!(v.id, "26.4-snapshot-5");
        assert_eq!(
            v.main_class.as_deref(),
            Some("net.minecraft.client.main.Main")
        );
        assert_eq!(v.java_major(), 25);
        assert!(v.client_download().is_ok());
        let libs = v.effective_libraries(&RuleContext::default());
        assert!(libs.len() > 50, "expected many libs, got {}", libs.len());
        assert!(libs.iter().all(|l| !l.path.contains("..")));
        assert!(v.arguments.as_ref().unwrap().game.len() > 5);
    }

    #[test]
    fn plan_includes_client_libraries_and_assets() {
        let v = VersionJson::parse(FIXTURE_VERSION.as_bytes()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        let index: AssetIndexFile = serde_json::from_str(
            r#"{"objects":{"icons/icon_16x16.png":{"hash":"0123456789abcdef0123456789abcdef01234567","size":10},
                          "sounds/a.ogg":{"hash":"89abcdef0123456789abcdef0123456789abcdef","size":20}}}"#,
        )
        .unwrap();
        let plan = VersionService::build_install_plan(
            &game,
            &v,
            Some(&index),
            true,
            &RuleContext::default(),
        )
        .unwrap();
        assert!(plan.files.iter().any(|f| f.kind == "client"));
        assert!(plan.files.iter().filter(|f| f.kind == "asset").count() == 2);
        assert!(plan.total_bytes > 0);
        assert!(plan.files.iter().all(|f| f.dest.starts_with(game.root())));
        // Modern manifests (1.19+) carry natives as classified libraries on the
        // classpath, so no extraction list is expected for 26.4.
        assert!(plan.natives.is_empty());
    }

    #[test]
    fn plan_without_assets_omits_asset_files() {
        let v = VersionJson::parse(FIXTURE_VERSION.as_bytes()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        let plan =
            VersionService::build_install_plan(&game, &v, None, false, &RuleContext::default())
                .unwrap();
        assert!(!plan.files.iter().any(|f| f.kind == "asset"));
    }

    #[tokio::test]
    async fn installed_versions_scans_disk() {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        game.ensure_layout().await.unwrap();
        let v = VersionJson::parse(FIXTURE_VERSION.as_bytes()).unwrap();
        VersionService::new(reqwest::Client::new())
            .save_installed_metadata(&game, &v)
            .await
            .unwrap();
        tokio::fs::write(game.version_jar(&v.id), b"fake")
            .await
            .unwrap();
        let svc = VersionService::new(reqwest::Client::new());
        let list = svc.installed_versions(&game).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, v.id);
        assert!(list[0].has_jar);
        assert_eq!(list[0].java_major, Some(25));
    }

    #[tokio::test]
    async fn verify_reports_missing_jar() {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        game.ensure_layout().await.unwrap();
        let v = VersionJson::parse(FIXTURE_VERSION.as_bytes()).unwrap();
        let svc = VersionService::new(reqwest::Client::new());
        let report = svc
            .verify_installed(&game, &v, false, false, &RuleContext::default())
            .await
            .unwrap();
        assert!(!report.ok);
        assert!(report.issues.iter().any(|i| i.kind == "client"));
        assert!(report.issues.iter().any(|i| i.kind == "asset_index"));
    }

    #[tokio::test]
    #[ignore = "diagnostic: reads SR_APP_DATA cache"]
    async fn reads_existing_manifest_cache() {
        let data = std::env::var("SR_APP_DATA").expect("SR_APP_DATA");
        let paths = AppPaths::new(data);
        let svc = VersionService::new(reqwest::Client::new());
        let result = svc.manifest(&paths, false).await.unwrap();
        assert!(!result.manifest.versions.is_empty());
        println!(
            "cache manifest: {} versions, latest release {}",
            result.manifest.versions.len(),
            result.manifest.latest.release
        );
    }

    #[tokio::test]
    #[ignore = "requires network access to Mojang"]
    async fn fetches_manifest_from_official_source() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        let svc = VersionService::new(reqwest::Client::new());
        let result = svc.manifest(&paths, true).await.unwrap();
        assert!(matches!(result.source, ManifestSource::Network));
        assert!(result.manifest.versions.len() > 500);
        assert!(tokio::fs::try_exists(paths.manifest_cache()).await.unwrap());
    }

    async fn write_version(game: &GamePaths, id: &str, inherits: Option<&str>) {
        let dir = game.version_dir(id);
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let json = match inherits {
            Some(parent) => format!(
                r#"{{"id":"{id}","inheritsFrom":"{parent}","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient"}}"#
            ),
            None => format!(
                r#"{{"id":"{id}","type":"release","mainClass":"net.minecraft.client.main.Main","javaVersion":{{"majorVersion":17}}}}"#
            ),
        };
        tokio::fs::write(dir.join(format!("{id}.json")), json)
            .await
            .unwrap();
        tokio::fs::write(dir.join(format!("{id}.jar")), b"jar")
            .await
            .unwrap();
    }

    fn instance(id: &str, name: &str, version_id: &str) -> crate::instance::Instance {
        crate::instance::Instance {
            id: id.into(),
            name: name.into(),
            version_id: version_id.into(),
            game_dir: std::path::PathBuf::from("/tmp/never-created"),
            java_path: None,
            memory_mb: 2048,
            jvm_args: vec![],
            game_args: vec![],
            created_at: "2026-01-01T00:00:00Z".into(),
            last_launched_at: None,
            account_id: None,
            resolution_width: None,
            resolution_height: None,
            mod_loader: None,
        }
    }

    #[tokio::test]
    async fn installed_versions_reports_size_and_loader() {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        game.ensure_layout().await.unwrap();
        write_version(&game, "1.20.4", None).await;
        write_version(&game, "fabric-loader-0.16.0-1.20.4", Some("1.20.4")).await;
        let svc = VersionService::new(reqwest::Client::new());
        let list = svc.installed_versions(&game).await.unwrap();
        assert_eq!(list.len(), 2);
        let fabric = list
            .iter()
            .find(|v| v.id.starts_with("fabric-loader"))
            .unwrap();
        assert_eq!(fabric.loader.as_deref(), Some("Fabric"));
        assert_eq!(fabric.inherits_from.as_deref(), Some("1.20.4"));
        assert!(fabric.size_bytes > 0);
        assert!(fabric.modified_at.is_some());
        assert!(fabric.path.ends_with("fabric-loader-0.16.0-1.20.4"));
    }

    #[tokio::test]
    async fn delete_plan_finds_dependent_versions_and_instances() {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        game.ensure_layout().await.unwrap();
        write_version(&game, "1.20.4", None).await;
        write_version(&game, "fabric-loader-0.16.0-1.20.4", Some("1.20.4")).await;
        let svc = VersionService::new(reqwest::Client::new());
        let instances = vec![instance("i1", "生存", "fabric-loader-0.16.0-1.20.4")];
        let plan = svc.delete_plan(&game, &instances, "1.20.4").await.unwrap();
        assert_eq!(plan.dependents, vec!["fabric-loader-0.16.0-1.20.4"]);
        assert_eq!(plan.instances.len(), 1);
        assert!(plan.instances[0].contains("生存"));
        assert!(plan.size_bytes > 0);

        let err = svc
            .delete_installed(&game, &instances, "1.20.4", true)
            .await
            .unwrap_err();
        assert!(err.message.contains("实例"));
        assert!(game.version_dir("1.20.4").exists());
    }

    #[tokio::test]
    async fn resolves_base_for_flattened_forge_without_extra() {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        game.ensure_layout().await.unwrap();
        write_version(&game, "1.21", None).await;
        let id = "1.21-forge-51.0.22";
        let vdir = game.version_dir(id);
        tokio::fs::create_dir_all(&vdir).await.unwrap();
        tokio::fs::write(
            vdir.join(format!("{id}.json")),
            format!(
                r#"{{"id":"{id}","type":"release","jar":"1.21","mainClass":"net.minecraftforge.bootstrap.ForgeBootstrap","libraries":[]}}"#
            ),
        )
        .await
        .unwrap();
        let svc = VersionService::new(reqwest::Client::new());
        assert_eq!(
            svc.base_version_of(&game, id).await.unwrap().as_deref(),
            Some("1.21")
        );
    }

    #[tokio::test]
    async fn delete_requires_force_for_inherited_versions() {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        game.ensure_layout().await.unwrap();
        write_version(&game, "1.20.4", None).await;
        write_version(&game, "quilt-loader-0.24.0-1.20.4", Some("1.20.4")).await;
        let svc = VersionService::new(reqwest::Client::new());
        let err = svc
            .delete_installed(&game, &[], "1.20.4", false)
            .await
            .unwrap_err();
        assert!(err.message.contains("继承"));

        let result = svc
            .delete_installed(&game, &[], "1.20.4", true)
            .await
            .unwrap();
        assert_eq!(result.deleted.len(), 2);
        assert!(!game.version_dir("1.20.4").exists());
        assert!(!game.version_dir("quilt-loader-0.24.0-1.20.4").exists());
    }

    #[tokio::test]
    async fn delete_plan_infers_loader_base_from_resolved_id() {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        game.ensure_layout().await.unwrap();
        write_version(&game, "1.20.4", None).await;
        // A resolved loader profile has no inheritsFrom in the saved file.
        let loader_id = "fabric-loader-0.16.0-1.20.4";
        let loader_dir = game.version_dir(loader_id);
        tokio::fs::create_dir_all(&loader_dir).await.unwrap();
        tokio::fs::write(
            loader_dir.join(format!("{loader_id}.json")),
            format!(r#"{{"id":"{loader_id}","type":"release","mainClass":"x"}}"#),
        )
        .await
        .unwrap();

        let svc = VersionService::new(reqwest::Client::new());
        let plan = svc.delete_plan(&game, &[], "1.20.4").await.unwrap();
        assert_eq!(plan.dependents, vec![loader_id]);
        assert_eq!(
            svc.base_version_of(&game, loader_id)
                .await
                .unwrap()
                .as_deref(),
            Some("1.20.4")
        );
        assert_eq!(svc.base_version_of(&game, "1.20.4").await.unwrap(), None);
    }

    #[tokio::test]
    async fn delete_rejects_unsafe_id_and_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        game.ensure_layout().await.unwrap();
        let svc = VersionService::new(reqwest::Client::new());
        assert!(svc
            .delete_installed(&game, &[], "../x", true)
            .await
            .is_err());

        #[cfg(unix)]
        {
            let outside = dir.path().join("outside");
            std::fs::create_dir_all(&outside).unwrap();
            std::os::unix::fs::symlink(&outside, game.version_dir("evil")).unwrap();
            assert!(svc
                .delete_installed(&game, &[], "evil", true)
                .await
                .is_err());
            assert!(outside.exists());
        }
    }
}
