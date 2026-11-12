//! Skin library: local storage, validation, Mojang lookup and applying.
//!
//! Two application paths exist and they are not equivalent:
//! - Microsoft accounts: the PNG is uploaded through the official Minecraft
//!   profile API, so the skin (and slim/classic model) is visible to everyone.
//! - Offline profiles: there is no server-side identity. The launcher can only
//!   inject a client-side resource pack that replaces the default skin
//!   textures. It is visible on this machine only and cannot change the arm
//!   model, which is chosen by the client from the profile UUID.

use crate::error::{AppError, ErrorCode, Result};
use crate::paths::{AppPaths, GamePaths};
use crate::util;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

/// Reject absurdly large uploads before decoding.
pub const MAX_SKIN_BYTES: usize = 2 * 1024 * 1024;
const COMPAT_SIZE: u32 = 64;
/// Name of the client-side resource pack used for offline profiles.
pub const LOCAL_PACK_NAME: &str = "SRLocalSkin.zip";
const LOCAL_PACK_ENTRY: &str = "file/SRLocalSkin.zip";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkinModel {
    Classic,
    Slim,
}

impl SkinModel {
    pub fn api_name(self) -> &'static str {
        match self {
            SkinModel::Classic => "classic",
            SkinModel::Slim => "slim",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkinSource {
    /// Extracted from the installed client jar (official default characters).
    Builtin,
    /// Imported from a local PNG.
    Local,
    /// Fetched from the Mojang profile API by player name.
    Mojang,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skin {
    pub id: String,
    pub name: String,
    /// The model the texture was authored for. Accounts may override it.
    pub model: SkinModel,
    pub source: SkinSource,
    /// Original dimensions (64 or 128).
    pub width: u32,
    pub height: u32,
    /// SHA-1 of the normalized 64x64 RGBA pixels, used for de-duplication.
    pub sha1: String,
    /// Original premium player name when `source` is Mojang.
    #[serde(default)]
    pub owner: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkinDb {
    #[serde(default)]
    skins: Vec<Skin>,
    #[serde(default)]
    builtin_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinView {
    #[serde(flatten)]
    pub skin: Skin,
    /// `data:image/png;base64,...` of the normalized texture for previews.
    pub preview: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinListView {
    pub skins: Vec<SkinView>,
    pub builtin_version: Option<String>,
    pub builtin_hint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MojangEndpoints {
    pub profile: String,
    pub session: String,
}

impl Default for MojangEndpoints {
    fn default() -> Self {
        Self {
            profile: "https://api.mojang.com/users/profiles/minecraft".into(),
            session: "https://sessionserver.mojang.com/session/minecraft/profile".into(),
        }
    }
}

#[derive(Clone)]
pub struct SkinLibrary {
    paths: AppPaths,
    http: reqwest::Client,
    endpoints: MojangEndpoints,
    texture_hosts: Vec<String>,
}

#[derive(Debug)]
struct ProcessedSkin {
    width: u32,
    height: u32,
    sha1: String,
    original: Vec<u8>,
    compat: Vec<u8>,
}

impl SkinLibrary {
    pub fn new(paths: AppPaths, http: reqwest::Client) -> Self {
        Self {
            paths,
            http,
            endpoints: MojangEndpoints::default(),
            texture_hosts: vec!["textures.minecraft.net".into()],
        }
    }

    /// Test hook: override Mojang endpoints and the allowed texture hosts.
    pub fn with_endpoints(
        mut self,
        endpoints: MojangEndpoints,
        texture_hosts: Vec<String>,
    ) -> Self {
        self.endpoints = endpoints;
        self.texture_hosts = texture_hosts;
        self
    }

    fn db_path(&self) -> PathBuf {
        self.paths.skins_file()
    }

    fn dir(&self) -> PathBuf {
        self.paths.skins_dir()
    }

    fn original_path(&self, id: &str) -> PathBuf {
        self.dir().join(format!("{id}.png"))
    }

    fn compat_path(&self, id: &str) -> PathBuf {
        self.dir().join(format!("{id}.64.png"))
    }

    async fn load_db(&self) -> Result<SkinDb> {
        Ok(util::read_json::<SkinDb>(&self.db_path())
            .await?
            .unwrap_or_default())
    }

    async fn save_db(&self, db: &SkinDb) -> Result<()> {
        util::write_json(&self.db_path(), db).await
    }

    pub async fn list(&self, game: &GamePaths) -> Result<SkinListView> {
        let (builtin_version, builtin_hint) = self.ensure_defaults(game).await?;
        let db = self.load_db().await?;
        let mut views = Vec::new();
        for skin in &db.skins {
            match self.view(skin).await {
                Ok(v) => views.push(v),
                Err(e) => {
                    tracing::warn!(skin = %skin.id, error = %e, "skin file missing");
                }
            }
        }
        views.sort_by(|a, b| {
            let rank = |s: &Skin| match s.source {
                SkinSource::Builtin => 0,
                SkinSource::Local => 1,
                SkinSource::Mojang => 2,
            };
            rank(&a.skin)
                .cmp(&rank(&b.skin))
                .then_with(|| match (a.skin.source, b.skin.source) {
                    (SkinSource::Builtin, SkinSource::Builtin) => a
                        .skin
                        .model
                        .api_name()
                        .cmp(b.skin.model.api_name())
                        .then_with(|| a.skin.name.cmp(&b.skin.name)),
                    _ => b.skin.created_at.cmp(&a.skin.created_at),
                })
        });
        Ok(SkinListView {
            skins: views,
            builtin_version,
            builtin_hint,
        })
    }

    pub async fn get(&self, id: &str) -> Result<Skin> {
        if id.is_empty()
            || id.len() > 64
            || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(AppError::invalid_input("皮肤 ID 非法"));
        }
        let db = self.load_db().await?;
        db.skins
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| AppError::not_found("皮肤不存在"))
    }

    pub async fn view(&self, skin: &Skin) -> Result<SkinView> {
        let bytes = tokio::fs::read(self.compat_path(&skin.id)).await?;
        let preview = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );
        Ok(SkinView {
            skin: skin.clone(),
            preview,
        })
    }

    pub async fn compat_bytes(&self, skin: &Skin) -> Result<Vec<u8>> {
        Ok(tokio::fs::read(self.compat_path(&skin.id)).await?)
    }

    /// Import a PNG. Accepts 64x64 and 128x128; 128x128 is downscaled to the
    /// vanilla-compatible 64x64 copy while the original is preserved.
    pub async fn import_bytes(
        &self,
        file_name: &str,
        bytes: &[u8],
        model: SkinModel,
    ) -> Result<Skin> {
        if bytes.is_empty() {
            return Err(AppError::invalid_input("皮肤文件为空"));
        }
        if bytes.len() > MAX_SKIN_BYTES {
            return Err(AppError::invalid_input("皮肤文件超过 2 MB"));
        }
        let processed = process_png(bytes)?;
        let mut db = self.load_db().await?;
        if let Some(existing) = db.skins.iter().find(|s| s.sha1 == processed.sha1) {
            return Ok(existing.clone());
        }
        let id = format!("skin-{}", util::short_id());
        util::ensure_dir(&self.dir()).await?;
        util::atomic_write(&self.original_path(&id), &processed.original).await?;
        util::atomic_write(&self.compat_path(&id), &processed.compat).await?;
        let skin = Skin {
            id,
            name: clean_name(file_name),
            model,
            source: SkinSource::Local,
            width: processed.width,
            height: processed.height,
            sha1: processed.sha1,
            owner: None,
            created_at: util::now_rfc3339(),
        };
        db.skins.push(skin.clone());
        self.save_db(&db).await?;
        Ok(skin)
    }

    pub async fn import_path(&self, path: &Path, model: SkinModel) -> Result<Skin> {
        let meta = tokio::fs::metadata(path).await.map_err(|e| {
            AppError::not_found(format!("无法读取皮肤文件：{}", path.display()))
                .with_detail(e.to_string())
        })?;
        if meta.len() as usize > MAX_SKIN_BYTES {
            return Err(AppError::invalid_input("皮肤文件超过 2 MB"));
        }
        let bytes = tokio::fs::read(path).await?;
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "skin.png".into());
        self.import_bytes(&name, &bytes, model).await
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let mut db = self.load_db().await?;
        let idx = db
            .skins
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| AppError::not_found("皮肤不存在"))?;
        if db.skins[idx].source == SkinSource::Builtin {
            return Err(AppError::unsupported("内置皮肤提取自游戏客户端，无法删除"));
        }
        db.skins.remove(idx);
        self.save_db(&db).await?;
        let _ = tokio::fs::remove_file(self.original_path(id)).await;
        let _ = tokio::fs::remove_file(self.compat_path(id)).await;
        Ok(())
    }

    /// Look up a premium player's current skin through the Mojang APIs.
    pub async fn fetch_mojang(&self, username: &str) -> Result<Skin> {
        let name = username.trim();
        if !(3..=16).contains(&name.chars().count())
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(AppError::invalid_input(
                "正版玩家名称需为 3-16 位字母、数字或下划线",
            ));
        }
        let profile_url = format!("{}/{}", self.endpoints.profile, name);
        let resp = self
            .http
            .get(&profile_url)
            .timeout(std::time::Duration::from_secs(20))
            .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND
            || resp.status() == reqwest::StatusCode::NO_CONTENT
        {
            return Err(AppError::not_found("未找到该正版玩家"));
        }
        if !resp.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Http,
                format!("Mojang 用户查询返回 HTTP {}", resp.status().as_u16()),
            ));
        }
        let profile: serde_json::Value = resp.json().await?;
        let uuid = profile
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::internal("Mojang 返回的用户信息缺少 id"))?;
        let resolved_name = profile
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(name)
            .to_string();

        let session_url = format!("{}/{}", self.endpoints.session, uuid);
        let resp = self
            .http
            .get(&session_url)
            .timeout(std::time::Duration::from_secs(20))
            .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Http,
                format!("Mojang 档案查询返回 HTTP {}", resp.status().as_u16()),
            ));
        }
        let session: serde_json::Value = resp.json().await?;
        let encoded = session
            .get("properties")
            .and_then(|v| v.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("textures"))
            })
            .and_then(|p| p.get("value"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::not_found("该玩家没有公开的皮肤数据"))?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| AppError::internal("Mojang 皮肤数据编码无效"))?;
        let textures: serde_json::Value = serde_json::from_slice(&decoded)
            .map_err(|_| AppError::internal("Mojang 皮肤数据格式无效"))?;
        let skin_node = textures
            .pointer("/textures/SKIN")
            .ok_or_else(|| AppError::not_found("该玩家当前使用默认皮肤"))?;
        let url = skin_node
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::not_found("该玩家当前使用默认皮肤"))?;
        let model = match skin_node
            .pointer("/metadata/model")
            .and_then(|v| v.as_str())
        {
            Some("slim") => SkinModel::Slim,
            _ => SkinModel::Classic,
        };

        let url = normalize_texture_url(url);
        let parsed = reqwest::Url::parse(&url).map_err(|_| AppError::internal("皮肤地址无效"))?;
        let host = parsed.host_str().unwrap_or_default();
        if !self.texture_hosts.iter().any(|h| h == host) {
            return Err(AppError::invalid_input("皮肤地址不在受信任的域名内"));
        }
        let resp = self
            .http
            .get(&url)
            .timeout(std::time::Duration::from_secs(20))
            .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(AppError::new(
                ErrorCode::Http,
                format!("皮肤下载返回 HTTP {}", resp.status().as_u16()),
            ));
        }
        let bytes = resp.bytes().await?;
        if bytes.len() > MAX_SKIN_BYTES {
            return Err(AppError::invalid_input("皮肤文件超过 2 MB"));
        }
        let processed = process_png(&bytes)?;
        let mut db = self.load_db().await?;
        if let Some(existing) = db
            .skins
            .iter()
            .find(|s| s.source == SkinSource::Mojang && s.sha1 == processed.sha1)
        {
            return Ok(existing.clone());
        }
        let id = format!("skin-{}", util::short_id());
        util::ensure_dir(&self.dir()).await?;
        util::atomic_write(&self.original_path(&id), &processed.original).await?;
        util::atomic_write(&self.compat_path(&id), &processed.compat).await?;
        let skin = Skin {
            id,
            name: resolved_name.clone(),
            model,
            source: SkinSource::Mojang,
            width: processed.width,
            height: processed.height,
            sha1: processed.sha1,
            owner: Some(resolved_name),
            created_at: util::now_rfc3339(),
        };
        db.skins.push(skin.clone());
        self.save_db(&db).await?;
        Ok(skin)
    }

    /// Extract the official default skins from an installed client jar. This
    /// avoids bundling Mojang assets with the launcher: the bytes come from the
    /// user's own game copy.
    async fn ensure_defaults(&self, game: &GamePaths) -> Result<(Option<String>, Option<String>)> {
        let db = self.load_db().await?;
        if db.builtin_version.is_some() && db.skins.iter().any(|s| s.source == SkinSource::Builtin)
        {
            return Ok((db.builtin_version, None));
        }

        let candidates = installed_jars(game).await;
        for (version_id, jar) in candidates {
            let extracted = match extract_default_skins(&jar) {
                Ok(v) => v,
                Err(e) => {
                    tracing::debug!(jar = %jar.display(), error = %e, "skin extraction failed");
                    continue;
                }
            };
            if extracted.is_empty() {
                continue;
            }
            let mut db = self.load_db().await?;
            util::ensure_dir(&self.dir()).await?;
            for (key, model, bytes) in extracted {
                let id = format!("builtin-{}-{}", model.api_name(), key);
                if db.skins.iter().any(|s| s.id == id) {
                    continue;
                }
                let processed = match process_png(&bytes) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::debug!(key, error = %e, "invalid builtin skin");
                        continue;
                    }
                };
                util::atomic_write(&self.original_path(&id), &processed.original).await?;
                util::atomic_write(&self.compat_path(&id), &processed.compat).await?;
                db.skins.push(Skin {
                    id,
                    name: display_name(&key),
                    model,
                    source: SkinSource::Builtin,
                    width: processed.width,
                    height: processed.height,
                    sha1: processed.sha1,
                    owner: None,
                    created_at: util::now_rfc3339(),
                });
            }
            db.builtin_version = Some(version_id.clone());
            self.save_db(&db).await?;
            return Ok((Some(version_id), None));
        }

        Ok((
            db.builtin_version,
            Some(
                "未在已安装版本中找到官方默认皮肤。安装任意 1.20.2+ 原版后会自动提取（来自你本机的游戏文件）。"
                    .into(),
            ),
        ))
    }
}

