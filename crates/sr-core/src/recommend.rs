//! Loader/shader recommendations for a specific installed Minecraft version.
//!
//! Data comes from official Fabric/Quilt metadata, BMCLAPI's Forge/OptiFine
//! indexes and the NeoForge Maven metadata. Forge/NeoForge/OptiFine require
//! running their official installers; this module only recommends a matching
//! version and returns the official download URL, never executes them.

use crate::download::ensure_trusted_url;
use crate::error::{AppError, ErrorCode, Result};
use crate::fabric::FabricService;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const BMCLAPI: &str = "https://bmclapi2.bangbang93.com";
const NEOFORGE_METADATA: &str =
    "https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge/maven-metadata.xml";
const MAX_META_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    /// recommended / available / unavailable / unsupported
    pub status: String,
    pub note: String,
    pub install_supported: bool,
    pub download_url: Option<String>,
    /// Selectable loader versions, newest first. Empty for non-loader items.
    #[serde(default)]
    pub versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Recommendations {
    pub game_version: String,
    pub items: Vec<Recommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ForgeBuild {
    #[serde(default)]
    version: String,
    #[serde(default)]
    mcversion: String,
    #[serde(default)]
    build: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OptiFineBuild {
    #[serde(default)]
    mcversion: String,
    #[serde(default)]
    patch: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    forge: Option<String>,
}

async fn get_json<T: serde::de::DeserializeOwned>(http: &reqwest::Client, url: &str) -> Result<T> {
    let parsed = ensure_trusted_url(url, false)?;
    let resp = http
        .get(parsed)
        .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
        .timeout(Duration::from_secs(20))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(AppError::new(
            ErrorCode::Http,
            format!("推荐数据返回 HTTP {}", resp.status().as_u16()),
        ));
    }
    let bytes = resp.bytes().await?;
    if bytes.len() > MAX_META_BYTES {
        return Err(AppError::new(ErrorCode::VersionInvalid, "推荐数据异常过大"));
    }
    serde_json::from_slice(&bytes).map_err(|e| {
        AppError::new(ErrorCode::VersionInvalid, "推荐数据格式无效").with_detail(e.to_string())
    })
}

async fn get_text(http: &reqwest::Client, url: &str) -> Result<String> {
    let parsed = ensure_trusted_url(url, false)?;
    let resp = http
        .get(parsed)
        .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
        .timeout(Duration::from_secs(20))
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(AppError::new(
            ErrorCode::Http,
            format!("推荐数据返回 HTTP {}", resp.status().as_u16()),
        ));
    }
    let bytes = resp.bytes().await?;
    if bytes.len() > MAX_META_BYTES {
        return Err(AppError::new(ErrorCode::VersionInvalid, "推荐数据异常过大"));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn forge_versions(json: &[u8], game_version: &str) -> Result<Vec<String>> {
    let builds: Vec<ForgeBuild> = serde_json::from_slice(json).map_err(|e| {
        AppError::new(ErrorCode::VersionInvalid, "Forge 版本列表格式无效")
            .with_detail(e.to_string())
    })?;
    let mut out: Vec<(u64, String)> = builds
        .into_iter()
        .filter(|b| b.mcversion == game_version && !b.version.is_empty())
        .map(|b| (b.build, b.version))
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.0));
    Ok(out.into_iter().map(|(_, v)| v).collect())
}

pub async fn forge_available_versions(
    http: &reqwest::Client,
    game_version: &str,
) -> Result<Vec<String>> {
    let url = format!("{BMCLAPI}/forge/minecraft/{game_version}");
    let list: Vec<ForgeBuild> = get_json(http, &url).await?;
    let bytes = serde_json::to_vec(&list).unwrap_or_default();
    forge_versions(&bytes, game_version)
}

pub async fn neoforge_available_versions(
    http: &reqwest::Client,
    game_version: &str,
) -> Result<Vec<String>> {
    let xml = get_text(http, NEOFORGE_METADATA).await?;
    Ok(neoforge_versions(&xml, game_version))
}

