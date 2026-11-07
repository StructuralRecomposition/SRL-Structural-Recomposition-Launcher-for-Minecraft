use crate::error::{AppError, ErrorCode, Result};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

use super::auth::LoCyanAuth;
use super::provider::{NodeStat, ProviderNode, ProviderTunnel, TunnelCreate, TunnelProvider};
use super::types::{AccountView, API_BACKUP_BASE, API_BASE, PROVIDER_ID, PROVIDER_NAME};

pub struct LoCyanFrpProvider {
    http: reqwest::Client,
    auth: Arc<LoCyanAuth>,
}

impl LoCyanFrpProvider {
    pub fn new(http: reqwest::Client, auth: Arc<LoCyanAuth>) -> Self {
        Self { http, auth }
    }

    async fn user_id(&self) -> Result<u64> {
        self.auth.cached_user_id().await.ok_or_else(|| {
            AppError::new(ErrorCode::AuthenticationRequired, "尚未连接 LoCyanFrp 账户")
        })
    }

    async fn send<F>(&self, build: F) -> Result<reqwest::Response>
    where
        F: Fn(&str) -> reqwest::RequestBuilder,
    {
        let token = self.auth.access_token().await?;
        let response = build(&token)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| AppError::network("无法连接 LoCyanFrp API").with_detail(e.to_string()))?;
        if response.status().as_u16() == 401 {
            let token = self.auth.force_refresh().await?;
            return build(&token)
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(|e| {
                    AppError::network("无法连接 LoCyanFrp API").with_detail(e.to_string())
                });
        }
        Ok(response)
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: Vec<(String, String)>,
    ) -> Result<T> {
        let mut last_error = None;
        for base in [API_BASE, API_BACKUP_BASE] {
            let url = format!("{base}{path}");
            let response = self
                .send(|token| self.http.get(&url).bearer_auth(token).query(&query))
                .await;
            match response {
                Ok(response) => return parse_json(response).await,
                Err(error) if matches!(error.code, ErrorCode::Network | ErrorCode::Timeout) => {
                    last_error = Some(error)
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| AppError::network("无法连接 LoCyanFrp API")))
    }

    async fn put_form<T: DeserializeOwned>(
        &self,
        path: &str,
        form: Vec<(String, String)>,
    ) -> Result<T> {
        let url = format!("{API_BASE}{path}");
        let response = self
            .send(|token| self.http.put(&url).bearer_auth(token).form(&form))
            .await?;
        parse_json(response).await
    }

    async fn delete_query<T: DeserializeOwned>(
        &self,
        path: &str,
        query: Vec<(String, String)>,
    ) -> Result<T> {
        let url = format!("{API_BASE}{path}");
        let response = self
            .send(|token| self.http.delete(&url).bearer_auth(token).query(&query))
            .await?;
        parse_json(response).await
    }
}

