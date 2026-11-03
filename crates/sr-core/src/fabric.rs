//! Fabric loader adapter. Uses only the official Fabric meta service and
//! Maven repository; no third-party installer binaries are executed.

use crate::download::ensure_trusted_url;
use crate::error::{AppError, ErrorCode, Result};
use crate::paths::GamePaths;
use crate::version::{resolve_inheritance, LibraryName, VersionJson, VersionService};
use serde::{Deserialize, Serialize};

pub const FABRIC_META: &str = "https://meta.fabricmc.net";
pub const QUILT_META: &str = "https://meta.quiltmc.org";
/// Public BMCLAPI mirror used as an automatic fallback. It is optional and
/// only ever serves the same official metadata/artifacts over HTTPS.
pub const BMCLAPI: &str = "https://bmclapi2.bangbang93.com";
/// Direct artifact mirror (BMCLAPI nodes redirect to hosts that some networks
/// cannot reach; this USTC mirror serves the same path without redirects).
pub const BMCLAPI_MAVEN: &str = "https://cmcc.mirrors.ustc.edu.cn/bmclapi";
const MAX_META_BYTES: usize = 8 * 1024 * 1024;
const META_ATTEMPTS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderKind {
    Fabric,
    Quilt,
}

impl LoaderKind {
    fn meta_base(self) -> &'static str {
        match self {
            Self::Fabric => FABRIC_META,
            Self::Quilt => QUILT_META,
        }
    }
    fn mirror_slug(self) -> &'static str {
        match self {
            Self::Fabric => "fabric",
            Self::Quilt => "quilt",
        }
    }
    fn api_version(self) -> &'static str {
        match self {
            Self::Fabric => "v2",
            Self::Quilt => "v3",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Fabric => "Fabric",
            Self::Quilt => "Quilt",
        }
    }
}

pub type QuiltLoader = FabricLoader;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricLoader {
    pub version: String,
    pub stable: bool,
    pub game_version: String,
}

#[derive(Debug, Deserialize)]
struct LoaderEntry {
    loader: LoaderPart,
    #[serde(default)]
    #[allow(dead_code)]
    intermediary: Option<IntermediaryPart>,
}

#[derive(Debug, Deserialize)]
struct LoaderPart {
    version: String,
    #[serde(default)]
    stable: bool,
}

#[derive(Debug, Deserialize)]
struct IntermediaryPart {
    #[serde(default)]
    #[allow(dead_code)]
    version: Option<String>,
}

#[derive(Clone)]
pub struct FabricService {
    http: reqwest::Client,
    kind: LoaderKind,
    /// Prefer the BMCLAPI mirror, then fall back to the official host.
    use_mirror: bool,
}