// ---------------------------------------------------------------------------
// PNG handling
// ---------------------------------------------------------------------------

fn process_png(bytes: &[u8]) -> Result<ProcessedSkin> {
    if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(AppError::invalid_input("不是有效的 PNG 文件"));
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| AppError::invalid_input(format!("PNG 解码失败：{e}")))?;
    // Validate dimensions before allocating pixel buffers so a crafted header
    // cannot trigger a large allocation.
    let (declared_w, declared_h) = {
        let info = reader.info();
        (info.width, info.height)
    };
    if !matches!((declared_w, declared_h), (64, 64) | (128, 128) | (64, 32)) {
        return Err(AppError::invalid_input(format!(
            "皮肤尺寸必须为 64×64、128×128 或旧版 64×32（当前 {declared_w}×{declared_h}）"
        )));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| AppError::invalid_input("PNG 尺寸过大"))?;
    let mut buf = vec![0u8; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| AppError::invalid_input(format!("PNG 解码失败：{e}")))?;
    let (width, height) = (info.width, info.height);
    if !matches!((width, height), (64, 64) | (128, 128) | (64, 32)) {
        return Err(AppError::invalid_input(format!(
            "皮肤尺寸必须为 64×64、128×128 或旧版 64×32（当前 {width}×{height}）"
        )));
    }
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => {
            return Err(AppError::invalid_input("不支持索引色 PNG"));
        }
    };
    if rgba.len() != (width * height * 4) as usize {
        return Err(AppError::invalid_input("PNG 像素数据不完整"));
    }
    let compat_rgba = match (width, height) {
        (64, 64) => rgba.clone(),
        (64, 32) => convert_legacy_64x32(&rgba),
        _ => downsample_2x(&rgba, width, height),
    };
    let sha1 = util::sha1_bytes(&compat_rgba);
    let compat = encode_png(&compat_rgba, COMPAT_SIZE, COMPAT_SIZE)?;
    Ok(ProcessedSkin {
        width,
        height,
        sha1,
        original: bytes.to_vec(),
        compat,
    })
}

