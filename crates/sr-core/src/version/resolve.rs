use super::model::{
    Argument, Arguments, AssetIndexRef, FileDownloadInfo, Library, LibraryName, LoggingConfig,
    VersionDownloads, VersionJson,
};
use super::rules::{library_allowed, RuleContext};
use crate::error::{AppError, ErrorCode, Result};
use std::collections::HashSet;

pub const MAX_INHERITANCE_DEPTH: usize = 16;

/// Resolve `inheritsFrom` chains. `lookup` must return metadata for a version id
/// (installed file or remote). Cycles and excessive depth are hard errors.
pub fn resolve_inheritance<F>(child: VersionJson, mut lookup: F) -> Result<VersionJson>
where
    F: FnMut(&str) -> Result<VersionJson>,
{
    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(child.id.clone());
    let mut current = child;
    let mut depth = 0usize;

    while let Some(parent_id) = current.inherits_from.clone() {
        depth += 1;
        if depth > MAX_INHERITANCE_DEPTH {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                "版本继承层级过深，可能损坏",
            ));
        }
        if !seen.insert(parent_id.clone()) {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                format!("版本继承出现循环：{parent_id}"),
            ));
        }
        let parent = lookup(&parent_id).map_err(|e| {
            AppError::new(
                ErrorCode::VersionInvalid,
                format!("缺少父版本 {parent_id}，无法解析继承关系"),
            )
            .with_detail(e.to_string())
        })?;
        current = merge_parent(&parent, &current);
    }
    Ok(current)
}

/// Merge a parent into a child. Child scalar fields win; libraries are unioned
/// with child overrides; argument lists are concatenated parent-first.
pub fn merge_parent(parent: &VersionJson, child: &VersionJson) -> VersionJson {
    let mut merged = parent.clone();

    merged.id = child.id.clone();
    merged.version_type = child.version_type.clone().or(parent.version_type.clone());
    merged.inherits_from = parent.inherits_from.clone();
    merged.main_class = child.main_class.clone().or(parent.main_class.clone());
    merged.assets = child.assets.clone().or(parent.assets.clone());
    merged.asset_index = child.asset_index.clone().or(parent.asset_index.clone());
    merged.jar = child.jar.clone().or(parent.jar.clone());
    merged.java_version = child.java_version.clone().or(parent.java_version.clone());
    merged.logging = child.logging.clone().or(parent.logging.clone());
    merged.compliance_level = child.compliance_level.or(parent.compliance_level);
    merged.minecraft_arguments = child
        .minecraft_arguments
        .clone()
        .or(parent.minecraft_arguments.clone());

    merged.downloads = match (&parent.downloads, &child.downloads) {
        (None, None) => None,
        (Some(p), None) => Some(p.clone()),
        (None, Some(c)) => Some(c.clone()),
        (Some(p), Some(c)) => Some(VersionDownloads {
            client: c.client.clone().or(p.client.clone()),
            server: c.server.clone().or(p.server.clone()),
            client_mappings: c.client_mappings.clone().or(p.client_mappings.clone()),
            server_mappings: c.server_mappings.clone().or(p.server_mappings.clone()),
        }),
    };

    merged.libraries = Some(merge_libraries(
        parent.libraries.as_deref().unwrap_or_default(),
        child.libraries.as_deref().unwrap_or_default(),
    ));

    merged.arguments = merge_arguments(parent.arguments.as_ref(), child.arguments.as_ref());
    merged.extra = parent.extra.clone();
    for (k, v) in &child.extra {
        merged.extra.insert(k.clone(), v.clone());
    }
    merged
}

fn merge_libraries(parent: &[Library], child: &[Library]) -> Vec<Library> {
    let mut out: Vec<Library> = Vec::with_capacity(parent.len() + child.len());
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for lib in parent.iter().chain(child.iter()) {
        let key = LibraryName::parse(&lib.name)
            .map(|n| n.key())
            .unwrap_or_else(|| lib.name.clone());
        if let Some(pos) = index.get(&key).copied() {
            out[pos] = lib.clone();
        } else {
            index.insert(key, out.len());
            out.push(lib.clone());
        }
    }
    out
}

