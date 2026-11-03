use crate::error::{AppError, ErrorCode, Result};
use crate::paths::AppPaths;
use crate::skin::SkinModel;
use crate::util;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountKind {
    /// Local-only profile. Never presented as a verified Minecraft account.
    Offline,
    Microsoft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountStatus {
    /// Offline profiles are usable locally but have no online identity.
    Offline,
    Authorized,
    Expired,
    /// Microsoft login requires an Azure client id that is not configured.
    NotConfigured,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub username: String,
    pub uuid: String,
    pub kind: AccountKind,
    pub status: AccountStatus,
    pub created_at: String,
    #[serde(default)]
    pub last_used_at: Option<String>,
    /// Selected skin in the launcher library. `None` keeps the account's
    /// current official skin (Microsoft) / default skin (offline).
    #[serde(default)]
    pub skin_id: Option<String>,
    /// Per-account model override. `None` uses the skin's authored model.
    #[serde(default)]
    pub skin_model: Option<SkinModel>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsFile {
    #[serde(default)]
    pub active_account_id: Option<String>,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    #[serde(flatten)]
    pub account: Account,
    pub active: bool,
    /// Human-readable explanation of what this profile can and cannot do.
    pub note: String,
}

#[derive(Clone)]
pub struct AccountStore {
    paths: AppPaths,
}

impl AccountStore {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub async fn load(&self) -> Result<AccountsFile> {
        Ok(util::read_json::<AccountsFile>(&self.paths.accounts_file())
            .await?
            .unwrap_or_default())
    }

    async fn save(&self, file: &AccountsFile) -> Result<()> {
        util::write_json(&self.paths.accounts_file(), file).await
    }

    pub async fn list(&self) -> Result<Vec<AccountView>> {
        let file = self.load().await?;
        Ok(file
            .accounts
            .into_iter()
            .map(|a| {
                let active = file.active_account_id.as_deref() == Some(a.id.as_str());
                let note = match (a.kind, a.status) {
                    (AccountKind::Offline, _) => {
                        "离线档案：仅用于本地游戏，不是已验证的正版账号。".to_string()
                    }
                    (AccountKind::Microsoft, AccountStatus::NotConfigured) => {
                        "Microsoft 登录尚未配置，无法使用。".to_string()
                    }
                    (AccountKind::Microsoft, AccountStatus::Expired) => {
                        "授权已过期，需要重新登录。".to_string()
                    }
                    _ => "已授权".to_string(),
                };
                AccountView {
                    account: a,
                    active,
                    note,
                }
            })
            .collect())
    }

    pub async fn add_offline(&self, username: &str) -> Result<Account> {
        let name = username.trim();
        validate_offline_username(name)?;
        let mut file = self.load().await?;
        if file
            .accounts
            .iter()
            .any(|a| a.kind == AccountKind::Offline && a.username.eq_ignore_ascii_case(name))
        {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                "同名离线档案已存在",
            ));
        }
        let account = Account {
            id: util::short_id(),
            username: name.to_string(),
            uuid: util::offline_uuid(name),
            kind: AccountKind::Offline,
            status: AccountStatus::Offline,
            created_at: util::now_rfc3339(),
            last_used_at: None,
            skin_id: None,
            skin_model: None,
        };
        file.accounts.push(account.clone());
        if file.active_account_id.is_none() {
            file.active_account_id = Some(account.id.clone());
        }
        self.save(&file).await?;
        Ok(account)
    }

    pub async fn remove(&self, id: &str) -> Result<()> {
        let mut file = self.load().await?;
        let before = file.accounts.len();
        file.accounts.retain(|a| a.id != id);
        if file.accounts.len() == before {
            return Err(AppError::not_found("账号不存在"));
        }
        if file.active_account_id.as_deref() == Some(id) {
            file.active_account_id = file.accounts.first().map(|a| a.id.clone());
        }
        self.save(&file).await
    }

    /// Create or update a Microsoft account after a successful login.
    pub async fn upsert_microsoft(&self, username: &str, uuid: &str) -> Result<Account> {
        if username.trim().is_empty() || uuid.trim().is_empty() {
            return Err(AppError::invalid_input("Microsoft 档案信息不完整"));
        }
        let mut file = self.load().await?;
        if let Some(existing) = file
            .accounts
            .iter_mut()
            .find(|a| a.kind == AccountKind::Microsoft && a.uuid == uuid)
        {
            existing.username = username.to_string();
            existing.status = AccountStatus::Authorized;
            existing.last_used_at = Some(util::now_rfc3339());
            let account = existing.clone();
            self.save(&file).await?;
            return Ok(account);
        }
        let account = Account {
            id: util::short_id(),
            username: username.to_string(),
            uuid: uuid.to_string(),
            kind: AccountKind::Microsoft,
            status: AccountStatus::Authorized,
            created_at: util::now_rfc3339(),
            last_used_at: Some(util::now_rfc3339()),
            skin_id: None,
            skin_model: None,
        };
        file.accounts.push(account.clone());
        if file.active_account_id.is_none() {
            file.active_account_id = Some(account.id.clone());
        }
        self.save(&file).await?;
        Ok(account)
    }

    /// Assign or clear the account skin. The caller validates that the skin
    /// exists before calling this.
    pub async fn set_skin(
        &self,
        id: &str,
        skin_id: Option<String>,
        model: Option<SkinModel>,
    ) -> Result<Account> {
        let mut file = self.load().await?;
        let account = file
            .accounts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| AppError::not_found("账号不存在"))?;
        account.skin_id = skin_id;
        account.skin_model = if account.skin_id.is_some() {
            model
        } else {
            None
        };
        let out = account.clone();
        self.save(&file).await?;
        Ok(out)
    }

    /// Remove references to a deleted skin from every account.
    pub async fn clear_skin_reference(&self, skin_id: &str) -> Result<()> {
        let mut file = self.load().await?;
        let mut changed = false;
        for account in &mut file.accounts {
            if account.skin_id.as_deref() == Some(skin_id) {
                account.skin_id = None;
                account.skin_model = None;
                changed = true;
            }
        }
        if changed {
            self.save(&file).await?;
        }
        Ok(())
    }

    pub async fn get(&self, id: &str) -> Result<Account> {
        let file = self.load().await?;
        file.accounts
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| AppError::not_found("账号不存在"))
    }

    pub async fn set_active(&self, id: &str) -> Result<()> {
        let mut file = self.load().await?;
        if !file.accounts.iter().any(|a| a.id == id) {
            return Err(AppError::not_found("账号不存在"));
        }
        file.active_account_id = Some(id.to_string());
        if let Some(a) = file.accounts.iter_mut().find(|a| a.id == id) {
            a.last_used_at = Some(util::now_rfc3339());
        }
        self.save(&file).await
    }

    pub async fn active(&self) -> Result<Option<Account>> {
        let file = self.load().await?;
        Ok(file
            .active_account_id
            .and_then(|id| file.accounts.into_iter().find(|a| a.id == id)))
    }

    /// Microsoft login is intentionally not faked. Without a configured Azure
    /// application the UI receives a precise, actionable error.
    pub async fn begin_microsoft_login(&self) -> Result<Account> {
        Err(AppError::new(
            ErrorCode::NotConfigured,
            "Microsoft 登录尚未配置",
        )
        .with_detail(
            "需要 Azure 应用注册的 client id 与 redirect URI，并在设置中启用。当前版本不会伪造登录结果。",
        ))
    }
}

