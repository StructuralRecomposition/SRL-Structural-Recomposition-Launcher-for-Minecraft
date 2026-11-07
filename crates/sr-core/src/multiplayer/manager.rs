use crate::error::{AppError, ErrorCode, Result};
use crate::instance::Instance;
use crate::paths::AppPaths;
use futures_util::future::join_all;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use super::auth::LoCyanAuth;
use super::frpc::FrpcManager;
use super::locyan::LoCyanFrpProvider;
use super::minecraft;
use super::nodes::{port_allowed, rank_nodes, tcp_latency};
use super::provider::{ProviderNode, TunnelCreate, TunnelProvider};
use super::types::{
    AccountView, CreateRoomRequest, Edition, FrpcView, LocalServiceView, MultiplayerConfig,
    MultiplayerStatus, NodeView, OrphanTunnel, ProgressEvent, ProgressSink, RoomView,
    StopRoomResult, StoredRoom, DEFAULT_TUNNEL_PREFIX, PROVIDER_ID, PROVIDER_NAME,
};

pub struct MultiplayerManager {
    paths: AppPaths,
    http: reqwest::Client,
    frpc: FrpcManager,
    room: RwLock<Option<StoredRoom>>,
    creating: Mutex<()>,
    progress: ProgressSink,
    last_error: RwLock<Option<String>>,
}

impl MultiplayerManager {
    pub async fn new(paths: AppPaths, http: reqwest::Client, progress: ProgressSink) -> Self {
        let room = load_room(&paths).await;
        let frpc = FrpcManager::new(paths.frpc_dir(), http.clone());
        Self {
            paths,
            http,
            frpc,
            room: RwLock::new(room),
            creating: Mutex::new(()),
            progress,
            last_error: RwLock::new(None),
        }
    }

    fn auth(&self, config: &MultiplayerConfig) -> Arc<LoCyanAuth> {
        Arc::new(LoCyanAuth::new(
            self.http.clone(),
            config.clone(),
            LoCyanAuth::keyring(),
        ))
    }

    fn provider(&self, config: &MultiplayerConfig) -> LoCyanFrpProvider {
        LoCyanFrpProvider::new(self.http.clone(), self.auth(config))
    }

    fn emit(&self, step: &str, label: &str, status: &str, detail: Option<String>) {
        (self.progress)(ProgressEvent {
            step: step.into(),
            label: label.into(),
            status: status.into(),
            detail,
        });
    }

    pub async fn status(&self, config: &MultiplayerConfig) -> MultiplayerStatus {
        let auth = self.auth(config);
        let mut logged_in = config.configured() && auth.has_session();
        let mut account = None;
        let mut last_error = self.last_error.read().await.clone();
        if logged_in && config.configured() {
            let provider = self.provider(config);
            match provider.account().await {
                Ok(value) => account = Some(value),
                Err(error) => {
                    if error.code == ErrorCode::AuthenticationRequired {
                        logged_in = false;
                    }
                    last_error = Some(error.message);
                }
            }
        }
        let frpc = self.frpc.status().await;
        let frpc_pid = self.frpc.pid().await;
        let stored_room = self.room.read().await.clone();
        let local_alive = match stored_room.as_ref().and_then(|r| r.local_pid) {
            Some(pid) => process_alive(pid).await,
            None => true,
        };
        let room = stored_room.map(|stored| {
            let state = if stored.retained {
                "retained"
            } else if stored.local_pid.is_some() && !local_alive {
                "local_stopped"
            } else if frpc.running {
                "online"
            } else {
                "offline"
            };
            RoomView {
                tunnel_id: stored.tunnel_id,
                tunnel_name: stored.tunnel_name,
                edition: stored.edition,
                instance_id: stored.instance_id,
                instance_name: stored.instance_name,
                version_id: stored.version_id,
                node_id: stored.node_id,
                node_name: stored.node_name,
                public_host: stored.public_host,
                public_port: stored.public_port,
                local_ip: stored.local_ip,
                local_port: stored.local_port,
                local_pid: stored.local_pid,
                latency_ms: stored.latency_ms,
                created_at: stored.created_at,
                state: state.into(),
                frpc_running: frpc.running,
                frpc_pid,
                message: if stored.retained {
                    Some("FRPC 已停止，云端隧道仍然保留。可以删除临时隧道。".into())
                } else if stored.local_pid.is_some() && !local_alive {
                    Some(
                        "本地 Minecraft 进程已经退出；公网隧道仍在运行。可以停止并删除临时隧道。"
                            .into(),
                    )
                } else if !frpc.running {
                    Some("FRPC 未运行。可以重启 FRPC 或停止并删除隧道。".into())
                } else {
                    None
                },
            }
        });
        MultiplayerStatus {
            provider: PROVIDER_ID.into(),
            provider_name: PROVIDER_NAME.into(),
            client_configured: config.configured(),
            logged_in,
            account,
            room,
            frpc,
            last_error,
        }
    }