fn merge_arguments(parent: Option<&Arguments>, child: Option<&Arguments>) -> Option<Arguments> {
    match (parent, child) {
        (None, None) => None,
        (Some(p), None) => Some(p.clone()),
        (None, Some(c)) => Some(c.clone()),
        (Some(p), Some(c)) => Some(Arguments {
            game: p.game.iter().chain(c.game.iter()).cloned().collect(),
            jvm: p.jvm.iter().chain(c.jvm.iter()).cloned().collect(),
        }),
    }
}

/// A library resolved for the current platform, with the exact file to fetch
/// and whether it belongs on the classpath or in the natives directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveLibrary {
    pub name: String,
    pub path: String,
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub is_native: bool,
    pub extract_exclude: Vec<String>,
}

impl VersionJson {
    pub fn main_class(&self) -> Result<&str> {
        self.main_class
            .as_deref()
            .ok_or_else(|| AppError::new(ErrorCode::VersionInvalid, "版本元数据缺少 mainClass"))
    }

    pub fn client_download(&self) -> Result<&FileDownloadInfo> {
        self.downloads
            .as_ref()
            .and_then(|d| d.client.as_ref())
            .ok_or_else(|| AppError::new(ErrorCode::VersionInvalid, "版本元数据缺少客户端下载信息"))
    }

    pub fn asset_index(&self) -> Result<&AssetIndexRef> {
        self.asset_index
            .as_ref()
            .ok_or_else(|| AppError::new(ErrorCode::VersionInvalid, "版本元数据缺少资源索引"))
    }

    pub fn java_major(&self) -> u32 {
        self.java_version
            .as_ref()
            .and_then(|j| j.major_version)
            .unwrap_or(8)
    }

    pub fn logging(&self) -> Option<&LoggingConfig> {
        self.logging.as_ref()
    }