pub fn validate_offline_username(name: &str) -> Result<()> {
    let len = name.chars().count();
    if !(3..=16).contains(&len) {
        return Err(AppError::invalid_input(
            "离线用户名长度必须为 3 到 16 个字符",
        ));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(AppError::invalid_input(
            "离线用户名只能包含字母、数字和下划线",
        ));
    }
    Ok(())
}

/// Secure credential abstraction. Implementations must never log values.
pub trait SecretStore: Send + Sync {
    fn set(&self, key: &str, value: &str) -> Result<()>;
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn delete(&self, key: &str) -> Result<()>;
}

pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new(service: &str) -> Self {
        Self {
            service: service.to_string(),
        }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, key).map_err(|e| {
            AppError::new(ErrorCode::NotConfigured, "系统凭据存储不可用").with_detail(e.to_string())
        })
    }
}

impl SecretStore for KeyringStore {
    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.entry(key)?.set_password(value).map_err(|e| {
            AppError::new(ErrorCode::NotConfigured, "无法写入系统凭据存储")
                .with_detail(e.to_string())
        })
    }
    fn get(&self, key: &str) -> Result<Option<String>> {
        match self.entry(key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(
                AppError::new(ErrorCode::NotConfigured, "无法读取系统凭据存储")
                    .with_detail(e.to_string()),
            ),
        }
    }
    fn delete(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::new(ErrorCode::NotConfigured, "无法删除系统凭据")
                .with_detail(e.to_string())),
        }
    }
}

