//! Read real mod metadata from local JARs.
//!
//! Matching uses declared mod ids (Fabric/Quilt/Forge/NeoForge), Modrinth
//! slugs, or exact CurseForge fingerprints. File names and title similarity are
//! never used as proof that a dependency is installed.

use crate::curseforge::{fingerprint_file, CurseForgeMatch};
use crate::error::Result;
use crate::instance::Instance;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_JARS: usize = 2000;
const MAX_JAR_BYTES: u64 = 512 * 1024 * 1024;
const MAX_META_BYTES: u64 = 512 * 1024;
const MAX_ZIP_ENTRIES: usize = 8192;
const MAX_FINGERPRINT_JARS: usize = 400;

#[derive(Debug, Clone, Default)]
pub struct InstalledMod {
    pub file_name: String,
    pub mod_ids: Vec<String>,
    pub version: Option<String>,
    pub fingerprint: Option<u32>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct InstalledIndex {
    pub mods: Vec<InstalledMod>,
    by_project: HashMap<String, usize>,
    by_mod_id: HashMap<String, usize>,
}

impl InstalledIndex {
    pub fn add_curseforge_match(&mut self, matches: &HashMap<u32, CurseForgeMatch>) {
        for (index, item) in self.mods.iter().enumerate() {
            let Some(fingerprint) = item.fingerprint else {
                continue;
            };
            let Some(found) = matches.get(&fingerprint) else {
                continue;
            };
            self.by_project
                .insert(format!("curseforge:{}", found.project_id), index);
        }
    }

    /// Find an installed mod for a platform project without guessing from the
    /// file name. Modrinth has no hash lookup here, so the declared mod id must
    /// equal the project slug (the common, documented convention).
    pub fn find(&self, source: &str, project_id: &str, slug: &str) -> Option<&InstalledMod> {
        if source.eq_ignore_ascii_case("curseforge") {
            if let Some(index) = self.by_project.get(&format!("curseforge:{project_id}")) {
                return self.mods.get(*index);
            }
            return None;
        }
        let key = slug.trim().to_ascii_lowercase();
        self.by_mod_id
            .get(&key)
            .and_then(|index| self.mods.get(*index))
    }

    pub fn len(&self) -> usize {
        self.mods.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mods.is_empty()
    }
}

/// Scan `mods/` without extracting archives. Metadata entries are size capped.
pub async fn scan_installed(instance: &Instance, with_fingerprint: bool) -> Result<InstalledIndex> {
    let dir = instance.game_dir.join("mods");
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut rd = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(InstalledIndex::default())
        }
        Err(error) => return Err(error.into()),
    };
    while let Some(entry) = rd.next_entry().await? {
        if paths.len() >= MAX_JARS {
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.ends_with(".jar") || name.ends_with(".jar.disabled")) {
            continue;
        }
        let Ok(meta) = tokio::fs::symlink_metadata(entry.path()).await else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_JAR_BYTES {
            continue;
        }
        let path = entry.path();
        paths.push(path);
    }
    let scanned = tokio::task::spawn_blocking(move || {
        let mut mods = Vec::new();
        for (position, path) in paths.into_iter().enumerate() {
            let Ok(file_name) = path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .ok_or(())
            else {
                continue;
            };
            let mut item = read_metadata(&path).unwrap_or_default();
            item.enabled = !file_name.ends_with(".disabled");
            item.file_name = file_name;
            if with_fingerprint && position < MAX_FINGERPRINT_JARS {
                item.fingerprint = fingerprint_file(&path).ok();
            }
            mods.push(item);
        }
        mods
    })
    .await
    .map_err(|error| crate::error::AppError::internal(format!("扫描模组目录失败：{error}")))?;

    let mut index = InstalledIndex::default();
    for item in scanned {
        let position = index.mods.len();
        for id in &item.mod_ids {
            index.by_mod_id.insert(id.to_ascii_lowercase(), position);
        }
        index.mods.push(item);
    }
    Ok(index)
}