    /// Effective libraries for a platform context.
    pub fn effective_libraries(&self, ctx: &RuleContext) -> Vec<EffectiveLibrary> {
        let mut out = Vec::new();
        for lib in self.libraries.as_deref().unwrap_or_default() {
            if !library_allowed(&lib.rules, ctx) {
                continue;
            }
            let exclude = lib
                .extract
                .as_ref()
                .map(|e| e.exclude.clone())
                .unwrap_or_default();

            if let Some(natives) = &lib.natives {
                let os = ctx.os.as_minecraft();
                let classifier_raw = natives
                    .get(os)
                    .or_else(|| natives.get(&format!("natives-{os}")));
                let Some(classifier_raw) = classifier_raw else {
                    continue;
                };
                let classifier = classifier_raw.replace(
                    "${arch}",
                    if ctx.arch_name() == "x86" { "32" } else { "64" },
                );
                if let Some(artifact) = lib
                    .downloads
                    .as_ref()
                    .and_then(|d| d.classifiers.as_ref())
                    .and_then(|c| c.get(&classifier))
                {
                    let path = artifact.path.clone().unwrap_or_else(|| {
                        let mut n = LibraryName::parse(&lib.name).unwrap_or(LibraryName {
                            group: "unknown".into(),
                            artifact: lib.name.clone(),
                            version: "0".into(),
                            classifier: None,
                            extension: "jar".into(),
                        });
                        n.classifier = Some(classifier.clone());
                        n.path()
                    });
                    out.push(EffectiveLibrary {
                        name: lib.name.clone(),
                        path,
                        url: artifact.url.clone(),
                        sha1: artifact.sha1.clone(),
                        size: artifact.size,
                        is_native: true,
                        extract_exclude: exclude.clone(),
                    });
                }
                continue;
            }

            let artifact = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref());
            let parsed = LibraryName::parse(&lib.name);
            let path = artifact
                .and_then(|a| a.path.clone())
                .or_else(|| parsed.as_ref().map(|p| p.path()))
                .unwrap_or_else(|| format!("{}.jar", lib.name.replace(':', "-")));
            let url = artifact.and_then(|a| a.url.clone()).or_else(|| {
                lib.url
                    .as_ref()
                    .map(|base| format!("{}/{}", base.trim_end_matches('/'), path))
            });
            out.push(EffectiveLibrary {
                name: lib.name.clone(),
                path,
                url,
                sha1: artifact
                    .and_then(|a| a.sha1.clone())
                    .or_else(|| lib.sha1.clone()),
                size: artifact.and_then(|a| a.size).or(lib.size),
                is_native: false,
                extract_exclude: exclude,
            });
        }
        out
    }

    pub fn classpath_entries(&self, ctx: &RuleContext) -> Vec<String> {
        self.effective_libraries(ctx)
            .into_iter()
            .filter(|l| !l.is_native)
            .map(|l| l.path)
            .collect()
    }

    pub fn all_argument_strings(&self, ctx: &RuleContext) -> (Vec<String>, Vec<String>) {
        let args = self.arguments.as_ref();
        let game = args
            .map(|a| {
                a.game
                    .iter()
                    .flat_map(|x| super::rules::argument_values(x, ctx).into_iter().cloned())
                    .collect()
            })
            .unwrap_or_default();
        let jvm = args
            .map(|a| {
                a.jvm
                    .iter()
                    .flat_map(|x| super::rules::argument_values(x, ctx).into_iter().cloned())
                    .collect()
            })
            .unwrap_or_default();
        (game, jvm)
    }

    /// True if the version uses the pre-1.13 `minecraftArguments` string.
    pub fn uses_legacy_arguments(&self) -> bool {
        self.arguments.is_none() && self.minecraft_arguments.is_some()
    }

    pub fn has_client_jar(&self) -> bool {
        self.downloads
            .as_ref()
            .and_then(|d| d.client.as_ref())
            .is_some()
    }

    /// The version whose `versions/<id>/<id>.jar` holds the client. Loader
    /// profiles inherit the base version's jar.
    pub fn jar_id(&self) -> String {
        self.jar.clone().unwrap_or_else(|| self.id.clone())
    }

    /// Debug helper: count argument entries (useful in diagnostics).
    pub fn argument_count(&self) -> (usize, usize) {
        self.arguments
            .as_ref()
            .map(|a| (a.game.len(), a.jvm.len()))
            .unwrap_or((0, 0))
    }

    /// Ensure the resolved metadata is usable for launching.
    pub fn validate_for_launch(&self) -> Result<()> {
        self.main_class()?;
        self.client_download()?;
        self.asset_index()?;
        if self.libraries.as_deref().unwrap_or_default().is_empty() {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                "版本元数据没有库依赖",
            ));
        }
        if !self.uses_legacy_arguments() && self.arguments.is_none() {
            return Err(AppError::new(
                ErrorCode::VersionInvalid,
                "版本元数据既没有 arguments 也没有 minecraftArguments",
            ));
        }
        Ok(())
    }
}

