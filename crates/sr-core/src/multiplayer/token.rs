use crate::account::{KeyringStore, SecretStore};
use crate::error::{AppError, ErrorCode, Result};
use std::sync::Arc;
use tokio::sync::Mutex;

const KEY_TOKEN: &str = "api_token";

pub struct Frp52TokenStore {
    secrets: Arc<dyn SecretStore>,
    cache: Mutex<Option<String>>,
}

impl Frp52TokenStore {
    pub fn keyring() -> Self {
        Self::new(Arc::new(KeyringStore::new("app.srlauncher.desktop.frp52")))
    }

    pub fn new(secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            secrets,
            cache: Mutex::new(None),
        }
    }

    pub fn has_token_cached(&self) -> bool {
        self.cache.try_lock().ok().is_some_and(|v| v.is_some())
    }

    pub async fn get(&self) -> Result<Option<String>> {
        if let Some(value) = self.cache.lock().await.clone() {
            return Ok(Some(value));
        }
        let value = self
            .secrets
            .get(KEY_TOKEN)?
            .filter(|v| !v.trim().is_empty());
        if let Some(value) = &value {
            *self.cache.lock().await = Some(value.clone());
        }
        Ok(value)
    }

    pub async fn set(&self, token: &str) -> Result<()> {
        let token = token.trim();
        if token.is_empty() {
            return Err(AppError::invalid_input("52FRP API 访问令牌不能为空"));
        }
        if !token.starts_with("hz_") {
            return Err(AppError::invalid_input("52FRP API 访问令牌应以 hz_ 开头"));
        }
        self.secrets.set(KEY_TOKEN, token)?;
        *self.cache.lock().await = Some(token.to_string());
        Ok(())
    }

    pub async fn clear(&self) -> Result<()> {
        *self.cache.lock().await = None;
        self.secrets.delete(KEY_TOKEN)?;
        Ok(())
    }

    pub async fn require(&self) -> Result<String> {
        self.get()
            .await?
            .ok_or_else(|| AppError::new(ErrorCode::AuthenticationRequired, "尚未连接 52FRP 账户"))
    }
}