/// Convert the pre-1.8 64x32 layout to 64x64 by mirroring the right limbs into
/// the left limb slots. This mirrors the reference algorithm used by
/// skinview-utils: each 4px face is copied with a horizontal flip.
fn convert_legacy_64x32(src: &[u8]) -> Vec<u8> {
    let mut dst = vec![0u8; 64 * 64 * 4];
    dst[..64 * 32 * 4].copy_from_slice(&src[..64 * 32 * 4]);
    // The head overlay does not exist in legacy skins; leave it transparent.
    for y in 0..16usize {
        for x in 32..64usize {
            let i = (y * 64 + x) * 4;
            dst[i..i + 4].fill(0);
        }
    }
    let faces: [(usize, usize, usize, usize, usize, usize); 12] = [
        (4, 16, 4, 4, 20, 48),   // right leg top -> left leg top
        (8, 16, 4, 4, 24, 48),   // right leg bottom -> left leg bottom
        (0, 20, 4, 12, 24, 52),  // right leg outer -> left leg inner
        (4, 20, 4, 12, 20, 52),  // right leg front -> left leg front
        (8, 20, 4, 12, 16, 52),  // right leg inner -> left leg outer
        (12, 20, 4, 12, 28, 52), // right leg back -> left leg back
        (44, 16, 4, 4, 36, 48),  // right arm top -> left arm top
        (48, 16, 4, 4, 40, 48),  // right arm bottom -> left arm bottom
        (40, 20, 4, 12, 40, 52), // right arm outer -> left arm inner
        (44, 20, 4, 12, 36, 52), // right arm front -> left arm front
        (48, 20, 4, 12, 32, 52), // right arm inner -> left arm outer
        (52, 20, 4, 12, 44, 52), // right arm back -> left arm back
    ];
    for (sx, sy, w, h, dx, dy) in faces {
        for y in 0..h {
            for x in 0..w {
                let s = ((sy + y) * 64 + sx + x) * 4;
                let d = ((dy + y) * 64 + dx + (w - 1 - x)) * 4;
                dst[d..d + 4].copy_from_slice(&src[s..s + 4]);
            }
        }
    }
    dst
}