    pub async fn login(&self, config: &MultiplayerConfig) -> Result<AccountView> {
        let auth = self.auth(config);
        let account = auth.login().await?;
        *self.last_error.write().await = None;
        Ok(account)
    }

    pub async fn logout(&self, config: &MultiplayerConfig) -> Result<()> {
        if self.room.read().await.is_some() {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                "请先停止并清理当前联机房间，再退出 LoCyanFrp 账户",
            ));
        }
        let auth = self.auth(config);
        auth.logout().await
    }

    pub async fn nodes(
        &self,
        config: &MultiplayerConfig,
        edition: Edition,
    ) -> Result<Vec<NodeView>> {
        let provider = self.provider(config);
        let nodes = provider.nodes().await?;
        let candidates: Vec<&ProviderNode> = nodes
            .iter()
            .filter(|node| match edition {
                Edition::Java => true,
                Edition::Bedrock => node.allow_udp,
            })
            .take(40)
            .collect();
        let ids: Vec<u64> = candidates.iter().map(|n| n.id).collect();
        let stats = provider.node_stats(&ids).await.unwrap_or_default();
        let latencies =
            join_all(candidates.iter().map(|node| async move {
                (node.id, tcp_latency(&node.host, node.server_port).await)
            }))
            .await;
        let ranked = rank_nodes(&nodes, &stats, &latencies, edition);
        *self.last_error.write().await = None;
        Ok(ranked)
    }

    pub async fn detect_local(
        &self,
        instance: &Instance,
        pid: Option<u32>,
        edition: Edition,
    ) -> LocalServiceView {
        minecraft::detect_local(instance, pid, edition, &self.paths.logs_dir()).await
    }

    pub async fn orphans(&self, config: &MultiplayerConfig) -> Result<Vec<OrphanTunnel>> {
        let provider = self.provider(config);
        let tunnels = provider.list_tunnels().await?;
        Ok(tunnels
            .into_iter()
            .filter(|t| t.name.starts_with(DEFAULT_TUNNEL_PREFIX))
            .map(|t| OrphanTunnel {
                tunnel_id: t.id,
                name: t.name,
                tunnel_type: t.tunnel_type,
                local_port: t.local_port,
                node_name: t.node_name,
            })
            .collect())
    }

    pub async fn delete_orphan(&self, config: &MultiplayerConfig, tunnel_id: u64) -> Result<()> {
        let provider = self.provider(config);
        let tunnels = provider.list_tunnels().await?;
        let tunnel = tunnels
            .into_iter()
            .find(|t| t.id == tunnel_id)
            .ok_or_else(|| AppError::not_found("隧道不存在或不属于当前账户"))?;
        if !tunnel.name.starts_with(DEFAULT_TUNNEL_PREFIX) {
            return Err(AppError::invalid_input(
                "只允许删除由 SRL 创建的临时联机隧道",
            ));
        }
        provider.delete_tunnel(tunnel_id).await
    }

    pub async fn create_room(
        &self,
        config: &MultiplayerConfig,
        request: &CreateRoomRequest,
        instance: &Instance,
        pid: Option<u32>,
    ) -> Result<RoomView> {
        let _guard = self.creating.try_lock().map_err(|_| {
            AppError::new(ErrorCode::AlreadyExists, "正在创建联机房间，请勿重复点击")
        })?;
        if self.room.read().await.is_some() {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                "已有联机房间记录，请先停止并清理后再创建",
            ));
        }
        *self.last_error.write().await = None;

        let provider = self.provider(config);
        let mut created_tunnel = None;
        let result = self
            .create_inner(
                config,
                request,
                instance,
                pid,
                &provider,
                &mut created_tunnel,
            )
            .await;

        match result {
            Ok((stored, latency)) => {
                if let Err(error) = save_room(&self.paths, &stored).await {
                    let _ = self.frpc.stop().await;
                    let _ = provider.delete_tunnel(stored.tunnel_id).await;
                    *self.last_error.write().await = Some(error.message.clone());
                    return Err(error);
                }
                *self.room.write().await = Some(stored.clone());
                self.emit("done", "联机房间创建成功", "ok", None);
                let frpc = self.frpc.status().await;
                Ok(RoomView {
                    tunnel_id: stored.tunnel_id,
                    tunnel_name: stored.tunnel_name,
                    edition: stored.edition,
                    instance_id: stored.instance_id,
                    instance_name: stored.instance_name,
                    version_id: stored.version_id,
                    node_id: stored.node_id,
                    node_name: stored.node_name,
                    public_host: stored.public_host,
                    public_port: stored.public_port,
                    local_ip: stored.local_ip,
                    local_port: stored.local_port,
                    local_pid: stored.local_pid,
                    latency_ms: latency,
                    created_at: stored.created_at,
                    state: "online".into(),
                    frpc_running: frpc.running,
                    frpc_pid: self.frpc.pid().await,
                    message: None,
                })
            }
            Err(error) => {
                let _ = self.frpc.stop().await;
                if let Some(tunnel_id) = created_tunnel {
                    let _ = provider.delete_tunnel(tunnel_id).await;
                }
                *self.last_error.write().await = Some(error.message.clone());
                self.emit("error", "创建失败", "error", Some(error.message.clone()));
                Err(error)
            }
        }
    }

    async fn create_inner(
        &self,
        config: &MultiplayerConfig,
        request: &CreateRoomRequest,
        instance: &Instance,
        pid: Option<u32>,
        provider: &LoCyanFrpProvider,
        created_tunnel: &mut Option<u64>,
    ) -> Result<(StoredRoom, Option<u64>)> {
        if !config.configured() {
            return Err(AppError::not_configured(
                "尚未配置 LoCyanFrp 应用 ID，请先在设置中完成 OAuth 应用配置",
            ));
        }

        self.emit("auth", "检查 LoCyanFrp 账户", "running", None);
        let account = provider.account().await?;
        self.emit("auth", "检查 LoCyanFrp 账户", "ok", None);

        self.emit("local", "检查本地 Minecraft 服务", "running", None);
        let local = self.detect_local(instance, pid, request.edition).await;
        let local_ip = request
            .local_ip
            .as_deref()
            .unwrap_or("127.0.0.1")
            .trim()
            .to_string();
        local_ip
            .parse::<IpAddr>()
            .map_err(|_| AppError::invalid_input("本地 IP 地址格式无效"))?;
        let local_port = request.local_port.or(local.detected_port).ok_or_else(|| {
            AppError::new(
                ErrorCode::LanPortNotFound,
                "没有检测到本地游戏端口，请先在游戏内开放局域网或手动填写端口",
            )
        })?;
        if local_port == 0 {
            return Err(AppError::invalid_input("本地端口必须大于 0"));
        }
        let local_detail = local.message.clone();
        if request.edition == Edition::Java {
            if !minecraft::tcp_reachable(&local_ip, local_port).await {
                return Err(AppError::new(
                    ErrorCode::LanPortNotFound,
                    format!("本地 TCP 端口 {local_ip}:{local_port} 未在监听"),
                ));
            }
        } else if !minecraft::udp_port_in_use(local_port) {
            return Err(AppError::new(
                ErrorCode::LanPortNotFound,
                format!("本地 UDP 端口 {local_port} 未在监听"),
            ));
        }
        self.emit("local", "检查本地 Minecraft 服务", "ok", Some(local_detail));

        self.emit("nodes", "获取可用节点", "running", None);
        let nodes = provider.nodes().await?;
        if nodes.is_empty() {
            return Err(AppError::not_found("当前账户没有可用节点"));
        }
        self.emit(
            "nodes",
            "获取可用节点",
            "ok",
            Some(format!("{} 个节点", nodes.len())),
        );

        self.emit("node", "选择合适节点", "running", None);
        let ids: Vec<u64> = nodes.iter().take(40).map(|n| n.id).collect();
        let stats = provider.node_stats(&ids).await.unwrap_or_default();
        let latencies =
            join_all(nodes.iter().take(40).map(|node| async move {
                (node.id, tcp_latency(&node.host, node.server_port).await)
            }))
            .await;
        let ranked = rank_nodes(&nodes, &stats, &latencies, request.edition);
        let selected: NodeView = if request.auto_node || request.node_id.is_none() {
            ranked
                .iter()
                .find(|node| node.available)
                .cloned()
                .ok_or_else(|| AppError::not_found("没有支持当前联机类型的可用节点"))?
        } else {
            ranked
                .iter()
                .find(|node| node.id == request.node_id.unwrap())
                .cloned()
                .ok_or_else(|| AppError::not_found("选择的节点不存在"))?
        };
        if !selected.available {
            return Err(AppError::invalid_input(
                selected
                    .reason
                    .clone()
                    .unwrap_or_else(|| "选择的节点不可用".into()),
            ));
        }
        self.emit(
            "node",
            "选择合适节点",
            "ok",
            Some(match selected.latency_ms {
                Some(ms) => format!("{}（{ms} ms）", selected.name),
                None => format!("{}（未测速）", selected.name),
            }),
        );

        self.emit("quota", "检查隧道额度", "running", None);
        let tunnels = provider.list_tunnels().await?;
        if tunnels
            .iter()
            .any(|t| t.name.starts_with(DEFAULT_TUNNEL_PREFIX))
        {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                "发现上次创建的 SRL 临时隧道，请先在联机中心清理后再创建",
            ));
        }
        if let (Some(limit), Some(count)) = (account.tunnel_limit, account.tunnel_count) {
            if count >= limit {
                return Err(AppError::new(
                    ErrorCode::Http,
                    format!("LoCyanFrp 隧道额度已用完（{count}/{limit}）"),
                ));
            }
        }
        self.emit("quota", "检查隧道额度", "ok", None);

        self.emit("port", "获取远程端口", "running", None);
        let remote_port = provider.random_port(selected.id).await?;
        let node = nodes.iter().find(|n| n.id == selected.id).unwrap();
        if !port_allowed(&node.port_range, remote_port) {
            return Err(AppError::new(
                ErrorCode::Http,
                "LoCyanFrp 返回的远程端口不在节点允许范围内",
            ));
        }
        self.emit("port", "获取远程端口", "ok", Some(remote_port.to_string()));

        self.emit("tunnel", "创建隧道", "running", None);
        let name = request.name.clone().unwrap_or_else(|| {
            format!(
                "{DEFAULT_TUNNEL_PREFIX}{}-{}",
                request.edition.tunnel_type(),
                &uuid::Uuid::new_v4().to_string()[..8]
            )
        });
        let tunnel_id = provider
            .create_tunnel(&TunnelCreate {
                node_id: selected.id,
                name: name.clone(),
                tunnel_type: request.edition.tunnel_type().into(),
                local_ip: local_ip.clone(),
                local_port,
                remote_port,
                use_encryption: true,
                use_compression: true,
            })
            .await?;
        *created_tunnel = Some(tunnel_id);
        self.emit("tunnel", "创建隧道", "ok", Some(format!("#{tunnel_id}")));

        self.emit("config", "获取隧道配置", "running", None);
        let config_text = provider.tunnel_config(tunnel_id).await?;
        self.emit("config", "获取隧道配置", "ok", None);

        self.emit("frpc", "下载并启动 FRPC", "running", None);
        let prepared = self
            .frpc
            .prepare(std::env::consts::OS, std::env::consts::ARCH)
            .await?;
        self.frpc.set_prepared(&prepared).await;
        let frpc_pid = self.frpc.start(&prepared.path, &config_text).await?;
        self.emit(
            "frpc",
            "下载并启动 FRPC",
            "ok",
            Some(format!("PID {frpc_pid}")),
        );

        self.emit("verify", "验证公网连接", "running", None);
        let mut online = false;
        for _ in 0..23 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            match provider.tunnel_status(tunnel_id).await {
                Ok(true) => {
                    online = true;
                    break;
                }
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(%error, "tunnel status check failed");
                }
            }
        }
        if !online {
            return Err(AppError::new(
                ErrorCode::ConnectFailed,
                "FRPC 已启动，但 LoCyanFrp 在 45 秒内没有报告隧道在线",
            ));
        }
        self.emit("verify", "验证公网连接", "ok", None);

        let stored = StoredRoom {
            tunnel_id,
            tunnel_name: name,
            edition: request.edition,
            instance_id: instance.id.clone(),
            instance_name: instance.name.clone(),
            version_id: instance.version_id.clone(),
            node_id: selected.id,
            node_name: selected.name.clone(),
            public_host: selected.host.clone(),
            public_port: remote_port,
            local_ip,
            local_port,
            local_pid: pid,
            latency_ms: selected.latency_ms,
            created_at: chrono::Utc::now().to_rfc3339(),
            retained: false,
        };
        Ok((stored, selected.latency_ms))
    }

    pub async fn stop_room(
        &self,
        config: &MultiplayerConfig,
        delete_tunnel: bool,
    ) -> Result<StopRoomResult> {
        let room = self.room.read().await.clone();
        let Some(room) = room else {
            return Ok(StopRoomResult {
                local_stopped: true,
                tunnel_deleted: false,
                message: "当前没有联机房间".into(),
            });
        };
        self.frpc.stop().await?;
        if !delete_tunnel {
            let mut retained = room;
            retained.retained = true;
            save_room(&self.paths, &retained).await?;
            *self.room.write().await = Some(retained);
            return Ok(StopRoomResult {
                local_stopped: true,
                tunnel_deleted: false,
                message: "FRPC 已停止；云端隧道保留，可稍后删除".into(),
            });
        }

        let provider = self.provider(config);
        let tunnels = provider.list_tunnels().await?;
        let found = tunnels.into_iter().find(|t| t.id == room.tunnel_id);
        match found {
            Some(t) if !t.name.starts_with(DEFAULT_TUNNEL_PREFIX) => {
                return Err(AppError::invalid_input(
                    "拒绝删除：目标隧道不是 SRL 创建的临时隧道",
                ));
            }
            None => {
                // The cloud tunnel is already gone. Clear the stale local room.
                let _ = std::fs::remove_file(self.paths.multiplayer_room_file());
                *self.room.write().await = None;
                return Ok(StopRoomResult {
                    local_stopped: true,
                    tunnel_deleted: true,
                    message: "FRPC 已停止；云端隧道已不存在，已清理本地记录".into(),
                });
            }
            Some(_) => {}
        }
        match provider.delete_tunnel(room.tunnel_id).await {
            Ok(()) => {
                let _ = std::fs::remove_file(self.paths.multiplayer_room_file());
                *self.room.write().await = None;
                *self.last_error.write().await = None;
                Ok(StopRoomResult {
                    local_stopped: true,
                    tunnel_deleted: true,
                    message: "FRPC 已停止，临时隧道已删除".into(),
                })
            }
            Err(error) => {
                let mut retained = room;
                retained.retained = true;
                save_room(&self.paths, &retained).await?;
                *self.room.write().await = Some(retained);
                *self.last_error.write().await = Some(error.message.clone());
                Ok(StopRoomResult {
                    local_stopped: true,
                    tunnel_deleted: false,
                    message: format!("FRPC 已停止，但删除云端隧道失败：{}", error.message),
                })
            }
        }
    }

    pub async fn restart_frpc(&self, config: &MultiplayerConfig) -> Result<FrpcView> {
        let room = self
            .room
            .read()
            .await
            .clone()
            .ok_or_else(|| AppError::not_found("当前没有联机房间"))?;
        if room.retained {
            return Err(AppError::not_found(
                "该房间的云端隧道已停止或已删除，请重新创建",
            ));
        }
        let provider = self.provider(config);
        let config_text = provider.tunnel_config(room.tunnel_id).await?;
        let prepared = self
            .frpc
            .prepare(std::env::consts::OS, std::env::consts::ARCH)
            .await?;
        self.frpc.set_prepared(&prepared).await;
        self.frpc.start(&prepared.path, &config_text).await?;
        Ok(self.frpc.status().await)
    }

    pub async fn shutdown(&self) {
        let was_running = self.frpc.is_running().await;
        self.frpc.shutdown().await;
        if was_running {
            let mut room = self.room.write().await;
            if let Some(room) = room.as_mut() {
                room.retained = true;
                let _ = save_room(&self.paths, room).await;
            }
        }
    }

    /// Expose the stored room for diagnostics/tests without leaking config.
    pub async fn stored_room(&self) -> Option<StoredRoom> {
        self.room.read().await.clone()
    }
}

async fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(windows)]
    {
        let output = tokio::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .await;
        return output
            .ok()
            .is_some_and(|out| String::from_utf8_lossy(&out.stdout).contains(&pid.to_string()));
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        true
    }
}

async fn load_room(paths: &AppPaths) -> Option<StoredRoom> {
    let text = tokio::fs::read_to_string(paths.multiplayer_room_file())
        .await
        .ok()?;
    serde_json::from_str(&text).ok()
}

async fn save_room(paths: &AppPaths, room: &StoredRoom) -> Result<()> {
    crate::util::ensure_dir(&paths.multiplayer_dir()).await?;
    let json = serde_json::to_string_pretty(room)?;
    crate::util::write_private_file(&paths.multiplayer_room_file(), json.as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn loads_and_saves_room() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        let room = StoredRoom {
            tunnel_id: 7,
            tunnel_name: "SRL-MP-TCP-test".into(),
            edition: Edition::Java,
            instance_id: "abc".into(),
            instance_name: "Test".into(),
            version_id: "1.21.1".into(),
            node_id: 1,
            node_name: "node".into(),
            public_host: "example.locyanfrp.cn".into(),
            public_port: 25565,
            local_ip: "127.0.0.1".into(),
            local_port: 25565,
            local_pid: Some(123),
            latency_ms: Some(30),
            created_at: "2026-11-07T00:00:00Z".into(),
            retained: false,
        };
        save_room(&paths, &room).await.unwrap();
        let loaded = load_room(&paths).await.unwrap();
        assert_eq!(loaded.tunnel_id, 7);
        assert_eq!(loaded.edition, Edition::Java);
    }
}