fn read_metadata(path: &Path) -> Option<InstalledMod> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return None;
    }
    let mut item = InstalledMod::default();

    if let Some(text) = read_entry(&mut archive, "fabric.mod.json") {
        parse_fabric_like(&text, &mut item);
    }
    if let Some(text) = read_entry(&mut archive, "quilt.mod.json") {
        parse_fabric_like(&text, &mut item);
    }
    if let Some(text) = read_entry(&mut archive, "META-INF/mods.toml") {
        parse_mods_toml(&text, &mut item);
    }
    if item.mod_ids.is_empty() {
        if let Some(text) = read_entry(&mut archive, "META-INF/neoforge.mods.toml") {
            parse_mods_toml(&text, &mut item);
        }
    }
    if item.mod_ids.is_empty() {
        None
    } else {
        item.mod_ids.sort();
        item.mod_ids.dedup();
        Some(item)
    }
}

fn read_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<String> {
    let entry = archive.by_name(name).ok()?;
    if entry.size() > MAX_META_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    entry.take(MAX_META_BYTES).read_to_end(&mut bytes).ok()?;
    String::from_utf8(bytes).ok()
}

fn parse_fabric_like(text: &str, item: &mut InstalledMod) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    // quilt.mod.json nests the block under `quilt_loader`.
    let root = value
        .get("quilt_loader")
        .filter(|inner| inner.is_object())
        .unwrap_or(&value);
    if let Some(id) = root.get("id").and_then(|v| v.as_str()) {
        push_id(item, id);
    }
    if item.version.is_none() {
        if let Some(version) = root.get("version").and_then(|v| v.as_str()) {
            item.version = Some(version.to_string());
        }
    }
    if let Some(provides) = root.get("provides").and_then(|v| v.as_array()) {
        for entry in provides {
            match entry {
                serde_json::Value::String(id) => push_id(item, id),
                serde_json::Value::Object(map) => {
                    if let Some(id) = map.get("id").and_then(|v| v.as_str()) {
                        push_id(item, id);
                    }
                }
                _ => {}
            }
        }
    }
}

fn parse_mods_toml(text: &str, item: &mut InstalledMod) {
    let Ok(value) = toml::from_str::<toml::Value>(text) else {
        return;
    };
    let Some(mods) = value.get("mods").and_then(|v| v.as_array()) else {
        return;
    };
    for entry in mods {
        if let Some(id) = entry.get("modId").and_then(|v| v.as_str()) {
            push_id(item, id);
        }
        if item.version.is_none() {
            if let Some(version) = entry.get("version").and_then(|v| v.as_str()) {
                item.version = Some(version.to_string());
            }
        }
    }
}

fn push_id(item: &mut InstalledMod, id: &str) {
    let trimmed = id.trim();
    if !trimmed.is_empty() && trimmed.len() <= 128 {
        item.mod_ids.push(trimmed.to_ascii_lowercase());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar_with(entries: &[(&str, &str)]) -> PathBuf {
        let dir = tempfile::tempdir().unwrap().keep();
        let path = dir.join("test.jar");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn reads_fabric_and_provides() {
        let path = jar_with(&[(
            "fabric.mod.json",
            r#"{"id":"fabric-api","version":"0.100.0","provides":["fabric-api-base"]}"#,
        )]);
        let item = read_metadata(&path).unwrap();
        assert!(item.mod_ids.contains(&"fabric-api".into()));
        assert!(item.mod_ids.contains(&"fabric-api-base".into()));
        assert_eq!(item.version.as_deref(), Some("0.100.0"));
    }

    #[test]
    fn reads_forge_mods_toml() {
        let path = jar_with(&[(
            "META-INF/mods.toml",
            "modLoader=\"javafml\"\n[[mods]]\nmodId=\"examplemod\"\nversion=\"1.2.3\"\n",
        )]);
        let item = read_metadata(&path).unwrap();
        assert_eq!(item.mod_ids, vec!["examplemod"]);
        assert_eq!(item.version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn rejects_jar_without_metadata() {
        let path = jar_with(&[("assets/readme.txt", "hello")]);
        assert!(read_metadata(&path).is_none());
    }

    #[test]
    fn index_matches_modrinth_slug_only_by_declared_id() {
        let mut index = InstalledIndex::default();
        index.mods.push(InstalledMod {
            file_name: "fabric-api.jar".into(),
            mod_ids: vec!["fabric-api".into()],
            version: Some("1".into()),
            fingerprint: None,
            enabled: true,
        });
        index.by_mod_id.insert("fabric-api".into(), 0);
        assert!(index.find("modrinth", "P7dR8mSH", "fabric-api").is_some());
        assert!(index
            .find("modrinth", "P7dR8mSH", "fabric-api-extra")
            .is_none());
    }
}