/// In-memory store for tests only; never persisted.
#[derive(Default)]
pub struct MemorySecretStore {
    inner: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl SecretStore for MemorySecretStore {
    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.inner.lock().unwrap().get(key).cloned())
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.inner.lock().unwrap().remove(key);
        Ok(())
    }
}

pub fn offline_player(account: &Account) -> Result<crate::version::PlayerProfile> {
    if account.kind != AccountKind::Offline {
        return Err(AppError::new(
            ErrorCode::AuthenticationRequired,
            "该账号需要有效的在线授权",
        ));
    }
    Ok(crate::version::PlayerProfile {
        name: account.username.clone(),
        uuid: account.uuid.clone(),
        access_token: "0".into(),
        user_type: "legacy".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_name_rules() {
        assert!(validate_offline_username("Steve_1").is_ok());
        assert!(validate_offline_username("ab").is_err());
        assert!(validate_offline_username("this_name_is_way_too_long").is_err());
        assert!(validate_offline_username("bad name").is_err());
        assert!(validate_offline_username("中文名").is_err());
    }

    #[tokio::test]
    async fn offline_account_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::new(AppPaths::new(dir.path()));
        let a = store.add_offline("Steve").await.unwrap();
        assert_eq!(a.kind, AccountKind::Offline);
        assert_eq!(a.status, AccountStatus::Offline);
        assert_eq!(a.uuid, util::offline_uuid("Steve"));
        assert!(store.add_offline("steve").await.is_err());

        let b = store.add_offline("Alex").await.unwrap();
        assert_eq!(store.active().await.unwrap().unwrap().id, a.id);
        store.set_active(&b.id).await.unwrap();
        assert_eq!(store.active().await.unwrap().unwrap().id, b.id);

        let list = store.list().await.unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().find(|v| v.account.id == b.id).unwrap().active);
        assert!(list[0].note.contains("离线"));

        // Persisted file must never contain a token field.
        let raw = tokio::fs::read_to_string(dir.path().join("accounts.json"))
            .await
            .unwrap();
        assert!(!raw.to_lowercase().contains("token"));

        store.remove(&b.id).await.unwrap();
        assert_eq!(store.active().await.unwrap().unwrap().id, a.id);
    }

    #[tokio::test]
    async fn microsoft_login_is_explicitly_not_configured() {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::new(AppPaths::new(dir.path()));
        let err = store.begin_microsoft_login().await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotConfigured);
        assert!(err.message.contains("尚未配置"));
    }

    #[test]
    fn secret_store_roundtrip() {
        let s = MemorySecretStore::default();
        s.set("k", "v").unwrap();
        assert_eq!(s.get("k").unwrap().as_deref(), Some("v"));
        s.delete("k").unwrap();
        assert_eq!(s.get("k").unwrap(), None);
    }

    #[tokio::test]
    async fn account_skin_assignment_and_clear() {
        let dir = tempfile::tempdir().unwrap();
        let store = AccountStore::new(AppPaths::new(dir.path()));
        let a = store.add_offline("Alex").await.unwrap();
        let updated = store
            .set_skin(&a.id, Some("skin-abc".into()), Some(SkinModel::Slim))
            .await
            .unwrap();
        assert_eq!(updated.skin_id.as_deref(), Some("skin-abc"));
        assert_eq!(updated.skin_model, Some(SkinModel::Slim));

        let cleared = store
            .set_skin(&a.id, None, Some(SkinModel::Classic))
            .await
            .unwrap();
        assert!(cleared.skin_id.is_none());
        assert!(cleared.skin_model.is_none());

        store
            .set_skin(&a.id, Some("skin-xyz".into()), None)
            .await
            .unwrap();
        store.clear_skin_reference("skin-xyz").await.unwrap();
        assert!(store.get(&a.id).await.unwrap().skin_id.is_none());
    }

    #[test]
    fn offline_profile_uses_legacy_identity() {
        let account = Account {
            id: "x".into(),
            username: "Steve".into(),
            uuid: util::offline_uuid("Steve"),
            kind: AccountKind::Offline,
            status: AccountStatus::Offline,
            created_at: util::now_rfc3339(),
            last_used_at: None,
            skin_id: None,
            skin_model: None,
        };
        let p = offline_player(&account).unwrap();
        assert_eq!(p.user_type, "legacy");
        assert_eq!(p.access_token, "0");
    }
}
