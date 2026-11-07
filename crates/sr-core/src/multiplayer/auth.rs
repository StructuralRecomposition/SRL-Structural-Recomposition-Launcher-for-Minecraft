use crate::account::{KeyringStore, SecretStore};
use crate::error::{AppError, ErrorCode, Result};
use reqwest::Url;
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use super::types::{
    AccountView, MultiplayerConfig, API_BACKUP_BASE, API_BASE, OAUTH_AUTHORIZE,
    OAUTH_REDIRECT_LOCALHOST, PROVIDER_ID,
};

const KEY_REFRESH: &str = "locyan_refresh_token";
const KEY_USER_ID: &str = "locyan_user_id";
const OAUTH_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone)]
struct CachedAccess {
    token: String,
    user_id: u64,
    expires_at: Instant,
}

pub struct LoCyanAuth {
    http: reqwest::Client,
    config: MultiplayerConfig,
    secrets: Arc<dyn SecretStore>,
    access: Mutex<Option<CachedAccess>>,
}

impl LoCyanAuth {
    pub fn new(
        http: reqwest::Client,
        config: MultiplayerConfig,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            http,
            config,
            secrets,
            access: Mutex::new(None),
        }
    }

    pub fn keyring() -> Arc<dyn SecretStore> {
        Arc::new(KeyringStore::new("app.srlauncher.desktop.multiplayer"))
    }

    pub fn configured(&self) -> bool {
        self.config.configured()
    }

    pub fn has_session(&self) -> bool {
        if !self.config.configured() {
            return false;
        }
        self.secrets
            .get(KEY_REFRESH)
            .ok()
            .flatten()
            .is_some_and(|v| !v.trim().is_empty())
    }

    pub async fn cached_user_id(&self) -> Option<u64> {
        if let Some(cached) = self.access.lock().await.as_ref() {
            return Some(cached.user_id);
        }
        self.secrets
            .get(KEY_USER_ID)
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
    }

    /// Run the browser OAuth consent flow. The local loopback listener receives
    /// the refresh token from LoCyanFrp's redirect-localhost endpoint.
    pub async fn login(&self) -> Result<AccountView> {
        let client_id = self
            .config
            .client_id
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                AppError::not_configured(
                    "尚未配置 LoCyanFrp 应用 ID，请先在设置中填写 OAuth 应用 ID",
                )
            })?;

        let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| {
            AppError::network("无法启动本地 OAuth 回调监听").with_detail(e.to_string())
        })?;
        let port = listener
            .local_addr()
            .map_err(|e| AppError::internal(e.to_string()))?
            .port();

        let redirect = Url::parse_with_params(
            OAUTH_REDIRECT_LOCALHOST,
            &[
                ("port", port.to_string()),
                ("path", "/oauth/callback".to_string()),
                ("ssl", "false".to_string()),
            ],
        )
        .map_err(|e| AppError::internal(e.to_string()))?;

        let authorize = Url::parse_with_params(
            OAUTH_AUTHORIZE,
            &[
                ("client_id", client_id.to_string()),
                ("scopes", self.config.scopes.clone()),
                ("redirect_uri", redirect.to_string()),
                ("mode", "callback".to_string()),
            ],
        )
        .map_err(|e| AppError::internal(e.to_string()))?;

        open_browser(authorize.as_str()).await?;

        let refresh_token = wait_for_refresh_token(listener).await?;
        let access = self.exchange_refresh_token(&refresh_token).await?;

        self.secrets.set(KEY_REFRESH, &refresh_token)?;
        self.secrets.set(KEY_USER_ID, &access.user_id.to_string())?;
        *self.access.lock().await = Some(access.clone());

        self.account().await
    }

    pub async fn logout(&self) -> Result<()> {
        *self.access.lock().await = None;
        self.secrets.delete(KEY_REFRESH)?;
        self.secrets.delete(KEY_USER_ID)?;
        Ok(())
    }

    pub async fn access_token(&self) -> Result<String> {
        let mut guard = self.access.lock().await;
        if let Some(cached) = guard.as_ref() {
            if cached.expires_at > Instant::now() + Duration::from_secs(30) {
                return Ok(cached.token.clone());
            }
        }
        let refresh = self
            .secrets
            .get(KEY_REFRESH)?
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| {
                AppError::new(ErrorCode::AuthenticationRequired, "尚未连接 LoCyanFrp 账户")
            })?;
        let access = self.exchange_refresh_token(&refresh).await?;
        let token = access.token.clone();
        *guard = Some(access);
        Ok(token)
    }

    pub async fn force_refresh(&self) -> Result<String> {
        *self.access.lock().await = None;
        self.access_token().await
    }

    pub async fn account(&self) -> Result<AccountView> {
        let token = self.access_token().await?;
        let user_id = self.cached_user_id().await.ok_or_else(|| {
            AppError::new(ErrorCode::AuthenticationRequired, "缺少 LoCyanFrp 用户 ID")
        })?;
        let url = format!("{API_BASE}/user?user_id={user_id}");
        let response = self
            .http
            .get(url)
            .bearer_auth(&token)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| {
                AppError::network("无法读取 LoCyanFrp 账户信息").with_detail(e.to_string())
            })?;
        let text = response.text().await.unwrap_or_default();
        let value = decode_envelope(&text)?;
        Ok(parse_account(&value, user_id))
    }

    async fn exchange_refresh_token(&self, refresh_token: &str) -> Result<CachedAccess> {
        let client_id = self
            .config
            .client_id
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| AppError::not_configured("尚未配置 LoCyanFrp 应用 ID"))?;

        let mut last_error = None;
        for base in [API_BASE, API_BACKUP_BASE] {
            let response = self
                .http
                .post(format!("{base}/auth/oauth/access-token"))
                .form(&[("app_id", client_id), ("refresh_token", refresh_token)])
                .timeout(Duration::from_secs(30))
                .send()
                .await;
            match response {
                Ok(response) => {
                    let text = response.text().await.unwrap_or_default();
                    let value = decode_envelope(&text)?;
                    let data = value.get("data").ok_or_else(|| {
                        AppError::new(ErrorCode::Http, "LoCyanFrp 未返回访问令牌")
                    })?;
                    let token = data
                        .get("access_token")
                        .and_then(Value::as_str)
                        .filter(|v| !v.is_empty())
                        .ok_or_else(|| {
                            AppError::new(
                                ErrorCode::AuthenticationRequired,
                                "LoCyanFrp 拒绝授权，请重新连接账户",
                            )
                        })?;
                    let user_id = data
                        .get("user_id")
                        .and_then(value_u64)
                        .ok_or_else(|| AppError::internal("LoCyanFrp 未返回用户 ID"))?;
                    return Ok(CachedAccess {
                        token: token.to_string(),
                        user_id,
                        expires_at: Instant::now() + Duration::from_secs(3600),
                    });
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(AppError::network("无法连接 LoCyanFrp API")
            .with_detail(last_error.map(|e| e.to_string()).unwrap_or_default()))
    }
}

async fn wait_for_refresh_token(listener: TcpListener) -> Result<String> {
    let deadline = Instant::now() + OAUTH_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(AppError::timeout("等待 LoCyanFrp 授权超时，请重试"));
        }
        let (mut socket, _) = tokio::time::timeout(remaining, listener.accept())
            .await
            .map_err(|_| AppError::timeout("等待 LoCyanFrp 授权超时，请重试"))?
            .map_err(|e| AppError::network("OAuth 回调监听失败").with_detail(e.to_string()))?;

        let mut buffer = vec![0u8; 16 * 1024];
        let read = tokio::time::timeout(Duration::from_secs(10), socket.read(&mut buffer))
            .await
            .map_err(|_| AppError::timeout("OAuth 回调读取超时"))?
            .unwrap_or(0);
        let request = String::from_utf8_lossy(&buffer[..read]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("/");

        let parsed = Url::parse(&format!("http://127.0.0.1{target}")).ok();
        let Some(parsed) = parsed else {
            let _ = respond(&mut socket, 400, "授权回调格式错误").await;
            continue;
        };
        if parsed.path() != "/oauth/callback" {
            let _ = respond(&mut socket, 404, "Not found").await;
            continue;
        }

        let mut refresh = None;
        let mut error = None;
        for (key, value) in parsed.query_pairs() {
            match key.as_ref() {
                "refresh_token" => refresh = Some(value.to_string()),
                "error" => error = Some(value.to_string()),
                _ => {}
            }
        }

        if let Some(error) = error {
            let _ = respond(&mut socket, 400, "授权被拒绝，可以关闭此页面返回 SRL。").await;
            return Err(AppError::new(
                ErrorCode::AuthenticationRequired,
                format!("LoCyanFrp 授权被拒绝：{error}"),
            ));
        }
        if let Some(refresh) = refresh {
            let _ = respond(&mut socket, 200, "授权成功，可以关闭此页面返回 SRL。").await;
            return Ok(refresh);
        }
        let _ = respond(&mut socket, 400, "授权回调缺少 refresh_token。").await;
    }
}

