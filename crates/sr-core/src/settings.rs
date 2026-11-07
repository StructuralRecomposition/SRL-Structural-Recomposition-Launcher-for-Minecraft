use crate::error::{AppError, Result};
use crate::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_CONCURRENCY: u32 = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Minecraft directory (versions/libraries/assets/instances).
    pub game_dir: PathBuf,
    /// Optional explicit Java executable. Empty means auto-detect.
    #[serde(default)]
    pub java_path: Option<PathBuf>,
    #[serde(default = "default_concurrency")]
    pub max_concurrent_downloads: u32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_true")]
    pub download_assets_on_install: bool,
    #[serde(default)]
    pub language: String,
    /// Explicit opt-in for user-configured mirrors. Not implemented in phase 1.
    #[serde(default)]
    pub allow_custom_hosts: bool,
    /// Prefer the public BMCLAPI mirror for Fabric metadata/artifacts, with
    /// automatic fallback to the official Fabric hosts.
    #[serde(default = "default_true")]
    pub use_mirror: bool,
    /// Public Azure application (client) id for Microsoft device-code login.
    /// This is not a secret; empty means Microsoft login stays disabled.
    #[serde(default)]
    pub microsoft_client_id: Option<String>,
    /// Apply the selected skin to offline profiles as a client-side resource
    /// pack (visible on this machine only).
    #[serde(default = "default_true")]
    pub apply_offline_skin: bool,
    /// DeepSeek-compatible model used by the 鸦鸭 assistant.
    #[serde(default = "default_assistant_model")]
    pub assistant_model: String,
    /// DeepSeek-compatible API base URL (no key; the key lives in keyring).
    #[serde(default = "default_assistant_base_url")]
    pub assistant_base_url: String,
}

fn default_concurrency() -> u32 {
    6
}
fn default_theme() -> String {
    "light".into()
}
fn default_true() -> bool {
    true
}
fn default_assistant_model() -> String {
    "deepseek-chat".into()
}
fn default_assistant_base_url() -> String {
    "https://api.deepseek.com".into()
}

impl Settings {
    pub fn defaults(base: &AppPaths) -> Self {
        Self {
            game_dir: base.base().join("minecraft"),
            java_path: None,
            max_concurrent_downloads: default_concurrency(),
            theme: default_theme(),
            download_assets_on_install: true,
            language: "zh-CN".into(),
            allow_custom_hosts: false,
            use_mirror: true,
            microsoft_client_id: None,
            apply_offline_skin: true,
            assistant_model: default_assistant_model(),
            assistant_base_url: default_assistant_base_url(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.game_dir.as_os_str().is_empty() {
            return Err(AppError::invalid_input("游戏目录不能为空"));
        }
        if !(1..=MAX_CONCURRENCY).contains(&self.max_concurrent_downloads) {
            return Err(AppError::invalid_input(format!(
                "并发下载数必须在 1 到 {MAX_CONCURRENCY} 之间"
            )));
        }
        if !matches!(self.theme.as_str(), "dark" | "light" | "system") {
            return Err(AppError::invalid_input("未知的主题选项"));
        }
        if let Some(java) = &self.java_path {
            if java.as_os_str().is_empty() {
                return Err(AppError::invalid_input("Java 路径不能为空"));
            }
        }
        if self.assistant_model.trim().is_empty() || self.assistant_model.len() > 128 {
            return Err(AppError::invalid_input("AI 模型名称不能为空或过长"));
        }
        let base = self.assistant_base_url.trim();
        if base.is_empty() || base.len() > 512 {
            return Err(AppError::invalid_input("AI API 地址不能为空或过长"));
        }
        let parsed = reqwest::Url::parse(base)
            .map_err(|_| AppError::invalid_input("AI API 地址格式无效"))?;
        if parsed.scheme() != "https"
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(AppError::invalid_input(
                "AI API 地址必须是 https 且不包含凭据",
            ));
        }
        if let Some(id) = &self.microsoft_client_id {
            let id = id.trim();
            if id.is_empty() {
                return Err(AppError::invalid_input("Microsoft client id 不能为空"));
            }
            if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || id.len() > 64 {
                return Err(AppError::invalid_input(
                    "Microsoft client id 格式非法（应为 GUID）",
                ));
            }
        }
        Ok(())
    }
}

pub async fn load_or_default(paths: &AppPaths) -> Result<Settings> {
    let settings = match crate::util::read_json::<Settings>(&paths.settings_file()).await? {
        Some(s) => {
            s.validate()?;
            s
        }
        None => Settings::defaults(paths),
    };
    Ok(settings)
}

pub async fn save(paths: &AppPaths, settings: &Settings) -> Result<()> {
    settings.validate()?;
    crate::util::write_json(&paths.settings_file(), settings).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_concurrency() {
        let paths = AppPaths::new("/tmp/sr");
        let mut s = Settings::defaults(&paths);
        s.max_concurrent_downloads = 0;
        assert!(s.validate().is_err());
        s.max_concurrent_downloads = 99;
        assert!(s.validate().is_err());
        s.max_concurrent_downloads = 8;
        assert!(s.validate().is_ok());
    }

    #[test]
    fn validates_assistant_endpoint() {
        let paths = AppPaths::new("/tmp/sr");
        let mut s = Settings::defaults(&paths);
        assert!(s.validate().is_ok());
        s.assistant_base_url = "http://api.deepseek.com".into();
        assert!(s.validate().is_err());
        s.assistant_base_url = "https://api.deepseek.com".into();
        s.assistant_model = "  ".into();
        assert!(s.validate().is_err());
        s.assistant_model = "deepseek-chat".into();
        assert!(s.validate().is_ok());
    }

    #[test]
    fn defaults_roundtrip() {
        let paths = AppPaths::new("/tmp/sr");
        let s = Settings::defaults(&paths);
        assert_eq!(s.theme, "light");
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[tokio::test]
    async fn persists_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        let mut s = Settings::defaults(&paths);
        s.max_concurrent_downloads = 3;
        s.theme = "light".into();
        save(&paths, &s).await.unwrap();
        let loaded = load_or_default(&paths).await.unwrap();
        assert_eq!(loaded.max_concurrent_downloads, 3);
        assert_eq!(loaded.theme, "light");
    }
}