pub fn neoforge_versions(xml: &str, game_version: &str) -> Vec<String> {
    let Some(prefix) = neoforge_prefix(game_version) else {
        return Vec::new();
    };
    let mut out: Vec<String> = xml_versions(xml)
        .into_iter()
        .filter(|v| v.starts_with(&prefix))
        .collect();
    out.sort_by_key(|v| {
        let mut parts = v
            .split(['.', '-'])
            .filter_map(|p| p.parse::<u64>().ok())
            .collect::<Vec<_>>();
        parts.resize(4, 0);
        std::cmp::Reverse((parts[0], parts[1], parts[2], parts[3]))
    });
    out
}

pub fn select_forge(json: &[u8], game_version: &str) -> Result<Option<(String, String)>> {
    let builds: Vec<ForgeBuild> = serde_json::from_slice(json).map_err(|e| {
        AppError::new(ErrorCode::VersionInvalid, "Forge 版本列表格式无效")
            .with_detail(e.to_string())
    })?;
    let best = builds
        .into_iter()
        .filter(|b| b.mcversion == game_version && !b.version.is_empty())
        .max_by_key(|b| b.build);
    Ok(best.map(|b| {
        let url = format!(
            "{BMCLAPI}/forge/download?mcversion={game_version}&version={}&category=installer&format=jar",
            b.version
        );
        (b.version, url)
    }))
}

/// Extract the Forge version an OptiFine build was compiled against
/// (`"Forge 52.0.16"` -> `"52.0.16"`). `None` for vanilla / N/A builds.
pub fn optifine_forge_requirement(forge: Option<&str>) -> Option<String> {
    let value = forge?.trim();
    let value = value
        .strip_prefix("Forge ")
        .or_else(|| value.strip_prefix("forge "))?
        .trim();
    if value.is_empty() || value.eq_ignore_ascii_case("N/A") {
        None
    } else {
        Some(value.to_string())
    }
}

/// Compare dotted numeric versions component-wise. Non-numeric chunks are
/// ignored; missing components count as zero.
pub fn version_gt(a: &str, b: &str) -> bool {
    let parse = |value: &str| -> Vec<u64> {
        value
            .split(|c: char| !c.is_ascii_digit())
            .filter_map(|part| part.parse::<u64>().ok())
            .collect()
    };
    let mut left = parse(a);
    let mut right = parse(b);
    let len = left.len().max(right.len());
    left.resize(len, 0);
    right.resize(len, 0);
    left > right
}

/// `(filename, label, download URL, Forge version)`.
pub type OptiFineSelection = (String, String, String, Option<String>);

pub fn select_optifine_file(json: &[u8], game_version: &str) -> Result<Option<OptiFineSelection>> {
    let builds: Vec<OptiFineBuild> = serde_json::from_slice(json).map_err(|e| {
        AppError::new(ErrorCode::VersionInvalid, "OptiFine 版本列表格式无效")
            .with_detail(e.to_string())
    })?;
    let normal: Vec<OptiFineBuild> = builds
        .iter()
        .filter(|b| {
            b.mcversion == game_version && !b.filename.is_empty() && !b.filename.contains("preview")
        })
        .cloned()
        .collect();
    let previews: Vec<OptiFineBuild> = builds
        .into_iter()
        .filter(|b| {
            b.mcversion == game_version
                && !b.filename.is_empty()
                && b.filename.contains("preview")
                && b.forge
                    .as_deref()
                    .is_some_and(|f| !f.trim().is_empty() && f != "Forge N/A")
        })
        .collect();
    let best = normal
        .iter()
        .find(|b| b.kind == "HD_U")
        .cloned()
        .or_else(|| normal.first().cloned())
        .or_else(|| {
            previews.into_iter().max_by_key(|b| {
                b.patch
                    .rsplit_once("pre")
                    .and_then(|(_, n)| n.parse::<u64>().ok())
                    .unwrap_or(0)
            })
        });
    Ok(best.map(|b| {
        let label = if b.patch.is_empty() {
            b.filename.clone()
        } else {
            format!("{} {}", b.kind, b.patch)
        };
        let url = format!("{BMCLAPI}/optifine/{game_version}/{}/{}", b.kind, b.patch);
        (b.filename.clone(), label, url, b.forge.clone())
    }))
}