async fn respond(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    message: &str,
) -> std::io::Result<()> {
    let reason = if status == 200 { "OK" } else { "Bad Request" };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>SRL</title></head>\
         <body style=\"font-family:-apple-system,Segoe UI,sans-serif;padding:48px;color:#1b1c19;background:#f5f4ef\">\
         <h2 style=\"font-weight:600\">SRL · Minecraft 联机</h2><p>{message}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.shutdown().await
}

async fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = tokio::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = tokio::process::Command::new("xdg-open");

    #[cfg(not(target_os = "windows"))]
    command.arg(url);

    command
        .spawn()
        .map_err(|e| AppError::internal(format!("无法打开系统浏览器：{e}")))?;
    Ok(())
}

fn decode_envelope(text: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(text).map_err(|e| {
        AppError::new(ErrorCode::Http, "LoCyanFrp API 返回了无法解析的响应")
            .with_detail(e.to_string())
    })?;
    let status = value.get("status").and_then(Value::as_i64).unwrap_or(200);
    if status != 200 {
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("LoCyanFrp API 返回错误");
        let code = match status {
            401 => ErrorCode::AuthenticationRequired,
            403 => ErrorCode::Http,
            429 => ErrorCode::Http,
            _ => ErrorCode::Http,
        };
        return Err(AppError::new(code, message.to_string())
            .with_detail(format!("status={status}"))
            .retryable(status == 429 || status >= 500));
    }
    Ok(value)
}