impl FabricService {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            kind: LoaderKind::Fabric,
            use_mirror: false,
        }
    }

    /// Quilt shares the same profile/install shape with a different meta base.
    pub fn quilt(http: reqwest::Client) -> Self {
        Self {
            http,
            kind: LoaderKind::Quilt,
            use_mirror: false,
        }
    }

    /// Enable/disable the BMCLAPI mirror preference. When enabled the mirror is
    /// tried first and the official host is still used as fallback.
    pub fn with_mirror(mut self, use_mirror: bool) -> Self {
        self.use_mirror = use_mirror;
        self
    }

    pub fn kind(&self) -> LoaderKind {
        self.kind
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

    /// Try the mirror and/or official host for a meta path. Returns the parsed
    /// value and whether the mirror was used (so profile URLs can be rewritten).
    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<(T, bool)> {
        let official = format!(
            "{}/{}/{}",
            self.kind.meta_base(),
            self.kind.api_version(),
            path
        );
        let mirrored = format!(
            "{}/{}-meta/{}/{}",
            BMCLAPI,
            self.kind.mirror_slug(),
            self.kind.api_version(),
            path
        );
        let candidates: Vec<(&str, bool)> = if self.use_mirror {
            vec![(mirrored.as_str(), true), (official.as_str(), false)]
        } else {
            vec![(official.as_str(), false), (mirrored.as_str(), true)]
        };
        let mut last_error: Option<AppError> = None;
        for (url, mirrored_used) in candidates {
            match self.get_json_url::<T>(url).await {
                Ok(value) => return Ok((value, mirrored_used)),
                Err(e) => last_error = Some(e),
            }
        }
        Err(last_error
            .unwrap_or_else(|| AppError::network(format!("无法获取 {} 元数据", self.kind.name()))))
    }

    async fn get_json_url<T: serde::de::DeserializeOwned>(&self, raw_url: &str) -> Result<T> {
        let parsed = ensure_trusted_url(raw_url, false)?;
        let mut last_error: Option<AppError> = None;
        for attempt in 0..META_ATTEMPTS {
            let result = self
                .http
                .get(parsed.clone())
                .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
                .timeout(std::time::Duration::from_secs(15))
                .send()
                .await;
            let resp = match result {
                Ok(resp) => resp,
                Err(e) => {
                    let err = AppError::from(e);
                    if attempt + 1 < META_ATTEMPTS && err.retryable {
                        last_error = Some(err);
                        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                        continue;
                    }
                    return Err(err);
                }
            };
            let status = resp.status();
            if !status.is_success() {
                let err = AppError::new(
                    ErrorCode::Http,
                    format!("{} 元数据返回 HTTP {}", self.kind.name(), status.as_u16()),
                )
                .retryable(status.is_server_error() || status.as_u16() == 429);
                if attempt + 1 < META_ATTEMPTS && err.retryable {
                    last_error = Some(err);
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                    continue;
                }
                return Err(err);
            }
            let bytes = resp.bytes().await?;
            if bytes.len() > MAX_META_BYTES {
                return Err(AppError::new(
                    ErrorCode::VersionInvalid,
                    format!("{} 元数据异常过大", self.kind.name()),
                ));
            }
            return serde_json::from_slice(&bytes).map_err(|e| {
                AppError::new(
                    ErrorCode::VersionInvalid,
                    format!("{} 元数据格式无效", self.kind.name()),
                )
                .with_detail(e.to_string())
            });
        }
        Err(last_error
            .unwrap_or_else(|| AppError::network(format!("无法获取 {} 元数据", self.kind.name()))))
    }

    /// List loader versions compatible with a Minecraft version.
    pub async fn list_loaders(&self, game_version: &str) -> Result<Vec<FabricLoader>> {
        Self::validate_segment(game_version, "Minecraft 版本")?;
        let path = format!("versions/loader/{game_version}");
        let (entries, _mirrored): (Vec<LoaderEntry>, bool) = self.get_json(&path).await?;
        let mut out: Vec<FabricLoader> = entries
            .into_iter()
            .map(|e| FabricLoader {
                version: e.loader.version,
                stable: e.loader.stable,
                game_version: game_version.to_string(),
            })
            .collect();
        // Stable builds first, newest first within each group (the API already
        // returns newest-first; preserve that order).
        out.sort_by_key(|l| !l.stable);
        Ok(out)
    }

    /// Fetch the loader profile JSON.
    pub async fn profile(&self, game_version: &str, loader_version: &str) -> Result<VersionJson> {
        Self::validate_segment(game_version, "Minecraft 版本")?;
        Self::validate_segment(loader_version, &format!("{} 加载器版本", self.kind.name()))?;
        let path = format!("versions/loader/{game_version}/{loader_version}/profile/json");
        let (mut profile, _mirrored): (VersionJson, bool) = self.get_json(&path).await?;
        if self.use_mirror {
            self.rewrite_profile_urls(&mut profile);
        }
        if profile.inherits_from.as_deref() != Some(game_version) {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                format!(
                    "{} profile 的 inheritsFrom 与目标版本不一致",
                    self.kind.name()
                ),
            ));
        }
        if profile.libraries.as_deref().unwrap_or_default().is_empty() {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                format!("{} profile 没有库依赖", self.kind.name()),
            ));
        }
        self.enrich_missing_hashes(&mut profile).await;
        Ok(profile)
    }

    /// Point Fabric Maven artifacts at the BMCLAPI mirror when metadata came
    /// from the mirror. Quilt Maven has no mirror entry, so those URLs stay
    /// official (and fail honestly if the network blocks them).
    fn rewrite_profile_urls(&self, profile: &mut VersionJson) {
        let Some(libs) = profile.libraries.as_mut() else {
            return;
        };
        for lib in libs.iter_mut() {
            // BMCLAPI/USTC do not mirror Quilt's `hashed` mappings artifact;
            // leave it on the official Maven (it is small and cached by the
            // official service). Other Quilt/Fabric artifacts are mirrored.
            if lib.name.starts_with("org.quiltmc:hashed:") {
                continue;
            }
            let Some(url) = lib.url.as_mut() else {
                continue;
            };
            if let Some(rest) = url.strip_prefix("https://maven.fabricmc.net/") {
                *url = format!("{BMCLAPI_MAVEN}/{rest}");
            } else if let Some(rest) =
                url.strip_prefix("https://maven.quiltmc.org/repository/release/")
            {
                *url = format!("{BMCLAPI_MAVEN}/{rest}");
            }
        }
    }

    /// Loader profile JSON omits hashes for some artifacts. The Maven sidecar
    /// `.sha1` is fetched so downloads can still be verified.
    async fn enrich_missing_hashes(&self, profile: &mut VersionJson) {
        let Some(libs) = profile.libraries.as_mut() else {
            return;
        };
        let mut fetched = 0;
        for lib in libs.iter_mut() {
            if lib.sha1.is_some() {
                continue;
            }
            let Some(base) = &lib.url else {
                continue;
            };
            let Some(name) = LibraryName::parse(&lib.name) else {
                continue;
            };
            if fetched >= 32 {
                break;
            }
            let path = name.path();
            let url = format!("{}/{}.sha1", base.trim_end_matches('/'), path);
            let Ok(parsed) = ensure_trusted_url(&url, false) else {
                continue;
            };
            let Ok(resp) = self
                .http
                .get(parsed)
                .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
            else {
                continue;
            };
            if !resp.status().is_success() {
                continue;
            }
            let Ok(text) = resp.text().await else {
                continue;
            };
            let candidate = text.split_whitespace().next().unwrap_or("");
            if candidate.len() == 40 && candidate.bytes().all(|b| b.is_ascii_hexdigit()) {
                lib.sha1 = Some(candidate.to_ascii_lowercase());
                fetched += 1;
            }
        }
    }

    /// Resolve a loader profile against an already-installed base version.
    /// The returned metadata keeps the loader version id but points `jar` at
    /// the base version so the client jar is shared.
    pub async fn resolve_for_install(
        &self,
        versions: &VersionService,
        game: &GamePaths,
        game_version: &str,
        loader_version: &str,
    ) -> Result<VersionJson> {
        let base = versions.load_installed(game, game_version).await?;
        let base_jar = game.version_jar(&base.jar_id());
        if !tokio::fs::try_exists(&base_jar).await.unwrap_or(false) {
            return Err(AppError::new(
                ErrorCode::NotFound,
                format!("基础版本 {game_version} 的客户端 JAR 缺失，请先重新安装基础版本"),
            ));
        }
        let profile = self.profile(game_version, loader_version).await?;
        self.resolve_profile(base, game_version, profile)
    }

    /// Merge an already-fetched profile with a base version. Kept separate from
    /// network access so offline caches and tests can reuse it.
    pub fn resolve_profile(
        &self,
        base: VersionJson,
        game_version: &str,
        profile: VersionJson,
    ) -> Result<VersionJson> {
        if profile.inherits_from.as_deref() != Some(game_version) {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                format!(
                    "{} profile 的 inheritsFrom 与目标版本不一致",
                    self.kind.name()
                ),
            ));
        }
        let loader_id = profile.id.clone();
        if crate::util::safe_component(&loader_id, "") != loader_id {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                format!("{} 版本 ID 包含非法字符", self.kind.name()),
            ));
        }
        let base_for_merge = base;
        let mut resolved = resolve_inheritance(profile, |id| {
            if id == game_version {
                Ok(base_for_merge.clone())
            } else {
                Err(AppError::not_found(format!("缺少父版本 {id}")))
            }
        })?;
        if resolved.jar.is_none() {
            resolved.jar = Some(game_version.to_string());
        }
        resolved.validate_for_launch()?;
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::RuleContext;

    const PROFILE: &str = include_str!("../tests/fixtures/fabric_profile_1.20.4.json");
    const QUILT_PROFILE: &str = include_str!("../tests/fixtures/quilt_profile_1.20.4.json");
    const QUILT_LOADERS: &str = include_str!("../tests/fixtures/quilt_loaders_1.20.4.json");
    const BASE: &str = include_str!("../tests/fixtures/version_1.20.4.json");
    const LOADERS: &str = include_str!("../tests/fixtures/fabric_loaders_1.20.4.json");

    #[test]
    fn parses_real_loader_list() {
        let entries: Vec<LoaderEntry> = serde_json::from_str(LOADERS).unwrap();
        assert!(entries.len() > 50);
        assert!(entries.iter().any(|e| e.loader.stable));
        assert!(entries.iter().any(|e| e.loader.version.starts_with("0.")));
    }

    #[test]
    fn profile_resolves_against_base() {
        let profile: VersionJson = serde_json::from_str(PROFILE).unwrap();
        let base: VersionJson = serde_json::from_str(BASE).unwrap();
        let base_id = base.id.clone();
        let mut resolved = resolve_inheritance(profile, |id| {
            if id == base_id {
                Ok(base.clone())
            } else {
                Err(AppError::not_found("missing"))
            }
        })
        .unwrap();
        resolved.jar = Some("1.20.4".into());
        assert_eq!(resolved.id, "fabric-loader-0.16.0-1.20.4");
        assert_eq!(resolved.jar_id(), "1.20.4");
        assert_eq!(
            resolved.main_class.as_deref(),
            Some("net.fabricmc.loader.impl.launch.knot.KnotClient")
        );
        assert_eq!(resolved.java_major(), 17);
        let libs = resolved.effective_libraries(&RuleContext::default());
        assert!(libs
            .iter()
            .any(|l| l.name.starts_with("net.fabricmc:fabric-loader")));
        assert!(libs.iter().any(|l| l.name.starts_with("org.ow2.asm:asm")));
        assert!(libs
            .iter()
            .filter(|l| l.name.starts_with("org.ow2.asm"))
            .all(|l| l.sha1.is_some()));
        // Base libraries are inherited too.
        assert!(libs.iter().any(|l| l.name.starts_with("com.mojang:")));
    }

    #[test]
    fn parses_real_quilt_metadata() {
        let entries: Vec<LoaderEntry> = serde_json::from_str(QUILT_LOADERS).unwrap();
        assert!(entries.len() > 100);
        assert!(entries.iter().any(|e| e.loader.version.contains("0.20")));

        let profile: VersionJson = serde_json::from_str(QUILT_PROFILE).unwrap();
        assert_eq!(profile.inherits_from.as_deref(), Some("1.20.4"));
        let base: VersionJson = serde_json::from_str(BASE).unwrap();
        let base_id = base.id.clone();
        let resolved = resolve_inheritance(profile, |id| {
            if id == base_id {
                Ok(base.clone())
            } else {
                Err(AppError::not_found("missing"))
            }
        })
        .unwrap();
        assert!(resolved.id.starts_with("quilt-loader-"));
        assert_eq!(
            resolved.main_class.as_deref(),
            Some("org.quiltmc.loader.impl.launch.knot.KnotClient")
        );
        assert!(resolved.libraries.as_deref().unwrap().len() >= 14);
        assert!(resolved
            .effective_libraries(&RuleContext::default())
            .iter()
            .any(|l| l.name.starts_with("org.quiltmc:quilt-loader")));
    }

    #[test]
    fn quilt_plan_uses_maven_and_shared_jar() {
        let profile: VersionJson = serde_json::from_str(QUILT_PROFILE).unwrap();
        let base: VersionJson = serde_json::from_str(BASE).unwrap();
        let service = FabricService::quilt(reqwest::Client::new());
        let resolved = service.resolve_profile(base, "1.20.4", profile).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path());
        let plan = VersionService::build_install_plan(
            &game,
            &resolved,
            None,
            false,
            &RuleContext::default(),
        )
        .unwrap();
        let client = plan.files.iter().find(|f| f.kind == "client").unwrap();
        assert_eq!(client.dest, game.version_jar("1.20.4"));
        assert!(plan
            .files
            .iter()
            .any(|f| f.url.contains("maven.quiltmc.org") && f.label.contains("quilt-loader")));
        assert!(plan
            .files
            .iter()
            .any(|f| f.url.contains("maven.fabricmc.net")));
        assert_eq!(plan.version.jar_id(), "1.20.4");
    }

    #[test]
    fn rejects_unsafe_segments() {
        for bad in ["", "../x", "a/b", "a\\b", "a b", "x?".repeat(40).as_str()] {
            assert!(FabricService::validate_segment(bad, "x").is_err(), "{bad}");
        }
        assert!(FabricService::validate_segment("1.20.4", "x").is_ok());
        assert!(FabricService::validate_segment("0.16.0", "x").is_ok());
    }

    #[test]
    fn loader_list_puts_stable_first() {
        let mut list = [
            FabricLoader {
                version: "0.17.0".into(),
                stable: false,
                game_version: "1.20.4".into(),
            },
            FabricLoader {
                version: "0.16.0".into(),
                stable: true,
                game_version: "1.20.4".into(),
            },
        ];
        list.sort_by_key(|l| !l.stable);
        assert!(list[0].stable);
    }
}
