//! Microsoft device-code OAuth, Xbox Live / XSTS and Minecraft services.
//!
//! Requires a public Azure application client id configured in settings. When
//! absent every entry point returns `NOT_CONFIGURED`; login is never faked.
//! Tokens are returned to the caller for secure storage and are never logged.

use crate::account::SecretStore;
use crate::download::ensure_trusted_url;
use crate::error::{AppError, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DEFAULT_SCOPE: &str = "XboxLive.signin offline_access";

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub login: String,
    pub xbox: String,
    pub xsts: String,
    pub minecraft: String,
    pub profile: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            login: "https://login.microsoftonline.com/consumers/oauth2/v2.0".into(),
            xbox: "https://user.auth.xboxlive.com/user/authenticate".into(),
            xsts: "https://xsts.auth.xboxlive.com/xsts/authorize".into(),
            minecraft: "https://api.minecraftservices.com/authentication/login_with_xbox".into(),
            profile: "https://api.minecraftservices.com/minecraft/profile".into(),
        }
    }
}

#[cfg(test)]
impl Endpoints {
    pub fn test(base: &str) -> Self {
        Self {
            login: format!("{base}/oauth"),
            xbox: format!("{base}/xbox"),
            xsts: format!("{base}/xsts"),
            minecraft: format!("{base}/minecraft"),
            profile: format!("{base}/profile"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeView {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrosoftPoll {
    /// pending | slowDown | authorized | expired | denied | error
    pub status: String,
    pub interval: Option<u64>,
    pub message: Option<String>,
    pub account: Option<crate::account::Account>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMicrosoftTokens {
    pub refresh_token: String,
    pub mc_access_token: String,
    /// Unix seconds.
    pub mc_expires_at: i64,
}

#[derive(Debug, Clone)]
pub struct MicrosoftSession {
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Clone)]
pub struct MicrosoftAuth {
    http: reqwest::Client,
    client_id: String,
    endpoints: Endpoints,
}

impl MicrosoftAuth {
    pub fn new(http: reqwest::Client, client_id: impl Into<String>) -> Self {
        Self {
            http,
            client_id: client_id.into(),
            endpoints: Endpoints::default(),
        }
    }

    #[cfg(test)]
    pub fn with_endpoints(
        http: reqwest::Client,
        client_id: impl Into<String>,
        endpoints: Endpoints,
    ) -> Self {
        Self {
            http,
            client_id: client_id.into(),
            endpoints,
        }
    }

    pub fn configured(&self) -> bool {
        !self.client_id.trim().is_empty()
    }

    fn parse_url(url: &str) -> Result<reqwest::Url> {
        if cfg!(test) && url.starts_with("http://127.0.0.1") {
            reqwest::Url::parse(url).map_err(|e| AppError::internal(format!("测试 URL 无效：{e}")))
        } else {
            ensure_trusted_url(url, false)
        }
    }

    async fn request_json(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> Result<(reqwest::StatusCode, Value)> {
        let parsed = Self::parse_url(url)?;
        let mut request = self
            .http
            .request(method, parsed)
            .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
            .timeout(std::time::Duration::from_secs(25));
        for (k, v) in headers {
            request = request.header(*k, *v);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok((status, value))
    }

    fn error_code(value: &Value) -> Option<&str> {
        value.get("error").and_then(|v| v.as_str())
    }

    pub async fn begin_device_code(&self) -> Result<DeviceCodeView> {
        if !self.configured() {
            return Err(
                AppError::new(ErrorCode::NotConfigured, "Microsoft 登录尚未配置")
                    .with_detail("请在设置中填写 Azure 应用的公开 client id。"),
            );
        }
        let url = format!("{}/devicecode", self.endpoints.login);
        let (status, value) = self
            .request_json(
                reqwest::Method::POST,
                &url,
                Some(json!({
                    "client_id": self.client_id,
                    "scope": DEFAULT_SCOPE,
                })),
                &[],
            )
            .await?;
        if !status.is_success() {
            return Err(AppError::new(
                ErrorCode::AuthenticationRequired,
                format!("无法开始 Microsoft 登录（HTTP {}）", status.as_u16()),
            )
            .with_detail(
                Self::error_code(&value)
                    .map(|c| format!("Microsoft 错误：{c}"))
                    .unwrap_or_default(),
            ));
        }
        Ok(DeviceCodeView {
            device_code: value
                .get("device_code")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AppError::internal("设备码响应缺少 device_code"))?
                .to_string(),
            user_code: value
                .get("user_code")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            verification_uri: value
                .get("verification_uri")
                .and_then(|v| v.as_str())
                .unwrap_or("https://microsoft.com/link")
                .to_string(),
            expires_in: value
                .get("expires_in")
                .and_then(|v| v.as_u64())
                .unwrap_or(900),
            interval: value.get("interval").and_then(|v| v.as_u64()).unwrap_or(5),
            message: value
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("请在浏览器中打开验证地址并输入代码。")
                .to_string(),
        })
    }

    async fn token_request(
        &self,
        device_code: Option<&str>,
        refresh_token: Option<&str>,
    ) -> Result<(reqwest::StatusCode, Value)> {
        let mut form = vec![("client_id", self.client_id.as_str())];
        if let Some(code) = device_code {
            form.push(("grant_type", "urn:ietf:params:oauth:grant-type:device_code"));
            form.push(("device_code", code));
        } else if let Some(token) = refresh_token {
            form.push(("grant_type", "refresh_token"));
            form.push(("refresh_token", token));
            form.push(("scope", DEFAULT_SCOPE));
        }
        let url = format!("{}/token", self.endpoints.login);
        let parsed = reqwest::Url::parse(&url)
            .map_err(|e| AppError::internal(format!("登录 URL 无效：{e}")))?;
        let response = self
            .http
            .post(parsed)
            .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
            .timeout(std::time::Duration::from_secs(25))
            .form(&form)
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
    }

    /// Poll once and, when authorized, return the full session. The frontend
    /// receives only non-secret status; the caller stores the tokens.
    pub async fn complete_device_login(
        &self,
        device_code: &str,
    ) -> Result<(MicrosoftPoll, Option<MicrosoftSession>)> {
        let (status, value) = self.token_request(Some(device_code), None).await?;
        if status.is_success() {
            let ms_access = value
                .get("access_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AppError::internal("令牌响应缺少 access_token"))?;
            let refresh = value
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AppError::internal("令牌响应缺少 refresh_token"))?;
            let session = self.exchange_for_minecraft(ms_access, refresh).await?;
            return Ok((
                MicrosoftPoll {
                    status: "authorized".into(),
                    interval: None,
                    message: None,
                    account: None,
                },
                Some(session),
            ));
        }
        let code = Self::error_code(&value).unwrap_or("unknown_error");
        let poll = match code {
            "authorization_pending" => MicrosoftPoll {
                status: "pending".into(),
                interval: None,
                message: None,
                account: None,
            },
            "slow_down" => MicrosoftPoll {
                status: "slowDown".into(),
                interval: Some(10),
                message: None,
                account: None,
            },
            "expired_token" | "bad_verification_code" => MicrosoftPoll {
                status: "expired".into(),
                interval: None,
                message: Some("设备码已过期，请重新开始登录。".into()),
                account: None,
            },
            "authorization_declined" => MicrosoftPoll {
                status: "denied".into(),
                interval: None,
                message: Some("用户拒绝了授权。".into()),
                account: None,
            },
            other => MicrosoftPoll {
                status: "error".into(),
                interval: None,
                message: Some(format!("Microsoft 登录错误：{other}")),
                account: None,
            },
        };
        Ok((poll, None))
    }

    async fn exchange_for_minecraft(
        &self,
        ms_access: &str,
        refresh_token: &str,
    ) -> Result<MicrosoftSession> {
        let (xbl_token, _uhs) = self.xbox_authenticate(ms_access).await?;
        let (xsts_token, xsts_uhs) = self.xsts_authorize(&xbl_token).await?;
        let (mc_token, expires_in) = self.minecraft_login(&xsts_uhs, &xsts_token).await?;
        let (uuid, name) = self.minecraft_profile(&mc_token).await?;
        Ok(MicrosoftSession {
            username: name,
            uuid,
            access_token: mc_token,
            refresh_token: refresh_token.to_string(),
            expires_in,
        })
    }

    async fn xbox_authenticate(&self, ms_access: &str) -> Result<(String, String)> {
        let (status, value) = self
            .request_json(
                reqwest::Method::POST,
                &self.endpoints.xbox,
                Some(json!({
                    "Properties": {
                        "AuthMethod": "RPS",
                        "SiteName": "user.auth.xboxlive.com",
                        "RpsTicket": format!("d={ms_access}")
                    },
                    "RelyingParty": "http://auth.xboxlive.com",
                    "TokenType": "JWT"
                })),
                &[("x-xbl-contract-version", "1")],
            )
            .await?;
        if !status.is_success() {
            return Err(
                AppError::new(ErrorCode::AuthenticationRequired, "Xbox Live 身份验证失败")
                    .with_detail(format!("HTTP {}", status.as_u16())),
            );
        }
        let token = value
            .get("Token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::internal("Xbox 响应缺少 Token"))?;
        let uhs = value
            .get("DisplayClaims")
            .and_then(|v| v.get("xui"))
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("uhs"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::internal("Xbox 响应缺少 uhs"))?;
        Ok((token.to_string(), uhs.to_string()))
    }

    async fn xsts_authorize(&self, xbl_token: &str) -> Result<(String, String)> {
        let (status, value) = self
            .request_json(
                reqwest::Method::POST,
                &self.endpoints.xsts,
                Some(json!({
                    "Properties": {
                        "SandboxId": "RETAIL",
                        "UserTokens": [xbl_token]
                    },
                    "RelyingParty": "rp://api.minecraftservices.com/",
                    "TokenType": "JWT"
                })),
                &[("x-xbl-contract-version", "1")],
            )
            .await?;
        if !status.is_success() {
            let xerr = value.get("XErr").and_then(|v| v.as_u64());
            let msg = match xerr {
                Some(2148916233) => "该 Microsoft 账号没有 Xbox 档案，请先在 Xbox 官网创建。",
                Some(2148916238) => "该账号是未成年人账号，需要加入家庭组后才能登录。",
                Some(2148916227) => "该 Xbox 账号已被封禁。",
                _ => "XSTS 授权失败，请检查账号状态或稍后重试。",
            };
            return Err(AppError::new(ErrorCode::AuthenticationRequired, msg)
                .with_detail(format!("XErr={}", xerr.unwrap_or(0))));
        }
        let token = value
            .get("Token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::internal("XSTS 响应缺少 Token"))?;
        let uhs = value
            .get("DisplayClaims")
            .and_then(|v| v.get("xui"))
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("uhs"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::internal("XSTS 响应缺少 uhs"))?;
        Ok((token.to_string(), uhs.to_string()))
    }

    async fn minecraft_login(&self, uhs: &str, xsts: &str) -> Result<(String, u64)> {
        let (status, value) = self
            .request_json(
                reqwest::Method::POST,
                &self.endpoints.minecraft,
                Some(json!({
                    "identityToken": format!("XBL3.0 x={uhs};{xsts}")
                })),
                &[],
            )
            .await?;
        if !status.is_success() {
            return Err(
                AppError::new(ErrorCode::AuthenticationRequired, "Minecraft 服务登录失败")
                    .with_detail(format!("HTTP {}", status.as_u16())),
            );
        }
        let token = value
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::internal("Minecraft 响应缺少 access_token"))?;
        let expires_in = value
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(86_400);
        Ok((token.to_string(), expires_in))
    }

    async fn minecraft_profile(&self, mc_token: &str) -> Result<(String, String)> {
        let parsed = Self::parse_url(&self.endpoints.profile)?;
        let response = self
            .http
            .get(parsed)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {mc_token}"))
            .header(reqwest::header::USER_AGENT, crate::USER_AGENT)
            .timeout(std::time::Duration::from_secs(25))
            .send()
            .await?;
        let status = response.status();
        if status.as_u16() == 404 {
            return Err(AppError::new(
                ErrorCode::AuthenticationRequired,
                "该 Microsoft 账号未拥有 Minecraft Java Edition",
            ));
        }
        if !status.is_success() {
            return Err(AppError::new(
                ErrorCode::AuthenticationRequired,
                format!("读取 Minecraft 档案失败（HTTP {}）", status.as_u16()),
            ));
        }
        let value: Value = response.json().await?;
        let uuid = value
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::internal("Minecraft 档案缺少 id"))?;
        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::internal("Minecraft 档案缺少 name"))?;
        Ok((uuid.to_string(), name.to_string()))
    }

    /// Refresh Microsoft + Minecraft tokens. Returns a fresh session.
    pub async fn refresh(&self, refresh_token: &str) -> Result<MicrosoftSession> {
        if !self.configured() {
            return Err(AppError::new(
                ErrorCode::NotConfigured,
                "Microsoft 登录尚未配置",
            ));
        }
        let (status, value) = self.token_request(None, Some(refresh_token)).await?;
        if !status.is_success() {
            return Err(AppError::new(
                ErrorCode::AuthenticationRequired,
                "Microsoft 授权已过期，请重新登录",
            ));
        }
        let ms_access = value
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::internal("刷新响应缺少 access_token"))?;
        let new_refresh = value
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .unwrap_or(refresh_token);
        self.exchange_for_minecraft(ms_access, new_refresh).await
    }
}

/// Load tokens for an account from the OS credential store.
pub fn load_tokens(
    store: &dyn SecretStore,
    account_id: &str,
) -> Result<Option<StoredMicrosoftTokens>> {
    let key = format!("microsoft:{account_id}");
    let Some(raw) = store.get(&key)? else {
        return Ok(None);
    };
    serde_json::from_str(&raw).map(Some).map_err(|e| {
        AppError::new(ErrorCode::NotConfigured, "系统凭据中的令牌数据损坏")
            .with_detail(e.to_string())
    })
}

pub fn save_tokens(
    store: &dyn SecretStore,
    account_id: &str,
    session: &MicrosoftSession,
) -> Result<StoredMicrosoftTokens> {
    let tokens = StoredMicrosoftTokens {
        refresh_token: session.refresh_token.clone(),
        mc_access_token: session.access_token.clone(),
        mc_expires_at: chrono::Utc::now().timestamp() + session.expires_in as i64 - 60,
    };
    let key = format!("microsoft:{account_id}");
    store.set(&key, &serde_json::to_string(&tokens)?)?;
    Ok(tokens)
}

pub fn player_from_tokens(
    account: &crate::account::Account,
    tokens: &StoredMicrosoftTokens,
) -> crate::version::PlayerProfile {
    crate::version::PlayerProfile {
        name: account.username.clone(),
        uuid: account.uuid.clone(),
        access_token: tokens.mc_access_token.clone(),
        user_type: "msa".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::MemorySecretStore;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn mock_server() -> (std::net::SocketAddr, Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let rec = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let rec = rec.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let mut raw = Vec::new();
                    loop {
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let req = String::from_utf8_lossy(&raw).into_owned();
                    let path = req
                        .lines()
                        .next()
                        .unwrap_or("")
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("")
                        .to_string();
                    rec.lock().unwrap().push(format!("{path}\n{req}"));
                    let body = if path.contains("/oauth/devicecode") {
                        r#"{"device_code":"dc","user_code":"ABCD","verification_uri":"https://microsoft.com/link","expires_in":900,"interval":5,"message":"go"}"#.to_string()
                    } else if path.contains("/oauth/token") {
                        if req.contains("refresh_token") {
                            r#"{"access_token":"ms2","refresh_token":"rt2","expires_in":3600}"#
                                .to_string()
                        } else {
                            r#"{"access_token":"ms1","refresh_token":"rt1","expires_in":3600}"#
                                .to_string()
                        }
                    } else if path.contains("/xbox") {
                        r#"{"Token":"xbl","DisplayClaims":{"xui":[{"uhs":"uhs1"}]}}"#.to_string()
                    } else if path.contains("/xsts") {
                        r#"{"Token":"xsts","DisplayClaims":{"xui":[{"uhs":"uhs2"}]}}"#.to_string()
                    } else if path.contains("/minecraft") && !path.contains("/profile") {
                        r#"{"access_token":"mc-token","expires_in":86400}"#.to_string()
                    } else if path.contains("/profile") {
                        r#"{"id":"uuid-1","name":"Steve"}"#.to_string()
                    } else {
                        "{}".to_string()
                    };
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(), body
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                });
            }
        });
        (addr, requests)
    }

    #[tokio::test]
    async fn full_device_flow_and_refresh() {
        let (addr, requests) = mock_server().await;
        let auth = MicrosoftAuth::with_endpoints(
            reqwest::Client::new(),
            "client-id",
            Endpoints::test(&format!("http://{addr}")),
        );
        let code = auth.begin_device_code().await.unwrap();
        assert_eq!(code.user_code, "ABCD");
        let (poll, session) = auth.complete_device_login("dc").await.unwrap();
        assert_eq!(poll.status, "authorized");
        let session = session.unwrap();
        assert_eq!(session.username, "Steve");
        assert_eq!(session.uuid, "uuid-1");
        assert_eq!(session.access_token, "mc-token");

        let refreshed = auth.refresh("rt1").await.unwrap();
        assert_eq!(refreshed.refresh_token, "rt2");
        assert_eq!(refreshed.access_token, "mc-token");

        let reqs = requests.lock().unwrap();
        assert!(reqs.iter().any(|r| r.starts_with("/oauth/devicecode")));
        assert!(reqs.iter().any(|r| r.starts_with("/xbox")));
        assert!(reqs.iter().any(|r| r.starts_with("/xsts")));
        assert!(reqs.iter().any(|r| r.starts_with("/minecraft\n")));
    }

    #[test]
    fn token_storage_roundtrip() {
        let store = MemorySecretStore::default();
        let session = MicrosoftSession {
            username: "Steve".into(),
            uuid: "u".into(),
            access_token: "mc".into(),
            refresh_token: "rt".into(),
            expires_in: 3600,
        };
        let stored = save_tokens(&store, "acc", &session).unwrap();
        assert!(stored.mc_expires_at > chrono::Utc::now().timestamp());
        let loaded = load_tokens(&store, "acc").unwrap().unwrap();
        assert_eq!(loaded.refresh_token, "rt");
        assert!(store.get("microsoft:acc").unwrap().unwrap().contains("mc"));
    }

    #[tokio::test]
    async fn unconfigured_login_is_explicit() {
        let auth = MicrosoftAuth::new(reqwest::Client::new(), "");
        let err = auth.begin_device_code().await.unwrap_err();
        assert_eq!(err.code, ErrorCode::NotConfigured);
    }
}