fn downsample_2x(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (nw, nh) = (width / 2, height / 2);
    let mut out = Vec::with_capacity((nw * nh * 4) as usize);
    for y in 0..nh {
        for x in 0..nw {
            let src = (((y * 2) * width + x * 2) * 4) as usize;
            out.extend_from_slice(&rgba[src..src + 4]);
        }
    }
    out
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| AppError::internal(format!("无法生成 PNG：{e}")))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| AppError::internal(format!("无法生成 PNG：{e}")))?;
    }
    Ok(out)
}

fn clean_name(raw: &str) -> String {
    let stem = Path::new(raw)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let trimmed: String = stem.trim().chars().take(40).collect();
    if trimmed.is_empty() {
        "本地皮肤".into()
    } else {
        trimmed
    }
}

fn display_name(key: &str) -> String {
    let mut chars = key.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => key.to_string(),
    }
}

fn normalize_texture_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("http://textures.minecraft.net/") {
        return format!("https://textures.minecraft.net/{rest}");
    }
    url.to_string()
}

// ---------------------------------------------------------------------------
// Default skin extraction
// ---------------------------------------------------------------------------

async fn installed_jars(game: &GamePaths) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(std::time::SystemTime, String, PathBuf)> = Vec::new();
    let Ok(mut rd) = tokio::fs::read_dir(game.versions_dir()).await else {
        return Vec::new();
    };
    while let Ok(Some(entry)) = rd.next_entry().await {
        let Ok(ft) = entry.file_type().await else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let jar = entry.path().join(format!("{id}.jar"));
        if tokio::fs::try_exists(&jar).await.unwrap_or(false) {
            let modified = std::fs::metadata(&jar)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            out.push((modified, id, jar));
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.0));
    out.into_iter().map(|(_, id, jar)| (id, jar)).collect()
}

