use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<ManifestVersion>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub release_time: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub compliance_level: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(rename = "type", default)]
    pub version_type: Option<String>,
    #[serde(default)]
    pub inherits_from: Option<String>,
    #[serde(default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default)]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default)]
    pub downloads: Option<VersionDownloads>,
    #[serde(default)]
    pub libraries: Option<Vec<Library>>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub java_version: Option<JavaVersionReq>,
    #[serde(default)]
    pub logging: Option<LoggingConfig>,
    #[serde(default)]
    pub jar: Option<String>,
    #[serde(default)]
    pub compliance_level: Option<i32>,
    /// Preserved so future fields survive a load/save cycle and can be
    /// inspected for diagnostics.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexRef {
    pub id: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub total_size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionDownloads {
    #[serde(default)]
    pub client: Option<FileDownloadInfo>,
    #[serde(default)]
    pub server: Option<FileDownloadInfo>,
    #[serde(default)]
    pub client_mappings: Option<FileDownloadInfo>,
    #[serde(default)]
    pub server_mappings: Option<FileDownloadInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileDownloadInfo {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    /// classifier key -> maven classifier, e.g. {"natives-osx": "natives-osx"}
    #[serde(default)]
    pub natives: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub extract: Option<Extract>,
    /// Legacy library entries may carry a base url instead of downloads.
    #[serde(default)]
    pub url: Option<String>,
    /// Fabric-style metadata puts hashes at the top level.
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: Option<BTreeMap<String, Artifact>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Argument {
    Simple(String),
    Conditional {
        #[serde(default)]
        rules: Vec<Rule>,
        value: StringOrVec,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum StringOrVec {
    One(String),
    Many(Vec<String>),
}

impl StringOrVec {
    pub fn iter(&self) -> Box<dyn Iterator<Item = &String> + '_> {
        match self {
            StringOrVec::One(s) => Box::new(std::iter::once(s)),
            StringOrVec::Many(v) => Box::new(v.iter()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<BTreeMap<String, bool>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersionReq {
    #[serde(default)]
    pub component: Option<String>,
    #[serde(default)]
    pub major_version: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    #[serde(default)]
    pub client: Option<LoggingClient>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingClient {
    #[serde(default)]
    pub argument: Option<String>,
    #[serde(default)]
    pub file: Option<FileDownloadInfo>,
    #[serde(rename = "type", default)]
    pub log_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetIndexFile {
    #[serde(default)]
    pub objects: BTreeMap<String, AssetObject>,
    #[serde(default, rename = "virtual")]
    pub is_virtual: Option<bool>,
    #[serde(default)]
    pub map_to_resources: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

impl VersionJson {
    pub fn parse(bytes: &[u8]) -> crate::error::Result<Self> {
        let v: VersionJson = serde_json::from_slice(bytes).map_err(|e| {
            crate::error::AppError::new(
                crate::error::ErrorCode::VersionInvalid,
                "版本元数据格式无效",
            )
            .with_detail(e.to_string())
        })?;
        if v.id.trim().is_empty() {
            return Err(crate::error::AppError::new(
                crate::error::ErrorCode::VersionInvalid,
                "版本元数据缺少 id",
            ));
        }
        Ok(v)
    }
}

/// Maven coordinate of a library name: `group:artifact:version[:classifier][@ext]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryName {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub extension: String,
}

impl LibraryName {
    pub fn parse(raw: &str) -> Option<Self> {
        let (coord, ext) = match raw.rsplit_once('@') {
            Some((c, e)) if !e.is_empty() => (c, e.to_string()),
            _ => (raw, "jar".to_string()),
        };
        let parts: Vec<&str> = coord.split(':').collect();
        if parts.len() < 3 || parts.iter().any(|p| p.is_empty()) {
            return None;
        }
        Some(Self {
            group: parts[0].to_string(),
            artifact: parts[1].to_string(),
            version: parts[2].to_string(),
            classifier: parts.get(3).map(|s| s.to_string()),
            extension: ext,
        })
    }

    pub fn path(&self) -> String {
        let file = match &self.classifier {
            Some(c) => format!(
                "{}-{}-{}.{}",
                self.artifact, self.version, c, self.extension
            ),
            None => format!("{}-{}.{}", self.artifact, self.version, self.extension),
        };
        format!(
            "{}/{}/{}/{}",
            self.group.replace('.', "/"),
            self.artifact,
            self.version,
            file
        )
    }

    /// Override key: same group/artifact/classifier means "same library slot".
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.group,
            self.artifact,
            self.classifier.clone().unwrap_or_default()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_names() {
        let n = LibraryName::parse("net.minecraft:client:1.20.4").unwrap();
        assert_eq!(n.path(), "net/minecraft/client/1.20.4/client-1.20.4.jar");
        assert_eq!(n.key(), "net.minecraft:client:");

        let n = LibraryName::parse("org.lwjgl:lwjgl:3.3.2:natives-macos").unwrap();
        assert_eq!(
            n.path(),
            "org/lwjgl/lwjgl/3.3.2/lwjgl-3.3.2-natives-macos.jar"
        );
        assert_eq!(n.key(), "org.lwjgl:lwjgl:natives-macos");

        let n = LibraryName::parse("de.oceanlabs.mcp:mcp_config:1.20.1@zip").unwrap();
        assert_eq!(n.extension, "zip");
        assert!(n.path().ends_with("mcp_config-1.20.1.zip"));
    }

    #[test]
    fn rejects_malformed_library_names() {
        for bad in ["", "a", "a:b", "a::1", ":b:1"] {
            assert!(LibraryName::parse(bad).is_none(), "{bad:?}");
        }
    }
}