pub fn select_optifine(json: &[u8], game_version: &str) -> Result<Option<(String, String)>> {
    Ok(select_optifine_file(json, game_version)?.map(|(_, label, url, _)| (label, url)))
}

/// Filename + download URL for the recommended OptiFine package.
pub async fn optifine_info(
    http: &reqwest::Client,
    game_version: &str,
) -> Result<Option<(String, String)>> {
    Ok(optifine_build_info(http, game_version)
        .await?
        .map(|b| (b.filename, b.url)))
}

fn optifine_selection(build: &OptiFineBuild, game_version: &str) -> OptiFineSelection {
    let label = if build.patch.is_empty() {
        build.filename.clone()
    } else {
        format!("{} {}", build.kind, build.patch)
    };
    let url = format!(
        "{BMCLAPI}/optifine/{game_version}/{}/{}",
        build.kind, build.patch
    );
    (build.filename.clone(), label, url, build.forge.clone())
}

/// Normal (non-preview) OptiFine builds for a Minecraft version, newest first.
pub fn select_optifine_all(json: &[u8], game_version: &str) -> Result<Vec<OptiFineSelection>> {
    let builds: Vec<OptiFineBuild> = serde_json::from_slice(json).map_err(|e| {
        AppError::new(ErrorCode::VersionInvalid, "OptiFine 版本列表格式无效")
            .with_detail(e.to_string())
    })?;
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for build in builds.iter().filter(|b| {
        b.mcversion == game_version && !b.filename.is_empty() && !b.filename.contains("preview")
    }) {
        let selection = optifine_selection(build, game_version);
        if seen.insert(selection.1.clone()) {
            out.push(selection);
        }
    }
    Ok(out)
}

fn select_optifine_label(
    json: &[u8],
    game_version: &str,
    label: &str,
) -> Result<Option<OptiFineSelection>> {
    Ok(select_optifine_all(json, game_version)?
        .into_iter()
        .find(|(_, item_label, _, _)| item_label == label))
}

/// Full OptiFine build metadata, including the Forge version the build was
/// compiled against (`forge` in the BMCLAPI JSON).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptiFineInfo {
    pub filename: String,
    pub label: String,
    pub url: String,
    pub forge: Option<String>,
}

pub async fn optifine_build_info(
    http: &reqwest::Client,
    game_version: &str,
) -> Result<Option<OptiFineInfo>> {
    optifine_build_info_for(http, game_version, None).await
}

pub async fn optifine_build_info_for(
    http: &reqwest::Client,
    game_version: &str,
    requested_label: Option<&str>,
) -> Result<Option<OptiFineInfo>> {
    let url = format!("{BMCLAPI}/optifine/{game_version}");
    let list: Vec<OptiFineBuild> = get_json(http, &url).await?;
    let bytes = serde_json::to_vec(&list).unwrap_or_default();
    let selected = match requested_label {
        Some(label) => select_optifine_label(&bytes, game_version, label)?,
        None => select_optifine_file(&bytes, game_version)?,
    };
    Ok(selected.map(|(filename, label, url, forge)| OptiFineInfo {
        filename,
        label,
        url,
        forge,
    }))
}

pub async fn optifine_build_labels(
    http: &reqwest::Client,
    game_version: &str,
) -> Result<Vec<String>> {
    let url = format!("{BMCLAPI}/optifine/{game_version}");
    let list: Vec<OptiFineBuild> = get_json(http, &url).await?;
    let bytes = serde_json::to_vec(&list).unwrap_or_default();
    Ok(select_optifine_all(&bytes, game_version)?
        .into_iter()
        .map(|(_, label, _, _)| label)
        .collect())
}