#[async_trait]
impl TunnelProvider for LoCyanFrpProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn name(&self) -> &'static str {
        PROVIDER_NAME
    }

    async fn account(&self) -> Result<AccountView> {
        self.auth.account().await
    }

    async fn nodes(&self) -> Result<Vec<ProviderNode>> {
        let user_id = self.user_id().await?;
        let mut page = 1u32;
        let mut nodes = Vec::new();
        loop {
            let value: Value = self
                .get_json(
                    "/nodes",
                    vec![
                        ("user_id".into(), user_id.to_string()),
                        ("page".into(), page.to_string()),
                        ("size".into(), "100".into()),
                    ],
                )
                .await?;
            let list = value
                .get("data")
                .and_then(|d| d.get("list"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if list.is_empty() {
                break;
            }
            nodes.extend(list.iter().filter_map(parse_node));
            let pages = value
                .get("data")
                .and_then(|d| d.get("pagination"))
                .and_then(|p| p.get("count"))
                .and_then(Value::as_u64)
                .unwrap_or(page as u64);
            if page as u64 >= pages {
                break;
            }
            page += 1;
        }
        Ok(nodes)
    }

    async fn node_stats(&self, node_ids: &[u64]) -> Result<Vec<NodeStat>> {
        if node_ids.is_empty() {
            return Ok(Vec::new());
        }
        let user_id = self.user_id().await?;
        let mut query = vec![("user_id".to_string(), user_id.to_string())];
        for id in node_ids {
            query.push(("nodes[]".into(), id.to_string()));
        }
        let value: Value = match self.get_json("/nodes/stats", query).await {
            Ok(value) => value,
            Err(error) if error.code == ErrorCode::Http => {
                let mut fallback = vec![("user_id".to_string(), user_id.to_string())];
                fallback.push((
                    "nodes".into(),
                    node_ids
                        .iter()
                        .map(u64::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                ));
                self.get_json("/nodes/stats", fallback).await?
            }
            Err(error) => return Err(error),
        };
        Ok(value
            .get("data")
            .and_then(|d| d.get("list"))
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|item| {
                        Some(NodeStat {
                            node_id: item.get("node_id").and_then(value_u64)?,
                            inbound: item.get("inbound").and_then(value_f64),
                            outbound: item.get("outbound").and_then(value_f64),
                            clients: item
                                .get("total_client")
                                .or_else(|| item.get("client_count"))
                                .and_then(value_u64)
                                .map(|v| v as u32),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn random_port(&self, node_id: u64) -> Result<u16> {
        let user_id = self.user_id().await?;
        let value: Value = self
            .get_json(
                "/node/port",
                vec![
                    ("user_id".into(), user_id.to_string()),
                    ("node_id".into(), node_id.to_string()),
                ],
            )
            .await?;
        value
            .get("data")
            .and_then(|d| d.get("port"))
            .and_then(value_u64)
            .and_then(|v| u16::try_from(v).ok())
            .ok_or_else(|| AppError::new(ErrorCode::Http, "LoCyanFrp 未返回可用远程端口"))
    }

    async fn list_tunnels(&self) -> Result<Vec<ProviderTunnel>> {
        let user_id = self.user_id().await?;
        let value: Value = self
            .get_json(
                "/tunnels",
                vec![
                    ("user_id".into(), user_id.to_string()),
                    ("page".into(), "1".into()),
                    ("size".into(), "100".into()),
                ],
            )
            .await?;
        Ok(value
            .get("data")
            .and_then(|d| d.get("list"))
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(parse_tunnel).collect())
            .unwrap_or_default())
    }

    async fn create_tunnel(&self, request: &TunnelCreate) -> Result<u64> {
        let user_id = self.user_id().await?;
        let form = vec![
            ("user_id".to_string(), user_id.to_string()),
            ("node_id".to_string(), request.node_id.to_string()),
            ("name".to_string(), request.name.clone()),
            ("type".to_string(), request.tunnel_type.clone()),
            ("local_ip".to_string(), request.local_ip.clone()),
            ("local_port".to_string(), request.local_port.to_string()),
            ("remote_port".to_string(), request.remote_port.to_string()),
            (
                "use_encryption".to_string(),
                request.use_encryption.to_string(),
            ),
            (
                "use_compression".to_string(),
                request.use_compression.to_string(),
            ),
        ];
        let value: Value = self.put_form("/tunnel", form).await?;
        value
            .get("data")
            .and_then(|d| d.get("tunnel_id"))
            .and_then(value_u64)
            .ok_or_else(|| AppError::new(ErrorCode::Http, "LoCyanFrp 未返回隧道 ID"))
    }

    async fn delete_tunnel(&self, tunnel_id: u64) -> Result<()> {
        let user_id = self.user_id().await?;
        let _: Value = self
            .delete_query(
                "/tunnel",
                vec![
                    ("user_id".into(), user_id.to_string()),
                    ("tunnel_id".into(), tunnel_id.to_string()),
                ],
            )
            .await?;
        Ok(())
    }

    async fn tunnel_status(&self, tunnel_id: u64) -> Result<bool> {
        let user_id = self.user_id().await?;
        let value: Value = self
            .get_json(
                "/tunnel/status",
                vec![
                    ("user_id".into(), user_id.to_string()),
                    ("tunnel_id".into(), tunnel_id.to_string()),
                ],
            )
            .await?;
        Ok(value
            .get("data")
            .and_then(|d| d.get("online"))
            .and_then(Value::as_bool)
            .unwrap_or(false))
    }

    async fn tunnel_config(&self, tunnel_id: u64) -> Result<String> {
        let user_id = self.user_id().await?;
        let value: Value = self
            .get_json(
                "/tunnel/config",
                vec![
                    ("user_id".into(), user_id.to_string()),
                    ("tunnel_id".into(), tunnel_id.to_string()),
                    ("format".into(), "TOML".into()),
                ],
            )
            .await?;
        value
            .get("data")
            .and_then(|d| d.get("config"))
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| AppError::new(ErrorCode::Http, "LoCyanFrp 未返回 FRPC 配置"))
    }
}

async fn parse_json<T: DeserializeOwned>(response: reqwest::Response) -> Result<T> {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let message = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| format!("LoCyanFrp 返回 HTTP {}", status.as_u16()));
        let code = match status.as_u16() {
            401 => ErrorCode::AuthenticationRequired,
            403 => ErrorCode::Http,
            429 => ErrorCode::Http,
            _ => ErrorCode::Http,
        };
        return Err(AppError::new(code, message)
            .with_detail(format!("HTTP {}", status.as_u16()))
            .retryable(status.as_u16() == 429 || status.is_server_error()));
    }
    let value: Value = serde_json::from_str(&text).map_err(|e| {
        AppError::new(ErrorCode::Http, "LoCyanFrp API 返回了无法解析的响应")
            .with_detail(e.to_string())
    })?;
    let api_status = value.get("status").and_then(Value::as_i64).unwrap_or(200);
    if api_status != 200 {
        let message = value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("LoCyanFrp API 返回错误");
        let code = match api_status {
            401 => ErrorCode::AuthenticationRequired,
            403 => ErrorCode::Http,
            _ => ErrorCode::Http,
        };
        return Err(AppError::new(code, message.to_string())
            .with_detail(format!("status={api_status}"))
            .retryable(api_status == 429 || api_status >= 500));
    }
    serde_json::from_value(value).map_err(|e| {
        AppError::new(ErrorCode::Http, "LoCyanFrp API 响应字段不完整").with_detail(e.to_string())
    })
}

fn parse_node(value: &Value) -> Option<ProviderNode> {
    let additional = value.get("additional").cloned().unwrap_or(Value::Null);
    Some(ProviderNode {
        id: value.get("id").and_then(value_u64)?,
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("未命名节点")
            .to_string(),
        host: value
            .get("host")
            .and_then(Value::as_str)
            .or_else(|| value.get("ip").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string(),
        ip: value.get("ip").and_then(Value::as_str).map(str::to_string),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string),
        server_port: value
            .get("server_port")
            .and_then(value_u64)
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(7000),
        port_range: value
            .get("port_range")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        allow_udp: additional
            .get("allow_udp")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        allow_http: additional
            .get("allow_http")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        allow_big_traffic: additional
            .get("allow_big_traffic")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        need_icp: additional
            .get("need_icp")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        verification_level: value
            .get("verification_level")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn parse_tunnel(value: &Value) -> Option<ProviderTunnel> {
    let node = value.get("node");
    Some(ProviderTunnel {
        id: value.get("id").and_then(value_u64)?,
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        tunnel_type: value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        local_port: value
            .get("local_port")
            .and_then(value_u64)
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(0),
        remote_port: value
            .get("remote_port")
            .and_then(value_u64)
            .and_then(|v| u16::try_from(v).ok()),
        node_id: node.and_then(|n| n.get("id")).and_then(value_u64),
        node_name: node
            .and_then(|n| n.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        node_host: node
            .and_then(|n| n.get("host"))
            .and_then(Value::as_str)
            .map(str::to_string),
        node_ip: node
            .and_then(|n| n.get("ip"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn value_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
        .or_else(|| value.as_f64().map(|v| v as u64))
}

fn value_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_node_payload() {
        let node = parse_node(&serde_json::json!({
            "id": 7, "name": "香港 01", "host": "hk.example", "ip": "1.2.3.4",
            "description": "test", "server_port": 7000, "port_range": ["20000-30000"],
            "additional": {"allow_udp": true, "allow_http": true, "allow_big_traffic": true, "need_icp": false}
        }))
        .unwrap();
        assert_eq!(node.id, 7);
        assert!(node.allow_udp);
        assert!(node.allow_big_traffic);
        assert_eq!(node.server_port, 7000);
    }
}
