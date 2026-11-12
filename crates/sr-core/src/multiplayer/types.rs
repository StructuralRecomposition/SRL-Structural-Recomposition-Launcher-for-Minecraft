use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const PROVIDER_ID: &str = "frp52";
pub const PROVIDER_NAME: &str = "52FRP";
pub const API_BASE: &str = "https://www.52frp.com/api/v1";
pub const TOKEN_HELP_URL: &str = "https://www.52frp.com/user/#/user-center";
pub const DEFAULT_TUNNEL_PREFIX: &str = "SRL_";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Java,
    Bedrock,
}

impl Edition {
    pub fn tunnel_type(self) -> &'static str {
        match self {
            Edition::Java => "tcp",
            Edition::Bedrock => "udp",
        }
    }
    pub fn default_port(self) -> u16 {
        match self {
            Edition::Java => 25565,
            Edition::Bedrock => 19132,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Edition::Java => "Java Edition",
            Edition::Bedrock => "Bedrock Edition",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub provider: String,
    pub user_id: u64,
    pub display_name: String,
    pub email: Option<String>,
    pub group: Option<String>,
    pub verified: Option<bool>,
    pub tunnel_count: Option<u32>,
    pub tunnel_limit: Option<u32>,
    pub balance: Option<f64>,
    pub traffic_remaining: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalServiceView {
    pub found: bool,
    pub pid: Option<u32>,
    pub edition: Edition,
    pub ports: Vec<u16>,
    pub detected_port: Option<u16>,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeView {
    pub id: u64,
    pub name: String,
    pub host: String,
    pub region: Option<String>,
    pub description: Option<String>,
    pub allow_udp: bool,
    pub allow_big_traffic: bool,
    pub need_icp: bool,
    pub latency_ms: Option<u64>,
    pub clients: Option<u32>,
    pub load: Option<f64>,
    /// True only when the provider actually returned load for this node.
    pub load_available: bool,
    pub score: f64,
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrpcView {
    pub running: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub sha256: Option<String>,
    pub log_tail: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomView {
    pub tunnel_id: u64,
    pub tunnel_name: String,
    pub edition: Edition,
    pub instance_id: String,
    pub instance_name: String,
    pub version_id: String,
    pub node_id: u64,
    pub node_name: String,
    pub public_host: String,
    pub public_port: u16,
    pub local_ip: String,
    pub local_port: u16,
    pub local_pid: Option<u32>,
    pub latency_ms: Option<u64>,
    pub created_at: String,
    pub state: String,
    pub frpc_running: bool,
    pub frpc_pid: Option<u32>,
    pub local_service_ok: Option<bool>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanTunnel {
    pub tunnel_id: u64,
    pub name: String,
    pub tunnel_type: String,
    pub local_port: u16,
    pub node_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelStateView {
    pub online: bool,
    pub enabled: Option<bool>,
    pub connections: Option<u64>,
    pub status: Option<u64>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomRole {
    Owner,
    Admin,
    Member,
    Guest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomCapabilities {
    pub role: RoomRole,
    pub can_manage_room: bool,
    pub can_view_member_ips: bool,
    pub can_kick: bool,
    pub can_ban: bool,
    pub can_whitelist: bool,
    pub can_op: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerMember {
    pub name: String,
    pub uuid: Option<String>,
    pub role: RoomRole,
    pub minecraft_op: Option<bool>,
    pub joined_at: Option<String>,
    pub left_at: Option<String>,
    pub status: String,
    pub observed_ip: Option<String>,
    pub ip_kind: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MembersView {
    pub available: bool,
    pub source: String,
    pub message: String,
    pub members: Vec<PlayerMember>,
    pub partial: bool,
    pub ip_note: String,
    pub game_control_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoomLogEntry {
    pub timestamp: String,
    pub source: String,
    pub level: String,
    pub event_type: String,
    pub message: String,
    #[serde(default)]
    pub tunnel_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiplayerStatus {
    pub provider: String,
    pub provider_name: String,
    pub token_configured: bool,
    pub logged_in: bool,
    pub account: Option<AccountView>,
    pub room: Option<RoomView>,
    pub tunnel: Option<TunnelStateView>,
    pub frpc: FrpcView,
    pub capabilities: RoomCapabilities,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoomRequest {
    pub instance_id: String,
    pub edition: Edition,
    #[serde(default)]
    pub local_ip: Option<String>,
    #[serde(default)]
    pub local_port: Option<u16>,
    #[serde(default)]
    pub node_id: Option<u64>,
    #[serde(default = "default_true")]
    pub auto_node: bool,
    #[serde(default)]
    pub name: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopRoomResult {
    pub local_stopped: bool,
    pub tunnel_deleted: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub step: String,
    pub label: String,
    pub status: String,
    pub detail: Option<String>,
}

pub type ProgressSink = Arc<dyn Fn(ProgressEvent) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredRoom {
    pub tunnel_id: u64,
    pub tunnel_name: String,
    pub edition: Edition,
    pub instance_id: String,
    pub instance_name: String,
    pub version_id: String,
    pub node_id: u64,
    pub node_name: String,
    pub public_host: String,
    pub public_port: u16,
    pub local_ip: String,
    pub local_port: u16,
    #[serde(default)]
    pub local_pid: Option<u32>,
    /// Unix seconds when the local game process started. Used to reject PID
    /// reuse; absent for rooms created before v0.2.4.
    #[serde(default)]
    pub local_started_at: Option<u64>,
    pub latency_ms: Option<u64>,
    pub created_at: String,
    #[serde(default)]
    pub retained: bool,
    /// running | paused | reconnecting | cleanup_required | unknown
    #[serde(default)]
    pub lifecycle_state: String,
}