fn xml_versions(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<version>") {
        rest = &rest[start + "<version>".len()..];
        let Some(end) = rest.find("</version>") else {
            break;
        };
        let value = rest[..end].trim();
        if !value.is_empty() {
            out.push(value.to_string());
        }
        rest = &rest[end + "</version>".len()..];
    }
    out
}

fn neoforge_prefix(game_version: &str) -> Option<String> {
    if let Some(rest) = game_version.strip_prefix("1.") {
        let parts: Vec<&str> = rest.split('.').collect();
        if parts.len() >= 2 {
            return Some(format!("{}.{}.", parts[0], parts[1]));
        }
    }
    if game_version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Some(format!("{game_version}."));
    }
    None
}

pub fn select_neoforge(xml: &str, game_version: &str) -> Option<(String, String)> {
    let prefix = neoforge_prefix(game_version)?;
    let best = xml_versions(xml)
        .into_iter()
        .filter(|v| v.starts_with(&prefix))
        .max_by_key(|v| {
            let mut parts = v
                .split(['.', '-'])
                .filter_map(|p| p.parse::<u64>().ok())
                .collect::<Vec<_>>();
            parts.resize(4, 0);
            (parts[0], parts[1], parts[2], parts[3])
        })?;
    let url = format!(
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/{best}/neoforge-{best}-installer.jar"
    );
    Some((best, url))
}

fn unavailable(id: &str, name: &str, error: impl std::fmt::Display) -> Recommendation {
    Recommendation {
        id: id.into(),
        name: name.into(),
        version: None,
        status: "unavailable".into(),
        note: format!("暂时无法获取推荐：{error}"),
        install_supported: false,
        download_url: None,
        versions: Vec::new(),
    }
}

