use crate::error::{AppError, ErrorCode, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::provider::{ProviderNode, ProviderTunnel, TunnelCreate, TunnelProvider};
use super::token::Frp52TokenStore;
use super::types::{AccountView, API_BASE, PROVIDER_ID, PROVIDER_NAME};

pub struct Frp52Provider {
    http: reqwest::Client,
    tokens: Arc<Frp52TokenStore>,
}

impl Frp52Provider {
    pub fn new(http: reqwest::Client, tokens: Arc<Frp52TokenStore>) -> Self {
        Self { http, tokens }
    }

    async fn send<F>(&self, build: F) -> Result<Value>
    where
        F: Fn(&str) -> reqwest::RequestBuilder,
    {
        let token = self.tokens.require().await?;
        self.send_with_token(&token, build).await
    }

    async fn send_with_token<F>(&self, token: &str, build: F) -> Result<Value>
    where
        F: Fn(&str) -> reqwest::RequestBuilder,
    {
        let response = build(token)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| AppError::network("无法连接 52FRP API").with_detail(e.to_string()))?;
        parse_response(response).await
    }

    async fn get_data(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.send(|token| {
            let mut req = self
                .http
                .get(format!("{API_BASE}{path}"))
                .bearer_auth(token);
            if !query.is_empty() {
                req = req.query(query);
            }
            req
        })
        .await
    }

    async fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        self.send(|token| {
            self.http
                .post(format!("{API_BASE}{path}"))
                .bearer_auth(token)
                .json(body)
        })
        .await
    }

    async fn delete(&self, path: &str) -> Result<Value> {
        self.send(|token| {
            self.http
                .delete(format!("{API_BASE}{path}"))
                .bearer_auth(token)
        })
        .await
    }

    /// Read the official tunnel detail. `cur_conns` is a TCP connection count,
    /// not a player count, and the UI must label it that way.
    pub async fn tunnel_state(&self, tunnel_id: u64) -> Result<super::types::TunnelStateView> {
        let data = self.get_data(&format!("/tunnels/{tunnel_id}"), &[]).await?;
        Ok(parse_tunnel_state(&data))
    }
}