/// A single game argument from a modern list. Used by the launcher builder.
pub fn argument_is_flag(arg: &Argument) -> bool {
    match arg {
        Argument::Simple(s) => s.starts_with('-'),
        Argument::Conditional { value, .. } => {
            value.iter().next().is_some_and(|s| s.starts_with('-'))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v(value: serde_json::Value) -> VersionJson {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn child_scalars_override_parent() {
        let parent = v(json!({
            "id": "parent", "mainClass": "p.Main", "assets": "legacy",
            "downloads": {"client": {"sha1":"a","size":1,"url":"https://x/client.jar"}},
            "javaVersion": {"majorVersion": 8},
            "libraries": [{"name":"g:a:1"}]
        }));
        let child = v(json!({"id":"child","inheritsFrom":"parent","mainClass":"c.Main"}));
        let m = merge_parent(&parent, &child);
        assert_eq!(m.id, "child");
        assert_eq!(m.main_class.as_deref(), Some("c.Main"));
        assert_eq!(m.assets.as_deref(), Some("legacy"));
        assert_eq!(m.java_major(), 8);
        assert_eq!(m.libraries.unwrap().len(), 1);
    }

    #[test]
    fn libraries_are_unioned_and_child_wins() {
        let parent = v(json!({"id":"p","libraries":[
            {"name":"g:a:1"},{"name":"g:b:1"}
        ]}));
        let child = v(json!({"id":"c","libraries":[{"name":"g:a:2"},{"name":"g:c:1"}]}));
        let m = merge_parent(&parent, &child);
        let libs = m.libraries.unwrap();
        assert_eq!(libs.len(), 3);
        assert!(libs.iter().any(|l| l.name == "g:a:2"));
        assert!(!libs.iter().any(|l| l.name == "g:a:1"));
    }

    #[test]
    fn arguments_concatenate_parent_first() {
        let parent = v(json!({"id":"p","arguments":{"game":["--a"],"jvm":["-Xmx1G"]}}));
        let child = v(json!({"id":"c","arguments":{"game":["--b"],"jvm":["-Dfoo=1"]}}));
        let m = merge_parent(&parent, &child);
        let a = m.arguments.unwrap();
        assert_eq!(a.game.len(), 2);
        assert_eq!(a.jvm.len(), 2);
    }

    #[test]
    fn inheritance_chain_resolves() {
        let child = v(json!({"id":"c","inheritsFrom":"b"}));
        let resolved = resolve_inheritance(child, |id| match id {
            "b" => Ok(v(json!({"id":"b","inheritsFrom":"a","mainClass":"b.Main"}))),
            "a" => Ok(v(json!({"id":"a","mainClass":"a.Main","assets":"legacy"}))),
            _ => Err(AppError::not_found("missing")),
        })
        .unwrap();
        assert_eq!(resolved.id, "c");
        assert_eq!(resolved.main_class.as_deref(), Some("b.Main"));
        assert_eq!(resolved.assets.as_deref(), Some("legacy"));
    }

    #[test]
    fn inheritance_cycle_is_rejected() {
        let child = v(json!({"id":"a","inheritsFrom":"b"}));
        let err = resolve_inheritance(child, |id| match id {
            "b" => Ok(v(json!({"id":"b","inheritsFrom":"a"}))),
            _ => Err(AppError::not_found("missing")),
        })
        .unwrap_err();
        assert!(err.message.contains("循环"));
    }

    #[test]
    fn natives_are_selected_and_not_on_classpath() {
        let ver = v(json!({
            "id":"x","mainClass":"M","assetIndex":{"id":"1","url":"https://x"},
            "downloads":{"client":{"sha1":"a","size":1,"url":"https://x/c.jar"}},
            "libraries":[
                {"name":"org.lwjgl:lwjgl:3.3.2",
                 "downloads":{"artifact":{"path":"org/lwjgl/lwjgl/3.3.2/lwjgl-3.3.2.jar","url":"https://x/l.jar","sha1":"b","size":2}}},
                {"name":"org.lwjgl:lwjgl:3.3.2:natives-macos",
                 "natives":{"osx":"natives-macos"},
                 "downloads":{"classifiers":{"natives-macos":{"path":"org/lwjgl/lwjgl/3.3.2/lwjgl-3.3.2-natives-macos.jar","url":"https://x/n.jar","sha1":"c","size":3}}},
                 "rules":[{"action":"allow","os":{"name":"osx"}}],
                 "extract":{"exclude":["META-INF/"]}}
            ]
        }));
        let mac = RuleContext {
            os: super::super::rules::OsName::Osx,
            arch: "aarch64".into(),
            os_version: "14".into(),
            features: Default::default(),
        };
        let libs = ver.effective_libraries(&mac);
        assert_eq!(libs.len(), 2);
        assert!(libs
            .iter()
            .any(|l| l.is_native && l.path.contains("natives-macos")));
        let cp = ver.classpath_entries(&mac);
        assert_eq!(cp, vec!["org/lwjgl/lwjgl/3.3.2/lwjgl-3.3.2.jar"]);
        let win = RuleContext {
            os: super::super::rules::OsName::Windows,
            arch: "x86_64".into(),
            os_version: "10".into(),
            features: Default::default(),
        };
        assert!(!ver.effective_libraries(&win).iter().any(|l| l.is_native));
    }

    #[test]
    fn legacy_library_url_is_constructed() {
        let ver = v(json!({"id":"x","libraries":[
            {"name":"g.a:b:1.0","url":"https://legacy.example/maven"}
        ]}));
        let libs = ver.effective_libraries(&RuleContext::default());
        assert_eq!(
            libs[0].url.as_deref(),
            Some("https://legacy.example/maven/g/a/b/1.0/b-1.0.jar")
        );
    }
}