pub async fn recommend(http: &reqwest::Client, game_version: &str) -> Recommendations {
    let fabric_http = http.clone();
    let quilt_http = http.clone();
    let forge_http = http.clone();
    let optifine_http = http.clone();
    let neo_http = http.clone();
    let game = game_version.to_string();

    let (fabric, quilt, forge, optifine, neoforge) = tokio::join!(
        async {
            let service = FabricService::new(fabric_http).with_mirror(true);
            match tokio::time::timeout(Duration::from_secs(20), service.list_loaders(&game)).await {
                Ok(Ok(list)) => list
                    .iter()
                    .find(|l| l.stable)
                    .cloned()
                    .or_else(|| list.into_iter().next())
                    .map(|l| Recommendation {
                        id: "fabric".into(),
                        name: "Fabric".into(),
                        version: Some(l.version.clone()),
                        status: "recommended".into(),
                        note: "稳定版 · 已配置 BMCLAPI/USTC 镜像回退".into(),
                        install_supported: true,
                        download_url: None,
                        versions: vec![l.version],
                    })
                    .unwrap_or_else(|| unavailable("fabric", "Fabric", "没有可用版本")),
                Ok(Err(e)) => unavailable("fabric", "Fabric", e.message),
                Err(_) => unavailable("fabric", "Fabric", "请求超时"),
            }
        },
        async {
            let service = FabricService::quilt(quilt_http);
            match tokio::time::timeout(Duration::from_secs(20), service.list_loaders(&game)).await {
                Ok(Ok(list)) => list
                    .iter()
                    .find(|l| l.stable)
                    .cloned()
                    .or_else(|| list.into_iter().next())
                    .map(|l| Recommendation {
                        id: "quilt".into(),
                        name: "Quilt".into(),
                        version: Some(l.version.clone()),
                        status: "available".into(),
                        note: "官方 meta 可达；安装仍需官方 Maven".into(),
                        install_supported: true,
                        download_url: None,
                        versions: vec![l.version],
                    })
                    .unwrap_or_else(|| unavailable("quilt", "Quilt", "没有可用版本")),
                Ok(Err(e)) => unavailable("quilt", "Quilt", format!("官方源不可达：{}", e.message)),
                Err(_) => unavailable("quilt", "Quilt", "官方源请求超时"),
            }
        },
        async {
            let url = format!("{BMCLAPI}/forge/minecraft/{game}");
            match get_json::<Vec<ForgeBuild>>(&forge_http, &url).await {
                Ok(list) => {
                    let json = serde_json::to_vec(&list).unwrap_or_default();
                    match select_forge(&json, &game) {
                        Ok(Some((version, download_url))) => {
                            let versions = forge_versions(&json, &game).unwrap_or_default();
                            Recommendation {
                                id: "forge".into(),
                                name: "Forge".into(),
                                version: Some(version),
                                status: "available".into(),
                                note: "后台安装 · 选择版本后由启动器完成下载与构建".into(),
                                install_supported: true,
                                download_url: Some(download_url),
                                versions,
                            }
                        }
                        _ => unavailable("forge", "Forge", "没有匹配版本"),
                    }
                }
                Err(e) => unavailable("forge", "Forge", e.message),
            }
        },
        async {
            let url = format!("{BMCLAPI}/optifine/{game}");
            match get_json::<Vec<OptiFineBuild>>(&optifine_http, &url).await {
                Ok(list) => {
                    let json = serde_json::to_vec(&list).unwrap_or_default();
                    match select_optifine(&json, &game) {
                        Ok(Some((version, download_url))) => Recommendation {
                            id: "optifine".into(),
                            name: "OptiFine".into(),
                            version: Some(version.clone()),
                            status: "available".into(),
                            note: "光影支持 · Forge 实例可在光影管理一键装入 mods/；Fabric 用户推荐 Iris".into(),
                            install_supported: false,
                            download_url: Some(download_url),
                            versions: vec![version],
                        },
                        _ => unavailable("optifine", "OptiFine", "没有匹配版本"),
                    }
                }
                Err(e) => unavailable("optifine", "OptiFine", e.message),
            }
        },
        async {
            match get_text(&neo_http, NEOFORGE_METADATA).await {
                Ok(xml) => match select_neoforge(&xml, &game) {
                    Some((version, download_url)) => {
                        let versions = neoforge_versions(&xml, &game);
                        Recommendation {
                            id: "neoforge".into(),
                            name: "NeoForge".into(),
                            version: Some(version),
                            status: "available".into(),
                            note: "后台安装 · 选择版本后由启动器完成下载与构建".into(),
                            install_supported: true,
                            download_url: Some(download_url),
                            versions,
                        }
                    }
                    None => unavailable("neoforge", "NeoForge", "没有匹配版本"),
                },
                Err(e) => unavailable("neoforge", "NeoForge", e.message),
            }
        },
    );

    let mut items = vec![fabric, quilt, forge, neoforge];
    items.push(Recommendation {
        id: "iris".into(),
        name: "Iris（光影）".into(),
        version: None,
        status: if items
            .iter()
            .any(|i| i.id == "fabric" && i.status == "recommended")
        {
            "recommended".to_string()
        } else {
            "unavailable".to_string()
        },
        note: "在 Fabric/Quilt 实例的「光影管理」中一键安装；原版无法加载光影".into(),
        install_supported: false,
        download_url: None,
        versions: Vec::new(),
    });
    items.push(optifine);

    Recommendations {
        game_version: game_version.to_string(),
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optifine_forge_requirement_parsing() {
        assert_eq!(
            optifine_forge_requirement(Some("Forge 52.0.16")).as_deref(),
            Some("52.0.16")
        );
        assert_eq!(optifine_forge_requirement(Some("Forge N/A")), None);
        assert_eq!(optifine_forge_requirement(Some("")), None);
        assert_eq!(optifine_forge_requirement(None), None);
    }

    #[test]
    fn version_gt_compares_numerically() {
        assert!(version_gt("52.1.20", "52.0.16"));
        assert!(!version_gt("52.0.16", "52.0.16"));
        assert!(!version_gt("52.0.10", "52.0.16"));
        assert!(version_gt("1.21.1", "1.21"));
        assert!(!version_gt("1.21", "1.21.0"));
    }

    #[test]
    fn picks_highest_forge_build() {
        let json = br#"[
            {"version":"52.0.1","mcversion":"1.21.1","build":52000001},
            {"version":"52.0.4","mcversion":"1.21.1","build":52000004},
            {"version":"51.0.0","mcversion":"1.21","build":51000000}
        ]"#;
        let (version, url) = select_forge(json, "1.21.1").unwrap().unwrap();
        assert_eq!(version, "52.0.4");
        assert!(url.contains("52.0.4"));
    }

    #[test]
    fn picks_normal_optifine_not_preview() {
        let json = br#"[
            {"mcversion":"1.21.1","patch":"J1","type":"HD_U","filename":"OptiFine_1.21.1_HD_U_J1.jar"},
            {"mcversion":"1.21.1","patch":"pre10","type":"HD_U_J1","filename":"preview_OptiFine_1.21.1_HD_U_J1_pre10.jar"}
        ]"#;
        let (label, url) = select_optifine(json, "1.21.1").unwrap().unwrap();
        assert_eq!(label, "HD_U J1");
        assert!(url.ends_with("/HD_U/J1"));
    }

    #[test]
    fn falls_back_to_latest_preview_when_no_stable() {
        let json = br#"[
            {"mcversion":"1.21","patch":"pre1","type":"HD_U_J1","filename":"preview_OptiFine_1.21_HD_U_J1_pre1.jar","forge":"Forge 52.0.0"},
            {"mcversion":"1.21","patch":"pre10","type":"HD_U_J1","filename":"preview_OptiFine_1.21_HD_U_J1_pre10.jar","forge":"Forge 52.0.0"}
        ]"#;
        let (label, url) = select_optifine(json, "1.21").unwrap().unwrap();
        assert_eq!(label, "HD_U_J1 pre10");
        assert!(url.ends_with("/HD_U_J1/pre10"));
    }

    #[test]
    fn lists_selectable_normal_optifine_builds() {
        let json = br#"[
            {"mcversion":"1.20.1","patch":"I5","type":"HD_U","filename":"OptiFine_1.20.1_HD_U_I5.jar","forge":"Forge 47.1.0"},
            {"mcversion":"1.20.1","patch":"I6","type":"HD_U","filename":"OptiFine_1.20.1_HD_U_I6.jar","forge":"Forge 47.2.0"},
            {"mcversion":"1.20.1","patch":"pre1","type":"HD_U_I6","filename":"preview_OptiFine_1.20.1_HD_U_I6_pre1.jar"}
        ]"#;
        let all = select_optifine_all(json, "1.20.1").unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].1, "HD_U I5");
        assert_eq!(all[1].1, "HD_U I6");
        let picked = select_optifine_label(json, "1.20.1", "HD_U I5")
            .unwrap()
            .unwrap();
        assert_eq!(picked.0, "OptiFine_1.20.1_HD_U_I5.jar");
        assert!(picked.2.ends_with("/HD_U/I5"));
        assert!(select_optifine_label(json, "1.20.1", "HD_U J1")
            .unwrap()
            .is_none());
    }

    #[test]
    fn rejects_optifine_preview_without_forge_support() {
        let json = br#"[
            {"mcversion":"1.21","patch":"pre5","type":"HD_U_J1","filename":"preview_OptiFine_1.21_HD_U_J1_pre5.jar","forge":"Forge N/A"}
        ]"#;
        assert!(select_optifine(json, "1.21").unwrap().is_none());
    }

    #[test]
    fn selects_neoforge_by_mc_prefix() {
        let xml = r#"<metadata><versioning><versions>
            <version>20.4.237</version>
            <version>21.1.219</version>
            <version>21.1.220</version>
            <version>26.3.0.95</version>
        </versions></versioning></metadata>"#;
        assert_eq!(select_neoforge(xml, "1.21.1").unwrap().0, "21.1.220");
        assert_eq!(select_neoforge(xml, "1.20.4").unwrap().0, "20.4.237");
        assert_eq!(select_neoforge(xml, "26.3").unwrap().0, "26.3.0.95");
        assert!(select_neoforge(xml, "1.22").is_none());
    }
}