#[async_trait]
impl TunnelProvider for Frp52Provider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    async fn account(&self) -> Result<AccountView> {
        let data = self.get_data("/account", &[]).await?;
        parse_account(&data, None)
    }

    async fn account_with_token(&self, token: &str) -> Result<AccountView> {
        let data = self
            .send_with_token(token, |token| {
                self.http
                    .get(format!("{API_BASE}/account"))
                    .bearer_auth(token)
            })
            .await?;
        parse_account(&data, None)
    }

    async fn nodes(&self) -> Result<Vec<ProviderNode>> {
        let data = self.get_data("/nodes", &[]).await?;
        let list = data
            .as_array()
            .ok_or_else(|| AppError::new(ErrorCode::Http, "52FRP 节点列表格式异常"))?;
        Ok(list.iter().filter_map(parse_node).collect())
    }

    async fn node_load(&self) -> Result<HashMap<u64, f64>> {
        let data = self.get_data("/nodes/load", &[]).await?;
        let mut map = HashMap::new();
        if let Some(object) = data.as_object() {
            for (key, value) in object {
                if let (Ok(id), Some(load)) = (key.parse::<u64>(), value.as_f64()) {
                    map.insert(id, load);
                }
            }
        }
        Ok(map)
    }

    async fn check_name(&self, name: &str) -> Result<bool> {
        let data = self
            .get_data("/tunnels/check-name", &[("name", name.to_string())])
            .await?;
        Ok(data
            .get("available")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && data.get("valid").and_then(Value::as_bool).unwrap_or(true))
    }

    async fn check_port(&self, node_id: u64, proxy_type: &str, port: u16) -> Result<bool> {
        let data = self
            .get_data(
                "/tunnels/check-port",
                &[
                    ("node_id", node_id.to_string()),
                    ("port", port.to_string()),
                    ("proxy_type", proxy_type.to_string()),
                ],
            )
            .await?;
        let valid = data.get("valid").and_then(Value::as_bool).unwrap_or(false);
        let conflict = data
            .get("conflict")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        Ok(valid && !conflict)
    }

    async fn occupied_ports(&self, node_id: u64, proxy_type: &str) -> Result<Vec<u16>> {
        let data = self
            .get_data(
                "/tunnels/occupied-ports",
                &[
                    ("node_id", node_id.to_string()),
                    ("proxy_type", proxy_type.to_string()),
                ],
            )
            .await?;
        Ok(data
            .get("ports")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| {
                        v.as_str()
                            .and_then(|s| s.parse::<u16>().ok())
                            .or_else(|| v.as_u64().and_then(|n| u16::try_from(n).ok()))
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn list_tunnels(&self) -> Result<Vec<ProviderTunnel>> {
        let started = std::time::Instant::now();
        let mut page = 1u64;
        let mut tunnels: Vec<ProviderTunnel> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        loop {
            if started.elapsed() > std::time::Duration::from_secs(20) {
                return Err(AppError::new(ErrorCode::Timeout, "52FRP 隧道列表查询超时"));
            }
            let data = self
                .get_data(
                    "/tunnels",
                    &[("page", page.to_string()), ("page_size", "100".into())],
                )
                .await?;
            let list = data.get("list").and_then(Value::as_array).ok_or_else(|| {
                AppError::new(ErrorCode::Http, "52FRP 隧道列表响应缺少 list 字段")
            })?;
            for value in list {
                if let Some(tunnel) = parse_tunnel(value) {
                    if seen.insert(tunnel.id) {
                        tunnels.push(tunnel);
                    }
                }
            }
            let last_page = data
                .get("last_page")
                .and_then(value_u64)
                .or_else(|| data.get("total_page").and_then(value_u64))
                .or_else(|| data.get("total_pages").and_then(value_u64))
                .or_else(|| {
                    data.get("pagination")
                        .and_then(|p| p.get("last_page"))
                        .and_then(value_u64)
                });
            if let Some(last) = last_page {
                if page >= last {
                    break;
                }
                page += 1;
                continue;
            }
            // No pagination metadata: a short page is the only safe terminator.
            if list.len() < 100 {
                break;
            }
            page += 1;
            if page > 50 {
                return Err(AppError::new(
                    ErrorCode::Http,
                    "52FRP 隧道列表超过 50 页，已停止读取",
                ));
            }
        }
        Ok(tunnels)
    }

    async fn create_tunnel(&self, request: &TunnelCreate) -> Result<u64> {
        let body = serde_json::json!({
            "proxy_name": request.proxy_name,
            "proxy_type": request.proxy_type,
            "node_id": request.node_id,
            "local_ip": request.local_ip,
            "local_port": request.local_port,
            "remote_port": request.remote_port,
        });
        let data = self.post_json("/tunnels", &body).await?;
        data.get("id")
            .and_then(Value::as_u64)
            .ok_or_else(|| AppError::new(ErrorCode::Http, "52FRP 未返回隧道 ID"))
    }

    async fn delete_tunnel(&self, tunnel_id: u64) -> Result<()> {
        let _ = self.delete(&format!("/tunnels/{tunnel_id}")).await?;
        Ok(())
    }

    async fn tunnel_online(&self, tunnel_id: u64) -> Result<bool> {
        let data = self.get_data(&format!("/tunnels/{tunnel_id}"), &[]).await?;
        let proxy = data.get("proxy").unwrap_or(&data);
        Ok(proxy.get("is_online").map(value_bool).unwrap_or(false))
    }

    async fn tunnel_config(&self, node_id: u64, tunnel_id: u64) -> Result<String> {
        let data = self
            .get_data(
                &format!("/nodes/{node_id}/config/file"),
                &[
                    ("proxy_ids", tunnel_id.to_string()),
                    ("format", "toml".into()),
                ],
            )
            .await?;
        data.get("content")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| AppError::new(ErrorCode::Http, "52FRP 未返回 FRPC 配置"))
    }
}

async fn parse_response(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    let text = match response.text().await {
        Ok(text) => text,
        Err(error) => {
            return Err(
                AppError::new(ErrorCode::Network, format!("读取 52FRP 响应失败：{error}"))
                    .retryable(true),
            )
        }
    };
    let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() || value.get("ok").and_then(Value::as_bool) != Some(true) {
        let code = value
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let message = value
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("52FRP 返回 HTTP {}", status.as_u16()));
        let error_code = match (status.as_u16(), code) {
            (401, _) => ErrorCode::AuthenticationRequired,
            (404, _) => ErrorCode::NotFound,
            (_, "unauthenticated") => ErrorCode::AuthenticationRequired,
            (_, "not_found") => ErrorCode::NotFound,
            (_, "validation_error") => ErrorCode::InvalidInput,
            _ => ErrorCode::Http,
        };
        let summary: String = crate::util::redact_secrets(&text)
            .chars()
            .take(300)
            .collect();
        return Err(AppError::new(error_code, message)
            .with_detail(format!(
                "HTTP {} · {code} · {}",
                status.as_u16(),
                summary.replace('\n', " ")
            ))
            .retryable(
                status.as_u16() == 429 || status.is_server_error() || status.as_u16() == 409,
            ));
    }
    Ok(value.get("data").cloned().unwrap_or(Value::Null))
}

