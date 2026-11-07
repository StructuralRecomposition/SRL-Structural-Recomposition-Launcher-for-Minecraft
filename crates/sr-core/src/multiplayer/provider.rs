use crate::error::Result;
use async_trait::async_trait;
use std::collections::HashMap;

use super::types::{AccountView, Edition};

#[derive(Debug, Clone, Default)]
pub struct ProviderNode {
    pub id: u64,
    pub name: String,
    pub host: String,
    pub ip: Option<String>,
    pub city: Option<String>,
    pub province: Option<String>,
    pub description: Option<String>,
    pub available: bool,
    pub is_online: bool,
    pub proxy_count: u32,
    pub max_proxies: i64,
    pub tunnel_protocols: Vec<String>,
    pub proxies_allowed: String,
    pub proxies_denied: String,
    pub tcp_port: u16,
    pub load: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct TunnelCreate {
    pub node_id: u64,
    pub proxy_name: String,
    pub proxy_type: String,
    pub local_ip: String,
    pub local_port: u16,
    pub remote_port: u16,
}

#[derive(Debug, Clone)]
pub struct ProviderTunnel {
    pub id: u64,
    pub name: String,
    pub tunnel_type: String,
    pub local_port: u16,
    pub remote_port: Option<u16>,
    pub node_id: Option<u64>,
    pub node_name: Option<String>,
    pub node_hostname: Option<String>,
    pub is_online: bool,
}

#[async_trait]
pub trait TunnelProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;

    async fn account(&self) -> Result<AccountView>;
    async fn account_with_token(&self, token: &str) -> Result<AccountView>;
    async fn nodes(&self) -> Result<Vec<ProviderNode>>;
    async fn node_load(&self) -> Result<HashMap<u64, f64>>;
    async fn check_name(&self, name: &str) -> Result<bool>;
    async fn check_port(&self, node_id: u64, proxy_type: &str, port: u16) -> Result<bool>;
    async fn occupied_ports(&self, node_id: u64, proxy_type: &str) -> Result<Vec<u16>>;
    async fn list_tunnels(&self) -> Result<Vec<ProviderTunnel>>;
    async fn create_tunnel(&self, request: &TunnelCreate) -> Result<u64>;
    async fn delete_tunnel(&self, tunnel_id: u64) -> Result<()>;
    async fn tunnel_online(&self, tunnel_id: u64) -> Result<bool>;
    async fn tunnel_config(&self, node_id: u64, tunnel_id: u64) -> Result<String>;
}

pub fn supports_edition(node: &ProviderNode, edition: Edition) -> bool {
    node.tunnel_protocols
        .iter()
        .any(|p| p.eq_ignore_ascii_case(edition.tunnel_type()))
}