/// Returns `(key, model, bytes)` for the default player skins contained in a
/// client jar. Supports the modern `player/wide|slim/<name>.png` layout and the
/// legacy `entity/steve.png` / `entity/alex.png` layout.
fn extract_default_skins(jar: &Path) -> Result<Vec<(String, SkinModel, Vec<u8>)>> {
    let file = std::fs::File::open(jar)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::internal(format!("无法读取客户端 JAR：{e}")))?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut entry = match zip.by_index(i) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let name = entry.name()?.to_string();
        let Some(rest) = name.strip_prefix("assets/minecraft/textures/entity/player/") else {
            continue;
        };
        let (model, rest) = if let Some(r) = rest.strip_prefix("wide/") {
            (SkinModel::Classic, r)
        } else if let Some(r) = rest.strip_prefix("slim/") {
            (SkinModel::Slim, r)
        } else {
            continue;
        };
        let Some(key) = rest.strip_suffix(".png") else {
            continue;
        };
        if key.contains('/') {
            continue;
        }
        let mut bytes = Vec::new();
        if entry.read_to_end(&mut bytes).is_err() {
            continue;
        }
        out.push((key.to_ascii_lowercase(), model, bytes));
    }
    if out.is_empty() {
        // Legacy layout (1.7 - 1.12): the vanilla default textures.
        for (file_name, model, key) in [
            (
                "assets/minecraft/textures/entity/steve.png",
                SkinModel::Classic,
                "steve",
            ),
            (
                "assets/minecraft/textures/entity/alex.png",
                SkinModel::Slim,
                "alex",
            ),
        ] {
            if let Ok(mut entry) = zip.by_name(file_name) {
                let mut bytes = Vec::new();
                if entry.read_to_end(&mut bytes).is_ok() {
                    out.push((key.to_string(), model, bytes));
                }
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Applying skins
// ---------------------------------------------------------------------------

/// Write or remove the client-side resource pack used by offline profiles.
/// Returns whether a skin pack is active afterwards.
pub async fn apply_local_skin(
    game_dir: &Path,
    skin_png: Option<&[u8]>,
    pack_format: u32,
) -> Result<bool> {
    let resourcepacks = game_dir.join("resourcepacks");
    let pack_path = resourcepacks.join(LOCAL_PACK_NAME);
    match skin_png {
        Some(bytes) => {
            util::ensure_dir(&resourcepacks).await?;
            let zip_bytes = build_skin_pack(bytes, pack_format)?;
            util::atomic_write(&pack_path, &zip_bytes).await?;
            set_pack_enabled(game_dir, true).await?;
            Ok(true)
        }
        None => {
            let had_pack = tokio::fs::try_exists(&pack_path).await.unwrap_or(false);
            if had_pack {
                let _ = tokio::fs::remove_file(&pack_path).await;
            }
            if had_pack || options_mentions_pack(game_dir).await {
                set_pack_enabled(game_dir, false).await?;
            }
            Ok(false)
        }
    }
}

fn build_skin_pack(skin_png: &[u8], format: u32) -> Result<Vec<u8>> {
    // 26.x rejects the pack when `supported_formats` is present at all (it is
    // deprecated from format 65 onward), while older clients need it. Keep the
    // two schemas strictly separate.
    let pack = if format >= 65 {
        serde_json::json!({
            "pack_format": format,
            "min_format": format,
            "max_format": format,
            "description": "SRL 本地皮肤（仅本机可见）"
        })
    } else {
        serde_json::json!({
            "pack_format": format,
            "supported_formats": { "min_inclusive": 3, "max_inclusive": 99 },
            "description": "SRL 本地皮肤（仅本机可见）"
        })
    };
    let meta = serde_json::json!({ "pack": pack });
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("pack.mcmeta", options)
        .map_err(|e| AppError::internal(format!("无法生成皮肤资源包：{e}")))?;
    zip.write_all(serde_json::to_string(&meta)?.as_bytes())?;

    // Modern layout: override every default character so the client picks our
    // texture regardless of the UUID-based default.
    for folder in ["wide", "slim"] {
        for name in [
            "steve", "alex", "ari", "efe", "kai", "makena", "noor", "sunny", "zuri",
        ] {
            let path = format!("assets/minecraft/textures/entity/player/{folder}/{name}.png");
            zip.start_file(path.as_str(), options)
                .map_err(|e| AppError::internal(format!("无法生成皮肤资源包：{e}")))?;
            zip.write_all(skin_png)?;
        }
    }
    // Legacy layout (pre-1.19.4).
    for name in ["steve", "alex"] {
        let path = format!("assets/minecraft/textures/entity/{name}.png");
        zip.start_file(path.as_str(), options)
            .map_err(|e| AppError::internal(format!("无法生成皮肤资源包：{e}")))?;
        zip.write_all(skin_png)?;
    }
    let cursor = zip
        .finish()
        .map_err(|e| AppError::internal(format!("无法生成皮肤资源包：{e}")))?;
    Ok(cursor.into_inner())
}

async fn options_mentions_pack(game_dir: &Path) -> bool {
    match tokio::fs::read_to_string(game_dir.join("options.txt")).await {
        Ok(content) => content
            .lines()
            .any(|line| line.starts_with("resourcePacks:") && line.contains(LOCAL_PACK_NAME)),
        Err(_) => false,
    }
}

async fn set_pack_enabled(game_dir: &Path, enabled: bool) -> Result<()> {
    let path = game_dir.join("options.txt");
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let key = "resourcePacks:";
    let idx = lines.iter().position(|l| l.starts_with(key));
    let mut packs: Vec<String> = match idx {
        Some(i) => {
            let raw = lines[i][key.len()..].trim();
            if raw.is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(raw).map_err(|_| {
                    AppError::internal(
                        "options.txt 中的 resourcePacks 不是合法 JSON，已保持原样（未改写）",
                    )
                })?
            }
        }
        None => Vec::new(),
    };
    let has = packs.iter().any(|p| p == LOCAL_PACK_ENTRY);
    if enabled && !has {
        packs.push(LOCAL_PACK_ENTRY.to_string());
    } else if !enabled && has {
        packs.retain(|p| p != LOCAL_PACK_ENTRY);
    } else {
        return Ok(());
    }
    let line = format!("{key}{}", serde_json::to_string(&packs)?);
    match idx {
        Some(i) => lines[i] = line,
        None => lines.push(line),
    }
    let mut text = lines.join("\n");
    text.push('\n');
    util::atomic_write(&path, text.as_bytes()).await?;
    Ok(())
}

/// Read the resource pack format from the client JAR (`version.json`). Modern
/// snapshots (26.x) expose `resource_major`; older releases expose `resource`.
pub fn detect_pack_format(jar: &Path) -> Option<u32> {
    let file = std::fs::File::open(jar).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut entry = zip.by_name("version.json").ok()?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut entry, &mut text).ok()?;
    let raw: serde_json::Value = serde_json::from_str(&text).ok()?;
    let pack = raw.get("pack_version")?;
    pack.get("resource_major")
        .or_else(|| pack.get("resource"))
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
}

/// Resource pack formats change between Minecraft versions. A pack whose
/// `pack_format` is newer than the client is treated as incompatible and will
/// not be enabled automatically, so map the most common releases explicitly.
pub fn pack_format_for(version_id: &str) -> u32 {
    let core = version_id
        .split('-')
        .next()
        .unwrap_or(version_id)
        .trim_start_matches('v');
    let mut it = core.split('.');
    let major: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let minor: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch: u32 = it
        .next()
        .and_then(|s| s.split('.').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    // 26.x packs are resolved from the client JAR; this is only a fallback.
    if major != 1 {
        return 97;
    }
    match (minor, patch) {
        (0..=12, _) => 3,
        (13..=14, _) => 4,
        (15, _) | (16, 0 | 1) => 5,
        (16, _) => 6,
        (17, _) => 7,
        (18, _) => 8,
        (19, 0..=2) => 9,
        (19, 3) => 12,
        (19, 4) => 13,
        (20, 0 | 1) => 15,
        (20, 2) => 18,
        (20, 3 | 4) => 22,
        (20, 5 | 6) => 32,
        (21, 0 | 1) => 34,
        (21, 2 | 3) => 42,
        (21, 4) => 46,
        (21, 5) => 55,
        (21, 6) => 63,
        (21, 7 | 8) => 64,
        _ => 15,
    }
}

// ---------------------------------------------------------------------------
// Microsoft upload
// ---------------------------------------------------------------------------

/// Upload a skin to the official Minecraft profile service. The caller must
/// supply a valid Minecraft access token.
pub async fn upload_microsoft(
    http: &reqwest::Client,
    access_token: &str,
    skin_png: &[u8],
    model: SkinModel,
) -> Result<()> {
    upload_microsoft_to(
        http,
        "https://api.minecraftservices.com/minecraft/profile/skins",
        access_token,
        skin_png,
        model,
    )
    .await
}

async fn upload_microsoft_to(
    http: &reqwest::Client,
    url: &str,
    access_token: &str,
    skin_png: &[u8],
    model: SkinModel,
) -> Result<()> {
    if skin_png.is_empty() {
        return Err(AppError::invalid_input("皮肤文件为空"));
    }
    let boundary = format!("----SRLSkin{}", util::short_id());
    let body = multipart_body(&boundary, model, skin_png);
    let resp = http
        .post(url)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {access_token}"),
        )
        .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
        .header(
            reqwest::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .send()
        .await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(AppError::new(
            ErrorCode::AuthenticationRequired,
            "Minecraft 授权已过期，请重新登录后再试",
        ));
    }
    let detail = resp.text().await.unwrap_or_default();
    if status == reqwest::StatusCode::BAD_REQUEST {
        return Err(
            AppError::invalid_input("Mojang 拒绝了该皮肤（尺寸或内容不符合要求）")
                .with_detail(detail.chars().take(300).collect::<String>()),
        );
    }
    Err(AppError::new(
        ErrorCode::Http,
        format!("皮肤上传返回 HTTP {}", status.as_u16()),
    )
    .with_detail(detail.chars().take(300).collect::<String>()))
}

fn multipart_body(boundary: &str, model: SkinModel, skin_png: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"variant\"\r\n\r\n");
    body.extend_from_slice(model.api_name().as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"skin.png\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: image/png\r\n\r\n");
    body.extend_from_slice(skin_png);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_png(width: u32, height: u32, alpha: u8) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, 90, alpha]);
            }
        }
        encode_png(&rgba, width, height).unwrap()
    }

    #[test]
    fn accepts_64_and_128_and_rejects_other_sizes() {
        let p = process_png(&test_png(64, 64, 255)).unwrap();
        assert_eq!((p.width, p.height), (64, 64));
        let base = p.sha1.clone();

        let mut hd_rgba = Vec::new();
        for y in 0..128u32 {
            for x in 0..128u32 {
                hd_rgba.extend_from_slice(&[((x / 2) % 256) as u8, ((y / 2) % 256) as u8, 90, 255]);
            }
        }
        let hd_png = encode_png(&hd_rgba, 128, 128).unwrap();
        let hd = process_png(&hd_png).unwrap();
        assert_eq!((hd.width, hd.height), (128, 128));
        // The HD fixture is a 2x nearest-neighbour copy: downscaling must
        // reproduce the same 64x64 pixels.
        assert_eq!(hd.sha1, base);

        let err = process_png(&test_png(32, 32, 255)).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert!(err.message.contains("64×64"));
        let err = process_png(b"not a png").unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn converts_legacy_64x32_limbs_and_clears_head_overlay() {
        let mut rgba = vec![0u8; 64 * 32 * 4];
        let mut put = |x: usize, y: usize, color: [u8; 4]| {
            let i = (y * 64 + x) * 4;
            rgba[i..i + 4].copy_from_slice(&color);
        };
        // Unused head-overlay area: must not survive as an opaque hat.
        for y in 0..16 {
            for x in 32..64 {
                put(x, y, [0, 0, 255, 255]);
            }
        }
        put(44, 20, [255, 0, 0, 255]); // right arm front
        put(4, 20, [0, 255, 0, 255]); // right leg front

        let png = encode_png(&rgba, 64, 32).unwrap();
        let processed = process_png(&png).unwrap();
        assert_eq!((processed.width, processed.height), (64, 32));

        let mut decoder = png::Decoder::new(Cursor::new(&processed.compat));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buf).unwrap();

        let alpha = |x: usize, y: usize| buf[(y * 64 + x) * 4 + 3];
        let pixel = |x: usize, y: usize| {
            let i = (y * 64 + x) * 4;
            [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
        };
        assert_eq!(alpha(40, 4), 0, "head overlay must be transparent");
        assert_eq!(pixel(39, 52), [255, 0, 0, 255], "left arm front mirrored");
        assert_eq!(pixel(23, 52), [0, 255, 0, 255], "left leg front mirrored");
    }

    #[test]
    fn skin_pack_writes_absolute_format_fields() {
        let bytes = build_skin_pack(&test_png(64, 64, 255), 97).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut meta = String::new();
        zip.by_name("pack.mcmeta")
            .unwrap()
            .read_to_string(&mut meta)
            .unwrap();
        assert!(meta.contains("\"min_format\":97"));
        assert!(meta.contains("\"max_format\":97"));
        assert!(meta.contains("\"pack_format\":97"));
        assert!(!meta.contains("supported_formats"));
    }

    #[test]
    fn detects_pack_format_from_client_jar() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("client.jar");
        let file = std::fs::File::create(&jar).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("version.json", options).unwrap();
        zip.write_all(br#"{"pack_version":{"resource":34,"data":48}}"#)
            .unwrap();
        zip.finish().unwrap();
        assert_eq!(detect_pack_format(&jar), Some(34));
    }

    #[test]
    fn pack_formats_are_version_aware() {
        assert_eq!(pack_format_for("1.12.2"), 3);
        assert_eq!(pack_format_for("1.20.1"), 15);
        assert_eq!(pack_format_for("1.21.1"), 34);
        assert_eq!(pack_format_for("1.21.1-forge-52.1.20"), 34);
        assert_eq!(pack_format_for("1.20.1-Fabric-0.15"), 15);
    }

    #[tokio::test]
    async fn import_roundtrip_and_duplicate_detection() {
        let dir = tempfile::tempdir().unwrap();
        let lib = SkinLibrary::new(AppPaths::new(dir.path()), reqwest::Client::new());
        let bytes = test_png(64, 64, 200);
        let a = lib
            .import_bytes("my skin.png", &bytes, SkinModel::Slim)
            .await
            .unwrap();
        assert_eq!(a.source, SkinSource::Local);
        assert_eq!(a.model, SkinModel::Slim);
        assert_eq!(a.name, "my skin");
        let b = lib
            .import_bytes("copy.png", &bytes, SkinModel::Classic)
            .await
            .unwrap();
        assert_eq!(a.id, b.id, "identical pixels must de-duplicate");

        let view = lib.view(&a).await.unwrap();
        assert!(view.preview.starts_with("data:image/png;base64,"));
        assert!(lib.delete(&a.id).await.is_ok());
        assert!(lib.get(&a.id).await.is_err());
    }

    #[tokio::test]
    async fn extracts_default_skins_from_a_client_jar() {
        let dir = tempfile::tempdir().unwrap();
        let game = GamePaths::new(dir.path().join("minecraft"));
        game.ensure_layout().await.unwrap();
        let version_dir = game.versions_dir().join("1.20.4");
        tokio::fs::create_dir_all(&version_dir).await.unwrap();
        let jar_path = version_dir.join("1.20.4.jar");
        {
            let file = std::fs::File::create(&jar_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            for (path, model) in [
                ("assets/minecraft/textures/entity/player/wide/steve.png", 64),
                ("assets/minecraft/textures/entity/player/slim/alex.png", 64),
            ] {
                zip.start_file(path, options).unwrap();
                zip.write_all(&test_png(model, model, 255)).unwrap();
            }
            zip.finish().unwrap();
        }

        let lib = SkinLibrary::new(
            AppPaths::new(dir.path().join("app")),
            reqwest::Client::new(),
        );
        let list = lib.list(&game).await.unwrap();
        assert_eq!(list.builtin_version.as_deref(), Some("1.20.4"));
        let names: Vec<_> = list
            .skins
            .iter()
            .map(|s| (s.skin.name.as_str(), s.skin.model))
            .collect();
        assert!(names.contains(&("Steve", SkinModel::Classic)));
        assert!(names.contains(&("Alex", SkinModel::Slim)));
        assert!(list
            .skins
            .iter()
            .all(|s| s.preview.starts_with("data:image/png")));

        let steve = list.skins.iter().find(|s| s.skin.name == "Steve").unwrap();
        assert_eq!(
            lib.delete(&steve.skin.id).await.unwrap_err().code,
            ErrorCode::Unsupported
        );
    }

    #[tokio::test]
    async fn local_resourcepack_is_written_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path();
        let png = test_png(64, 64, 255);
        assert!(apply_local_skin(game_dir, Some(&png), 34).await.unwrap());
        let pack = game_dir.join("resourcepacks").join(LOCAL_PACK_NAME);
        assert!(pack.exists());
        let options = tokio::fs::read_to_string(game_dir.join("options.txt"))
            .await
            .unwrap();
        assert!(options.contains("file/SRLocalSkin.zip"));

        // The pack must contain both modern and legacy default-skin paths.
        let file = std::fs::File::open(&pack).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        assert!(zip
            .by_name("assets/minecraft/textures/entity/player/wide/steve.png")
            .is_ok());
        assert!(zip
            .by_name("assets/minecraft/textures/entity/player/slim/alex.png")
            .is_ok());
        assert!(zip
            .by_name("assets/minecraft/textures/entity/steve.png")
            .is_ok());

        assert!(!apply_local_skin(game_dir, None, 34).await.unwrap());
        assert!(!pack.exists());
        let options = tokio::fs::read_to_string(game_dir.join("options.txt"))
            .await
            .unwrap();
        assert!(!options.contains("file/SRLocalSkin.zip"));
    }

    #[tokio::test]
    async fn malformed_options_are_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path();
        tokio::fs::write(game_dir.join("options.txt"), "resourcePacks:not-json\n")
            .await
            .unwrap();
        let err = apply_local_skin(game_dir, Some(&test_png(64, 64, 255)), 34)
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Internal);
        let raw = tokio::fs::read_to_string(game_dir.join("options.txt"))
            .await
            .unwrap();
        assert_eq!(raw, "resourcePacks:not-json\n");
    }

    #[test]
    fn multipart_contains_variant_and_file() {
        let body = multipart_body("BOUND", SkinModel::Slim, b"PNGDATA");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("name=\"variant\"\r\n\r\nslim"));
        assert!(text.contains("name=\"file\"; filename=\"skin.png\""));
        assert!(text.contains("Content-Type: image/png"));
        assert!(text.ends_with("--BOUND--\r\n"));
    }

    async fn mock_post_server(
        status: u16,
        reply: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 16384];
            let n = socket.read(&mut buf).await.unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let header = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.len(),
                reply
            );
            let _ = socket.write_all(header.as_bytes()).await;
            request
        });
        (format!("http://127.0.0.1:{}/skin", addr.port()), handle)
    }

    #[tokio::test]
    async fn upload_microsoft_sends_authorization_and_multipart() {
        let (url, server) = mock_post_server(200, "{}").await;
        upload_microsoft_to(
            &reqwest::Client::new(),
            &url,
            "secret-token",
            b"PNGDATA",
            SkinModel::Slim,
        )
        .await
        .unwrap();
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /skin HTTP/1.1"));
        assert!(
            request.contains("authorization: Bearer secret-token")
                || request.contains("Authorization: Bearer secret-token")
        );
        assert!(request.contains("multipart/form-data; boundary=----SRLSkin"));
        assert!(request.contains("name=\"variant\""));
        assert!(request.contains("slim"));
        assert!(request.contains("PNGDATA"));
    }

    #[tokio::test]
    async fn upload_microsoft_reports_expired_authorization() {
        let (url, server) = mock_post_server(401, "{\"error\":\"unauthorized\"}").await;
        let err = upload_microsoft_to(
            &reqwest::Client::new(),
            &url,
            "expired",
            b"PNGDATA",
            SkinModel::Classic,
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::AuthenticationRequired);
        server.await.unwrap();
    }

    #[tokio::test]
    #[ignore = "hits the real Mojang API"]
    async fn fetch_real_player_skin() {
        let dir = tempfile::tempdir().unwrap();
        let lib = SkinLibrary::new(AppPaths::new(dir.path()), reqwest::Client::new());
        let skin = lib.fetch_mojang("Notch").await.expect("real lookup failed");
        assert_eq!(skin.source, SkinSource::Mojang);
        assert_eq!(skin.owner.as_deref(), Some("Notch"));
        assert!(matches!((skin.width, skin.height), (64, 64) | (64, 32)));
        let view = lib.view(&skin).await.unwrap();
        assert!(view.preview.starts_with("data:image/png"));
    }

    #[tokio::test]
    async fn fetches_a_skin_from_mocked_mojang_endpoints() {
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let texture = test_png(64, 64, 255);
        let texture_for_server = texture.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let rec = requests.clone();
        let server = tokio::spawn(async move {
            for _ in 0..3 {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let rec = rec.clone();
                let texture = texture_for_server.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).to_string();
                    rec.lock().unwrap().push(head.clone());
                    let (content_type, body): (&str, Vec<u8>) = if head.contains("/texture") {
                        ("image/png", texture)
                    } else if head.contains("/session/") {
                        let textures = serde_json::json!({
                            "textures": { "SKIN": {
                                "url": format!("http://127.0.0.1:{}/texture", addr.port()),
                                "metadata": { "model": "slim" }
                            }}
                        });
                        let value = base64::engine::general_purpose::STANDARD
                            .encode(serde_json::to_vec(&textures).unwrap());
                        let body = serde_json::json!({
                            "id": "069a79f444e94726a5befca90e38aaf5",
                            "name": "Notch",
                            "properties": [{ "name": "textures", "value": value }]
                        });
                        ("application/json", serde_json::to_vec(&body).unwrap())
                    } else {
                        let body = serde_json::json!({
                            "id": "069a79f444e94726a5befca90e38aaf5",
                            "name": "Notch"
                        });
                        ("application/json", serde_json::to_vec(&body).unwrap())
                    };
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                });
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let base = format!("http://127.0.0.1:{}", addr.port());
        let lib = SkinLibrary::new(AppPaths::new(dir.path()), reqwest::Client::new())
            .with_endpoints(
                MojangEndpoints {
                    profile: format!("{base}/profiles"),
                    session: format!("{base}/session/minecraft/profile"),
                },
                vec!["127.0.0.1".into()],
            );
        let skin = lib.fetch_mojang("Notch").await.unwrap();
        assert_eq!(skin.source, SkinSource::Mojang);
        assert_eq!(skin.model, SkinModel::Slim);
        assert_eq!(skin.owner.as_deref(), Some("Notch"));
        assert_eq!((skin.width, skin.height), (64, 64));
        server.await.unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests[0].contains("/profiles/Notch"));
        assert!(requests[1].contains("/session/minecraft/profile/069a"));
    }
}
