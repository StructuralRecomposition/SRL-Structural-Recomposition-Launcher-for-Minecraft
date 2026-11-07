use crate::error::Result;
use async_trait::async_trait;
use serde::Deserialize;

use super::types::{AccountView, Edition};

#[derive(Debug, Clone)]
pub struct ProviderNode {
    pub id: u64,
    pub name: String,
    pub host: String,
    pub ip: Option<String>,
    pub description: Option<String>,
    pub server_port: u16,
    pub port_range: Vec<String>,
    pub allow_udp: bool,
    pub allow_http: bool,
    pub allow_big_traffic: bool,
    pub need_icp: bool,
    pub verification_level: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NodeStat {
    pub node_id: u64,
    pub inbound: Option<f64>,
    pub outbound: Option<f64>,
    pub clients: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct TunnelCreate {
    pub node_id: u64,
    pub name: String,
    pub tunnel_type: String,
    pub local_ip: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub use_encryption: bool,
    pub use_compression: bool,
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
    pub node_host: Option<String>,
    pub node_ip: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TunnelStatus {
    pub online: bool,
}

#[async_trait]
pub trait TunnelProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;

    async fn account(&self) -> Result<AccountView>;
    async fn nodes(&self) -> Result<Vec<ProviderNode>>;
    async fn node_stats(&self, node_ids: &[u64]) -> Result<Vec<NodeStat>>;
    async fn random_port(&self, node_id: u64) -> Result<u16>;
    async fn list_tunnels(&self) -> Result<Vec<ProviderTunnel>>;
    async fn create_tunnel(&self, request: &TunnelCreate) -> Result<u64>;
    async fn delete_tunnel(&self, tunnel_id: u64) -> Result<()>;
    async fn tunnel_status(&self, tunnel_id: u64) -> Result<bool>;
    async fn tunnel_config(&self, tunnel_id: u64) -> Result<String>;
}

pub fn supports_edition(node: &ProviderNode, edition: Edition) -> bool {
    match edition {
        Edition::Java => true,
        Edition::Bedrock => node.allow_udp,
    }
}