fn parse_account(data: &Value, fallback_id: Option<u64>) -> Result<AccountView> {
    let data = data.get("data").unwrap_or(data);
    let user_id = data
        .get("id")
        .and_then(value_u64)
        .or(fallback_id)
        .ok_or_else(|| AppError::new(ErrorCode::Http, "52FRP 未返回用户 ID"))?;
    let display_name = data
        .get("nickname")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .or_else(|| data.get("username").and_then(Value::as_str))
        .unwrap_or("52FRP 用户")
        .to_string();
    let proxies = data.get("proxies");
    let tunnel_count = proxies
        .and_then(|p| p.get("use"))
        .and_then(value_u64)
        .map(|v| v as u32);
    let tunnel_limit = proxies
        .and_then(|p| p.get("max"))
        .and_then(|v| v.as_i64())
        .filter(|v| *v >= 0)
        .map(|v| v as u32);
    let traffic_remaining = data
        .get("traffic")
        .and_then(|t| t.get("total_remaining"))
        .and_then(value_u64);
    Ok(AccountView {
        provider: PROVIDER_ID.into(),
        user_id,
        display_name,
        email: data
            .get("email")
            .and_then(Value::as_str)
            .map(str::to_string),
        group: data
            .get("group")
            .and_then(Value::as_str)
            .map(str::to_string),
        verified: data.get("identity_verified").and_then(Value::as_bool),
        tunnel_count,
        tunnel_limit,
        balance: data.get("balance").and_then(Value::as_f64),
        traffic_remaining,
    })
}

fn parse_node(value: &Value) -> Option<ProviderNode> {
    let mut protocols: Vec<String> = value
        .get("tunnel_protocols")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_ascii_lowercase()))
                .collect()
        })
        .unwrap_or_default();
    if protocols.is_empty() {
        if let Some(raw) = value
            .get("supported_tunnel_protocols")
            .and_then(Value::as_str)
        {
            if let Ok(parsed) = serde_json::from_str::<Vec<String>>(raw) {
                protocols = parsed.into_iter().map(|v| v.to_ascii_lowercase()).collect();
            }
        }
    }
    Some(ProviderNode {
        id: value.get("id").and_then(value_u64)?,
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("未命名节点")
            .to_string(),
        host: value
            .get("hostname")
            .and_then(Value::as_str)
            .or_else(|| value.get("ip").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string(),
        ip: value.get("ip").and_then(Value::as_str).map(str::to_string),
        city: value
            .get("city")
            .and_then(Value::as_str)
            .map(str::to_string),
        province: value
            .get("province")
            .and_then(Value::as_str)
            .map(str::to_string),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string),
        available: value
            .get("available")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        is_online: value
            .get("is_online")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        proxy_count: value.get("proxy_count").and_then(value_u64).unwrap_or(0) as u32,
        max_proxies: value
            .get("max_proxies")
            .and_then(Value::as_i64)
            .unwrap_or(-1),
        tunnel_protocols: protocols,
        proxies_allowed: value
            .get("proxies_allowed")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        proxies_denied: value
            .get("proxies_denied")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        tcp_port: value
            .get("tcp_port")
            .and_then(value_u64)
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(7000),
        load: None,
    })
}