fn value_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
        .or_else(|| value.as_f64().map(|v| v as u64))
}

fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(v) if !v.trim().is_empty() => Some(v.clone()),
        Value::Number(v) => Some(v.to_string()),
        _ => None,
    }
}

fn find_string(data: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = data.get(*key) {
            if let Some(found) = value_string(value) {
                return Some(found);
            }
        }
    }
    None
}

fn find_u32(data: &Value, keys: &[&str]) -> Option<u32> {
    for key in keys {
        if let Some(value) = data.get(*key).and_then(value_u64) {
            return Some(value as u32);
        }
    }
    None
}

pub fn parse_account(value: &Value, fallback_id: u64) -> AccountView {
    let data = value.get("data").unwrap_or(value);
    let data = data.get("user").filter(|v| v.is_object()).unwrap_or(data);
    let user_id = data
        .get("id")
        .or_else(|| data.get("user_id"))
        .or_else(|| data.get("uid"))
        .and_then(value_u64)
        .unwrap_or(fallback_id);
    let display_name = find_string(data, &["username", "nickname", "name", "display_name"])
        .or_else(|| find_string(data, &["email"]))
        .unwrap_or_else(|| format!("用户 {user_id}"));
    let email = find_string(data, &["email"]);
    let group = data
        .get("group")
        .and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| v.get("name").and_then(Value::as_str).map(str::to_string))
        })
        .or_else(|| find_string(data, &["group_name", "user_group"]));
    let verified = data
        .get("verified")
        .and_then(Value::as_bool)
        .or_else(|| data.get("email_verified").and_then(Value::as_bool))
        .or_else(|| {
            data.get("verification_level")
                .and_then(Value::as_str)
                .map(|v| v != "NONE")
        });
    let tunnel_count = find_u32(
        data,
        &[
            "tunnel_count",
            "tunnels_count",
            "used_tunnels",
            "tunnels",
            "tunnel_used",
        ],
    );
    let tunnel_limit = find_u32(
        data,
        &[
            "tunnel_limit",
            "max_tunnel",
            "max_tunnels",
            "tunnel_max",
            "tunnel_quota",
        ],
    );
    AccountView {
        provider: PROVIDER_ID.into(),
        user_id,
        display_name,
        email,
        group,
        verified,
        tunnel_count,
        tunnel_limit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_payload_variants() {
        let value = serde_json::json!({
            "status": 200,
            "data": {
                "id": 42,
                "username": "max",
                "email": "max@example.com",
                "group": {"name": "VIP"},
                "verified": true,
                "tunnel_count": 1,
                "max_tunnels": "5"
            }
        });
        let account = parse_account(&value, 0);
        assert_eq!(account.user_id, 42);
        assert_eq!(account.display_name, "max");
        assert_eq!(account.group.as_deref(), Some("VIP"));
        assert_eq!(account.tunnel_count, Some(1));
        assert_eq!(account.tunnel_limit, Some(5));
    }

    #[test]
    fn rejects_non_200_envelope() {
        let error = decode_envelope(r#"{"status":403,"message":"无权限","data":{}}"#).unwrap_err();
        assert_eq!(error.message, "无权限");
    }
}