fn parse_tunnel(value: &Value) -> Option<ProviderTunnel> {
    Some(ProviderTunnel {
        id: value.get("id").and_then(value_u64)?,
        name: value
            .get("proxy_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        tunnel_type: value
            .get("proxy_type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase(),
        local_port: value
            .get("local_port")
            .and_then(value_u64)
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(0),
        remote_port: value.get("remote_port").and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<u16>().ok())
                .or_else(|| v.as_u64().and_then(|n| u16::try_from(n).ok()))
        }),
        node_id: value.get("node_id").and_then(value_u64),
        node_name: value
            .get("node_name")
            .and_then(Value::as_str)
            .map(str::to_string),
        node_hostname: value
            .get("node_hostname")
            .and_then(Value::as_str)
            .map(str::to_string),
        is_online: value.get("is_online").map(value_bool).unwrap_or(false),
    })
}

pub fn parse_tunnel_state(data: &Value) -> super::types::TunnelStateView {
    let payload = data
        .get("data")
        .filter(|inner| inner.get("proxy").is_some())
        .unwrap_or(data);
    let proxy = payload.get("proxy").unwrap_or(payload);
    super::types::TunnelStateView {
        online: proxy.get("is_online").map(value_bool).unwrap_or(false),
        enabled: proxy.get("enabled").and_then(Value::as_bool),
        connections: proxy.get("cur_conns").and_then(value_u64),
        status: proxy.get("status").and_then(value_u64),
        checked_at: chrono::Utc::now().to_rfc3339(),
    }
}

fn value_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
        .or_else(|| value.as_f64().map(|v| v as u64))
}

fn value_bool(value: &Value) -> bool {
    value
        .as_bool()
        .or_else(|| value.as_u64().map(|v| v != 0))
        .or_else(|| value.as_str().map(|v| v == "1" || v == "true"))
        .unwrap_or(false)
}

/// Parse 52FRP's `proxies_allowed` / `proxies_denied` strings into closed
/// ranges. Accepts comma/space/semicolon separated `20000-30000` or `20000`.
pub fn parse_port_ranges(raw: &str) -> Vec<(u16, u16)> {
    raw.split([',', ';', ' ', '\n', '\t'])
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            if let Some((start, end)) = part.split_once('-') {
                let start = start.trim().parse::<u16>().ok()?;
                let end = end.trim().parse::<u16>().ok()?;
                if start <= end {
                    Some((start, end))
                } else {
                    None
                }
            } else {
                let port = part.parse::<u16>().ok()?;
                Some((port, port))
            }
        })
        .collect()
}

pub fn port_in_ranges(ranges: &[(u16, u16)], port: u16) -> bool {
    ranges
        .iter()
        .any(|(start, end)| (*start..=*end).contains(&port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_port_ranges() {
        let ranges = parse_port_ranges("20000-30000, 40000; 50000");
        assert_eq!(ranges, vec![(20000, 30000), (40000, 40000), (50000, 50000)]);
        assert!(port_in_ranges(&ranges, 25565));
        assert!(!port_in_ranges(&ranges, 60000));
    }

    #[test]
    fn parses_tunnel_detail_without_inventing_player_counts() {
        let data = serde_json::json!({
            "ok": true,
            "data": {
                "proxy": {
                    "id": 9,
                    "is_online": 1,
                    "enabled": true,
                    "cur_conns": 3,
                    "status": 0
                }
            }
        });
        let state = parse_tunnel_state(&data);
        assert!(state.online);
        assert_eq!(state.enabled, Some(true));
        assert_eq!(state.connections, Some(3));
        assert_eq!(state.status, Some(0));
    }

    #[test]
    fn parses_account_shape() {
        let data = serde_json::json!({
            "ok": true,
            "data": {
                "id": 7, "username": "max", "email": "max@example.com",
                "identity_verified": true, "balance": 12.5,
                "proxies": {"use": 2, "max": 5},
                "traffic": {"total_remaining": 1048576}
            }
        });
        let account = parse_account(&data, None).unwrap();
        assert_eq!(account.user_id, 7);
        assert_eq!(account.tunnel_count, Some(2));
        assert_eq!(account.tunnel_limit, Some(5));
        assert_eq!(account.traffic_remaining, Some(1048576));
    }
}
