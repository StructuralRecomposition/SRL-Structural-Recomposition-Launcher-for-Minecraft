use crate::error::{AppError, ErrorCode, Result};
use crate::instance::Instance;
use crate::paths::AppPaths;
use chrono::{DateTime, Utc};
use futures_util::future::join_all;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

/// Bounded insertion-ordered set used for player-event de-duplication.
/// Old keys are evicted once the cap is reached so new events keep flowing.
#[derive(Default)]
struct SeenEvents {
    order: std::collections::VecDeque<String>,
    set: std::collections::HashSet<String>,
}

impl SeenEvents {
    const LIMIT: usize = 10_000;

    fn insert(&mut self, key: String) -> bool {
        if !self.set.insert(key.clone()) {
            return false;
        }
        self.order.push_back(key);
        while self.order.len() > Self::LIMIT {
            if let Some(old) = self.order.pop_front() {
                self.set.remove(&old);
            }
        }
        true
    }

    fn clear(&mut self) {
        self.order.clear();
        self.set.clear();
    }
}

use super::frp52::{parse_port_ranges, port_in_ranges, Frp52Provider};
use super::frpc::{DownloadProgress, FrpcManager};
use super::journal;
use super::minecraft;
use super::nodes::{rank_nodes, tcp_latency};
use super::provider::{supports_edition, ProviderNode, TunnelCreate, TunnelProvider};
use super::token::Frp52TokenStore;
use super::types::{
    AccountView, CreateRoomRequest, Edition, FrpcView, LocalServiceView, MembersView,
    MultiplayerStatus, NodeView, OrphanTunnel, PlayerMember, ProgressEvent, ProgressSink,
    RoomCapabilities, RoomView, StopRoomResult, StoredRoom, TunnelStateView, DEFAULT_TUNNEL_PREFIX,
    PROVIDER_ID, PROVIDER_NAME,
};

pub struct MultiplayerManager {
    paths: AppPaths,
    http: reqwest::Client,
    tokens: Arc<Frp52TokenStore>,
    frpc: FrpcManager,
    room: RwLock<Option<StoredRoom>>,
    creating: Mutex<()>,
    progress: ProgressSink,
    last_error: RwLock<Option<String>>,
    tunnel_cache: RwLock<Option<(Instant, TunnelStateView)>>,
    last_room_state: RwLock<Option<String>>,
    seen_player_events: Mutex<SeenEvents>,
}

impl MultiplayerManager {
    pub async fn new(paths: AppPaths, http: reqwest::Client, progress: ProgressSink) -> Self {
        Self::new_with_bundle(paths, http, progress, None).await
    }

    pub async fn new_with_bundle(
        paths: AppPaths,
        http: reqwest::Client,
        progress: ProgressSink,
        bundled_frpc: Option<std::path::PathBuf>,
    ) -> Self {
        let room = load_room(&paths).await;
        let frpc = FrpcManager::new_with_bundle(paths.frpc_dir(), http.clone(), bundled_frpc);
        Self {
            paths,
            http,
            tokens: Arc::new(Frp52TokenStore::keyring()),
            frpc,
            room: RwLock::new(room),
            creating: Mutex::new(()),
            progress,
            last_error: RwLock::new(None),
            tunnel_cache: RwLock::new(None),
            last_room_state: RwLock::new(None),
            seen_player_events: Mutex::new(SeenEvents::default()),
        }
    }

    fn frpc_manager(&self) -> FrpcManager {
        FrpcManager::new_with_bundle(
            self.paths.frpc_dir(),
            self.http.clone(),
            self.frpc.bundled_dir_clone(),
        )
    }

    fn provider(&self) -> Frp52Provider {
        Frp52Provider::new(self.http.clone(), self.tokens.clone())
    }

    fn emit(&self, step: &str, label: &str, status: &str, detail: Option<String>) {
        (self.progress)(ProgressEvent {
            step: step.into(),
            label: label.into(),
            status: status.into(),
            detail,
        });
    }

    pub async fn status(&self) -> MultiplayerStatus {
        let token_configured = self.tokens.get().await.ok().flatten().is_some();
        let mut logged_in = token_configured;
        let mut account = None;
        let mut last_error = self.last_error.read().await.clone();
        if token_configured {
            match self.provider().account().await {
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
        let local_alive = match stored_room
            .as_ref()
            .and_then(|room| room.local_pid.map(|pid| (pid, room.local_started_at)))
        {
            Some((pid, Some(started_at))) => process_matches(pid, started_at).await,
            Some((pid, None)) => process_alive(pid).await,
            None => true,
        };
        let local_service_ok = match stored_room.as_ref() {
            Some(stored) => Some(match stored.edition {
                Edition::Java => {
                    minecraft::java_protocol_ok(&stored.local_ip, stored.local_port).await
                }
                Edition::Bedrock => {
                    minecraft::bedrock_protocol_ok(&stored.local_ip, stored.local_port).await
                }
            }),
            None => None,
        };
        let tunnel = match stored_room.as_ref() {
            Some(stored) if logged_in => self.tunnel_state(stored.tunnel_id).await,
            _ => None,
        };
        let room = stored_room.map(|stored| {
            let tunnel_online = tunnel.as_ref().map(|value| value.online);
            let state = if stored.retained {
                "retained"
            } else if stored.local_pid.is_some() && !local_alive {
                "local_stopped"
            } else if !frpc.running {
                "offline"
            } else if tunnel_online == Some(false) {
                "tunnel_offline"
            } else if local_service_ok == Some(false) {
                "local_unreachable"
            } else if tunnel_online.is_none() {
                "unconfirmed"
            } else {
                "online"
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
                local_service_ok,
                message: if stored.retained {
                    Some("FRPC 已停止，云端隧道仍然保留。可以删除临时隧道。".into())
                } else if stored.local_pid.is_some() && !local_alive {
                    Some(
                        "本地 Minecraft 进程已经退出；公网隧道仍在运行。可以停止并删除临时隧道。"
                            .into(),
                    )
                } else if !frpc.running {
                    Some("FRPC 未运行。可以重启 FRPC 或停止并删除隧道。".into())
                } else if tunnel_online == Some(false) {
                    Some("FRPC 在运行，但云端隧道尚未在线；当前不能确认玩家可连接。".into())
                } else if local_service_ok == Some(false) {
                    Some(
                        "FRPC 在运行，但本地 Minecraft 协议检测失败；当前不能确认玩家可连接。"
                            .into(),
                    )
                } else if tunnel_online.is_none() {
                    Some("FRPC 在运行，但无法查询云端隧道状态；连接可用性未知。".into())
                } else {
                    None
                },
            }
        });
        if let Some(room) = room.as_ref() {
            let mut last = self.last_room_state.write().await;
            if last.as_deref() != Some(room.state.as_str()) {
                let previous = last.clone();
                *last = Some(room.state.clone());
                drop(last);
                if previous.is_some() {
                    let _ = journal::append(
                        &self.paths,
                        "room",
                        if room.state == "online" {
                            "info"
                        } else {
                            "warning"
                        },
                        "room.state",
                        &format!("房间状态变为 {}", room.state),
                        Some(room.tunnel_id),
                    )
                    .await;
                }
            }
        }
        MultiplayerStatus {
            provider: PROVIDER_ID.into(),
            provider_name: PROVIDER_NAME.into(),
            token_configured,
            logged_in,
            account,
            room,
            tunnel,
            frpc,
            capabilities: owner_capabilities(),
            last_error,
        }
    }

    async fn tunnel_state(&self, tunnel_id: u64) -> Option<TunnelStateView> {
        {
            let cache = self.tunnel_cache.read().await;
            if let Some((at, value)) = cache.as_ref() {
                if at.elapsed() < Duration::from_secs(20) {
                    return Some(value.clone());
                }
            }
        }
        match self.provider().tunnel_state(tunnel_id).await {
            Ok(value) => {
                *self.tunnel_cache.write().await = Some((Instant::now(), value.clone()));
                Some(value)
            }
            Err(error) => {
                tracing::debug!(%error, "52FRP tunnel state unavailable");
                None
            }
        }
    }

    /// Player events come from the real Minecraft log captured by SRL. No
    /// event is inferred from TCP connection counts.
    pub async fn members(&self) -> MembersView {
        let room = self.room.read().await.clone();
        let Some(room) = room else {
            return MembersView {
                available: false,
                source: "none".into(),
                message: "当前没有联机房间。".into(),
                members: Vec::new(),
                partial: true,
                ip_note: String::new(),
                game_control_available: false,
            };
        };
        if room.edition != Edition::Java {
            return MembersView {
                available: false,
                source: "none".into(),
                message: "基岩版没有可读取的 Java 服务端玩家日志。".into(),
                members: Vec::new(),
                partial: true,
                ip_note: String::new(),
                game_control_available: false,
            };
        }
        let since = DateTime::parse_from_rfc3339(&room.created_at)
            .ok()
            .map(|value| value.with_timezone(&Utc));
        let events =
            minecraft::read_player_events(&self.paths.logs_dir(), &room.instance_id, since);
        let Some(events) = events else {
            return MembersView {
                available: false,
                source: "minecraft-log".into(),
                message: "没有找到该实例由 SRL 捕获的游戏日志，无法读取玩家事件。".into(),
                members: Vec::new(),
                partial: true,
                ip_note: String::new(),
                game_control_available: false,
            };
        };
        {
            let mut seen = self.seen_player_events.lock().await;
            for event in &events {
                let key = format!(
                    "{}|{}|{}|{}|{}",
                    event.action,
                    event.player,
                    event.at.as_deref().unwrap_or(""),
                    event.observed_ip.as_deref().unwrap_or(""),
                    event.line
                );
                if !seen.insert(key) {
                    continue;
                }
                let _ = journal::append(
                    &self.paths,
                    "minecraft",
                    "info",
                    if event.action == "leave" {
                        "player.leave"
                    } else {
                        "player.join"
                    },
                    &format!(
                        "玩家 {} {}",
                        event.player,
                        if event.action == "leave" {
                            "离开"
                        } else {
                            "加入"
                        }
                    ),
                    Some(room.tunnel_id),
                )
                .await;
            }
        }
        let mut members: std::collections::BTreeMap<String, PlayerMember> = Default::default();
        for event in events {
            let entry = members
                .entry(event.player.clone())
                .or_insert_with(|| PlayerMember {
                    name: event.player.clone(),
                    uuid: None,
                    role: super::types::RoomRole::Guest,
                    minecraft_op: None,
                    joined_at: None,
                    left_at: None,
                    status: "unknown".into(),
                    observed_ip: None,
                    ip_kind: "unknown".into(),
                    source: event.source.clone(),
                });
            match event.action.as_str() {
                "join" => {
                    entry.joined_at = event.at.clone().or(entry.joined_at.clone());
                    entry.left_at = None;
                    entry.status = "connected".into();
                    if event.observed_ip.is_some() {
                        entry.observed_ip = event.observed_ip;
                        entry.ip_kind = event.ip_kind;
                    }
                }
                "leave" => {
                    entry.left_at = event.at.clone().or(entry.left_at.clone());
                    entry.status = "left".into();
                }
                _ => {}
            }
        }
        MembersView {
            available: true,
            source: "minecraft-log".into(),
            message: if members.is_empty() {
                "游戏日志可读取，但还没有观察到玩家加入事件。".into()
            } else {
                "来自 Minecraft 游戏日志的真实加入/离开事件。".into()
            },
            members: members.into_values().collect(),
            partial: true,
            ip_note: "服务端观察到的地址经 52FRP 转发时通常是 127.0.0.1；这表示本地代理连接，不代表游客公网 IP。".into(),
            game_control_available: false,
        }
    }

    pub async fn connect(&self, token: &str) -> Result<AccountView> {
        let provider = self.provider();
        let account = provider.account_with_token(token).await?;
        self.tokens.set(token).await?;
        *self.last_error.write().await = None;
        let frpc = self.frpc_manager();
        tokio::spawn(async move {
            frpc.warm_update().await;
        });
        Ok(account)
    }

    pub async fn logout(&self) -> Result<()> {
        if self.room.read().await.is_some() {
            return Err(AppError::new(
                ErrorCode::AlreadyExists,
                "请先停止并清理当前联机房间，再退出 52FRP 账户",
            ));
        }
        self.tokens.clear().await
    }

    pub async fn nodes(&self, edition: Edition) -> Result<Vec<NodeView>> {
        let provider = self.provider();
        let nodes = provider.nodes().await?;
        let load = provider.node_load().await.unwrap_or_default();
        let candidates: Vec<&ProviderNode> = nodes
            .iter()
            .filter(|node| {
                node.available
                    && node.is_online
                    && supports_edition(node, edition)
                    && (node.max_proxies < 0 || (node.proxy_count as i64) < node.max_proxies)
            })
            .take(40)
            .collect();
        let latencies =
            join_all(candidates.iter().map(|node| async move {
                (node.id, tcp_latency(&node.host, node.tcp_port).await)
            }))
            .await;
        let ranked = rank_nodes(&nodes, &load, &latencies, edition);
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

    pub async fn orphans(&self) -> Result<Vec<OrphanTunnel>> {
        let provider = self.provider();
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

    pub async fn delete_orphan(&self, tunnel_id: u64) -> Result<()> {
        let provider = self.provider();
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
        self.seen_player_events.lock().await.clear();
        *self.last_room_state.write().await = None;
        let _ = journal::append(
            &self.paths,
            "room",
            "info",
            "room.create",
            "开始创建联机房间",
            None,
        )
        .await;

        let provider = self.provider();
        let mut created_tunnel = None;
        let result = self
            .create_inner(request, instance, pid, &provider, &mut created_tunnel)
            .await;

        match result {
            Ok(stored) => {
                if let Err(error) = save_room(&self.paths, &stored).await {
                    let _ = self.frpc.stop().await;
                    let _ = provider.delete_tunnel(stored.tunnel_id).await;
                    *self.last_error.write().await = Some(error.message.clone());
                    return Err(error);
                }
                *self.room.write().await = Some(stored.clone());
                self.emit("done", "联机房间创建成功", "ok", None);
                let _ = journal::append(
                    &self.paths,
                    "room",
                    "info",
                    "room.created",
                    &format!(
                        "房间已创建：{}:{}（隧道 #{}，节点 {}）",
                        stored.public_host, stored.public_port, stored.tunnel_id, stored.node_name
                    ),
                    Some(stored.tunnel_id),
                )
                .await;
                *self.tunnel_cache.write().await = None;
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
                    latency_ms: stored.latency_ms,
                    created_at: stored.created_at,
                    state: "online".into(),
                    frpc_running: frpc.running,
                    frpc_pid: self.frpc.pid().await,
                    local_service_ok: Some(true),
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
                let _ = journal::append(
                    &self.paths,
                    "room",
                    "error",
                    "room.error",
                    &format!("创建联机房间失败：{}", error.message),
                    None,
                )
                .await;
                Err(error)
            }
        }
    }

    fn frpc_progress(&self) -> DownloadProgress {
        let sink = self.progress.clone();
        let started = Instant::now();
        let last = Arc::new(std::sync::Mutex::new(
            Instant::now() - Duration::from_secs(1),
        ));
        Arc::new(move |done, total| {
            let Ok(mut last_emit) = last.lock() else {
                return;
            };
            if last_emit.elapsed() < Duration::from_millis(300) {
                return;
            }
            *last_emit = Instant::now();
            let mb = done as f64 / 1024.0 / 1024.0;
            let speed = if started.elapsed().as_secs_f64() > 0.1 {
                done as f64 / started.elapsed().as_secs_f64() / 1024.0 / 1024.0
            } else {
                0.0
            };
            let detail = match total {
                Some(total) => format!(
                    "已下载 {:.1} / {:.1} MB · {:.1} MB/s",
                    mb,
                    total as f64 / 1024.0 / 1024.0,
                    speed
                ),
                None => format!("已下载 {:.1} MB · {:.1} MB/s", mb, speed),
            };
            (sink)(ProgressEvent {
                step: "frpc".into(),
                label: "下载并启动 FRPC".into(),
                status: "running".into(),
                detail: Some(detail),
            });
        })
    }

    async fn create_inner(
        &self,
        request: &CreateRoomRequest,
        instance: &Instance,
        pid: Option<u32>,
        provider: &Frp52Provider,
        created_tunnel: &mut Option<u64>,
    ) -> Result<StoredRoom> {
        self.emit("auth", "检查 52FRP 账户", "running", None);
        let account = provider.account().await?;
        if account.verified == Some(false) {
            return Err(AppError::new(
                ErrorCode::Http,
                "52FRP 账户尚未实名认证，无法创建隧道",
            ));
        }
        self.emit("auth", "检查 52FRP 账户", "ok", None);

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
        if request.edition == Edition::Java {
            if !minecraft::java_protocol_ok(&local_ip, local_port).await {
                return Err(AppError::new(
                    ErrorCode::LanPortNotFound,
                    format!(
                        "本地端口 {local_ip}:{local_port} 没有返回 Minecraft 状态；请确认已开放局域网，且该端口不是其他程序"
                    ),
                ));
            }
        } else if !minecraft::bedrock_protocol_ok(&local_ip, local_port).await {
            return Err(AppError::new(
                ErrorCode::LanPortNotFound,
                format!(
                    "本地 UDP 端口 {local_port} 没有返回 RakNet 响应；请确认基岩版世界已开放且端口正确"
                ),
            ));
        }
        self.emit(
            "local",
            "检查本地 Minecraft 服务",
            "ok",
            Some(local.message.clone()),
        );

        self.emit("nodes", "获取可用节点", "running", None);
        let nodes = provider.nodes().await?;
        let usable: Vec<&ProviderNode> = nodes
            .iter()
            .filter(|node| {
                node.available
                    && node.is_online
                    && supports_edition(node, request.edition)
                    && (node.max_proxies < 0 || (node.proxy_count as i64) < node.max_proxies)
                    && !node.host.trim().is_empty()
            })
            .collect();
        if usable.is_empty() {
            return Err(AppError::not_found(
                "52FRP 没有当前账户可用、在线且支持该协议的节点；请检查套餐覆盖、实名状态和节点状态",
            ));
        }
        self.emit(
            "nodes",
            "获取可用节点",
            "ok",
            Some(format!("{} 个可用节点", usable.len())),
        );

        self.emit("node", "选择合适节点", "running", None);
        let load = provider.node_load().await.unwrap_or_default();
        let latencies =
            join_all(usable.iter().take(40).map(|node| async move {
                (node.id, tcp_latency(&node.host, node.tcp_port).await)
            }))
            .await;
        let ranked = rank_nodes(&nodes, &load, &latencies, request.edition);
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
        let node = nodes
            .iter()
            .find(|n| n.id == selected.id)
            .ok_or_else(|| AppError::not_found("节点信息丢失"))?;
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
                    format!("52FRP 隧道额度已用完（{count}/{limit}）"),
                ));
            }
        }
        self.emit("quota", "检查隧道额度", "ok", None);

        self.emit("port", "分配远程端口", "running", None);
        let occupied = provider
            .occupied_ports(node.id, request.edition.tunnel_type())
            .await
            .map_err(|error| {
                AppError::new(
                    error.code,
                    format!("无法获取节点已占用端口：{}", error.message),
                )
            })?;
        let allowed = parse_port_ranges(&node.proxies_allowed);
        let denied = parse_port_ranges(&node.proxies_denied);
        let ranges = if allowed.is_empty() {
            vec![(10000, 65535)]
        } else {
            allowed.clone()
        };
        let mut remote_port = None;
        let salt = uuid::Uuid::new_v4().as_u128() as u64;
        let port_deadline = std::time::Instant::now() + Duration::from_secs(15);
        for attempt in 0..40u64 {
            if std::time::Instant::now() >= port_deadline {
                break;
            }
            let Some(candidate) =
                choose_port(&ranges, &denied, &occupied, salt.wrapping_add(attempt))
            else {
                continue;
            };
            match provider
                .check_port(node.id, request.edition.tunnel_type(), candidate)
                .await
            {
                Ok(true) => {
                    remote_port = Some(candidate);
                    break;
                }
                Ok(false) => {}
                Err(error) => {
                    return Err(AppError::new(
                        error.code,
                        format!("检查远程端口失败：{}", error.message),
                    ));
                }
            }
        }
        let remote_port = remote_port.ok_or_else(|| {
            AppError::new(
                ErrorCode::Http,
                "52FRP 节点上没有找到可用的远程端口，请更换节点",
            )
        })?;
        self.emit("port", "分配远程端口", "ok", Some(remote_port.to_string()));

        self.emit("tunnel", "创建隧道", "running", None);
        let name = request
            .name
            .clone()
            .unwrap_or_else(|| format!("SRL_{}", &uuid::Uuid::new_v4().to_string()[..8]));
        let name = if provider.check_name(&name).await.unwrap_or(false) {
            name
        } else {
            format!("SRL_{}", &uuid::Uuid::new_v4().to_string()[..8])
        };
        let tunnel_id = provider
            .create_tunnel(&TunnelCreate {
                node_id: node.id,
                proxy_name: name.clone(),
                proxy_type: request.edition.tunnel_type().into(),
                local_ip: local_ip.clone(),
                local_port,
                remote_port,
            })
            .await?;
        *created_tunnel = Some(tunnel_id);
        self.emit("tunnel", "创建隧道", "ok", Some(format!("#{tunnel_id}")));

        self.emit("config", "获取 FRPC 配置", "running", None);
        let config_text = provider.tunnel_config(node.id, tunnel_id).await?;
        self.emit("config", "获取 FRPC 配置", "ok", None);

        self.emit("frpc", "下载并启动 FRPC", "running", None);
        let prepared = self.frpc.prepare(Some(self.frpc_progress())).await?;
        self.frpc.set_prepared(&prepared).await;
        let frpc_pid = self.frpc.start(&prepared.path, &config_text).await?;
        self.emit(
            "frpc",
            "下载并启动 FRPC",
            "ok",
            Some(format!("PID {frpc_pid}")),
        );

        self.emit(
            "verify",
            "等待 52FRP 云端确认",
            "running",
            Some("FRPC 已启动，正在等待隧道在线".into()),
        );
        let mut online = false;
        for _ in 0..23 {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if provider.tunnel_online(tunnel_id).await.unwrap_or(false) {
                online = true;
                break;
            }
        }
        if !online {
            return Err(AppError::new(
                ErrorCode::ConnectFailed,
                "FRPC 已启动，但 52FRP 在 45 秒内没有报告隧道在线",
            ));
        }
        self.emit("verify", "等待 52FRP 云端确认", "ok", None);

        let local_started_at = match pid {
            Some(pid) => process_start_time(pid).await,
            None => None,
        };
        Ok(StoredRoom {
            tunnel_id,
            tunnel_name: name,
            edition: request.edition,
            instance_id: instance.id.clone(),
            instance_name: instance.name.clone(),
            version_id: instance.version_id.clone(),
            node_id: node.id,
            node_name: selected.name.clone(),
            public_host: node.host.clone(),
            public_port: remote_port,
            local_ip,
            local_port,
            local_pid: pid,
            local_started_at,
            latency_ms: selected.latency_ms,
            created_at: chrono::Utc::now().to_rfc3339(),
            retained: false,
        })
    }

    pub async fn stop_room(&self, delete_tunnel: bool) -> Result<StopRoomResult> {
        let room = self.room.read().await.clone();
        let Some(room) = room else {
            return Ok(StopRoomResult {
                local_stopped: true,
                tunnel_deleted: false,
                message: "当前没有联机房间".into(),
            });
        };
        let tunnel_id = room.tunnel_id;
        self.frpc.stop().await?;
        let _ = journal::append(
            &self.paths,
            "frpc",
            "info",
            "frpc.stop",
            "FRPC 已停止",
            Some(room.tunnel_id),
        )
        .await;
        if !delete_tunnel {
            let mut retained = room;
            retained.retained = true;
            save_room(&self.paths, &retained).await?;
            *self.room.write().await = Some(retained);
            let _ = journal::append(
                &self.paths,
                "room",
                "warning",
                "room.retained",
                "房间已停止，云端隧道保留",
                Some(tunnel_id),
            )
            .await;
            *self.last_room_state.write().await = Some("retained".into());
            return Ok(StopRoomResult {
                local_stopped: true,
                tunnel_deleted: false,
                message: "FRPC 已停止；云端隧道保留，可稍后删除".into(),
            });
        }

        let provider = self.provider();
        let tunnels = provider.list_tunnels().await?;
        let found = tunnels.into_iter().find(|t| t.id == room.tunnel_id);
        match found {
            Some(t) if !t.name.starts_with(DEFAULT_TUNNEL_PREFIX) => {
                return Err(AppError::invalid_input(
                    "拒绝删除：目标隧道不是 SRL 创建的临时隧道",
                ));
            }
            None => {
                let _ = std::fs::remove_file(self.paths.multiplayer_room_file());
                *self.room.write().await = None;
                *self.last_room_state.write().await = None;
                *self.tunnel_cache.write().await = None;
                let _ = journal::append(
                    &self.paths,
                    "room",
                    "info",
                    "room.closed",
                    "房间已关闭；云端隧道已不存在",
                    Some(room.tunnel_id),
                )
                .await;
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
                *self.last_room_state.write().await = None;
                *self.tunnel_cache.write().await = None;
                let _ = journal::append(
                    &self.paths,
                    "room",
                    "info",
                    "room.closed",
                    "房间已关闭，临时隧道已删除",
                    Some(room.tunnel_id),
                )
                .await;
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

    pub async fn restart_frpc(&self) -> Result<FrpcView> {
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
        let provider = self.provider();
        let config_text = provider.tunnel_config(room.node_id, room.tunnel_id).await?;
        let prepared = self.frpc.prepare(None).await?;
        self.frpc.set_prepared(&prepared).await;
        self.frpc.start(&prepared.path, &config_text).await?;
        let _ = journal::append(
            &self.paths,
            "frpc",
            "info",
            "frpc.restart",
            "FRPC 已重新启动",
            Some(room.tunnel_id),
        )
        .await;
        *self.tunnel_cache.write().await = None;
        // A spawned process is not a restored tunnel. Confirm with the
        // provider before reporting success.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last = String::from("云端隧道尚未返回状态");
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, provider.tunnel_state(room.tunnel_id)).await {
                Ok(Ok(state)) if state.online => {
                    *self.tunnel_cache.write().await = Some((Instant::now(), state));
                    return Ok(self.frpc.status().await);
                }
                Ok(Ok(_)) => last = "云端隧道仍显示离线".into(),
                Ok(Err(error)) => last = error.message,
                Err(_) => last = "查询云端隧道超时".into(),
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            tokio::time::sleep(remaining.min(Duration::from_secs(1))).await;
        }
        Err(AppError::new(
            ErrorCode::Timeout,
            format!("FRPC 已启动，但未能在 10 秒内确认云端隧道恢复：{last}"),
        ))
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

    pub async fn stored_room(&self) -> Option<StoredRoom> {
        self.room.read().await.clone()
    }

    /// Current FRPC log ring buffer, used by the unified room log view.
    pub async fn frpc_log_tail(&self) -> Vec<String> {
        self.frpc.status().await.log_tail
    }
}

fn owner_capabilities() -> RoomCapabilities {
    RoomCapabilities {
        role: super::types::RoomRole::Owner,
        can_manage_room: true,
        can_view_member_ips: true,
        can_kick: false,
        can_ban: false,
        can_whitelist: false,
        can_op: false,
        note: "SRL 只管理自己的房间与隧道；当前没有经过验证的 Minecraft 服务端控制通道，因此踢出、封禁、白名单和 OP 操作不可用。".into(),
    }
}

/// Pick a candidate from the node's allowed ranges while avoiding denied and
/// already-occupied ports. The caller still asks 52FRP's check-port endpoint
/// before creating the tunnel.
pub fn choose_port(
    allowed: &[(u16, u16)],
    denied: &[(u16, u16)],
    occupied: &[u16],
    salt: u64,
) -> Option<u16> {
    if allowed.is_empty() {
        return None;
    }
    let mut state = salt.max(1);
    for _ in 0..200 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let (start, end) = allowed[(state as usize) % allowed.len()];
        let span = (end as u32).saturating_sub(start as u32) + 1;
        let port = start as u32 + (state as u32 % span);
        let port = port as u16;
        if port_in_ranges(denied, port) || occupied.contains(&port) {
            continue;
        }
        return Some(port);
    }
    None
}

/// Read the process start time so a reused PID is not mistaken for the
/// original game process. Returns Unix seconds.
#[allow(clippy::needless_return)]
async fn process_start_time(pid: u32) -> Option<u64> {
    #[cfg(unix)]
    {
        let output = tokio::process::Command::new("ps")
            .args(["-o", "etime=", "-p", &pid.to_string()])
            .output()
            .await
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let elapsed = parse_ps_etime(text.trim())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        return Some(now.saturating_sub(elapsed));
    }
    #[cfg(windows)]
    {
        let script = format!(
            "(Get-Process -Id {pid} -ErrorAction Stop).StartTime.ToUniversalTime().Subtract([datetime]'1970-01-01').TotalSeconds"
        );
        let output = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .await
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        return text.trim().parse::<f64>().ok().map(|v| v as u64);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        None
    }
}

fn parse_ps_etime(value: &str) -> Option<u64> {
    // [[DD-]HH:]MM:SS
    let (days, rest) = match value.split_once('-') {
        Some((days, rest)) => (days.parse::<u64>().ok()?, rest),
        None => (0, value),
    };
    let parts: Vec<&str> = rest.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [h, m, s] => (
            h.parse::<u64>().ok()?,
            m.parse::<u64>().ok()?,
            s.parse::<u64>().ok()?,
        ),
        [m, s] => (0, m.parse::<u64>().ok()?, s.parse::<u64>().ok()?),
        _ => return None,
    };
    Some(days * 86400 + hours * 3600 + minutes * 60 + seconds)
}

async fn process_matches(pid: u32, started_at: u64) -> bool {
    if !process_alive(pid).await {
        return false;
    }
    match process_start_time(pid).await {
        Some(actual) => actual.abs_diff(started_at) <= 3,
        None => false,
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
        output
            .ok()
            .is_some_and(|out| String::from_utf8_lossy(&out.stdout).contains(&pid.to_string()))
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

    #[test]
    fn chooses_port_inside_ranges_and_skips_occupied() {
        let allowed = vec![(20000, 20005)];
        let occupied = vec![20000, 20001, 20002];
        for salt in 1..20 {
            let port = choose_port(&allowed, &[], &occupied, salt).unwrap();
            assert!((20003..=20005).contains(&port));
        }
    }

    #[test]
    fn avoids_denied_ranges() {
        let allowed = vec![(20000, 20002)];
        let denied = vec![(20000, 20001)];
        assert_eq!(choose_port(&allowed, &denied, &[], 7), Some(20002));
    }

    #[test]
    fn parses_unix_ps_elapsed_time() {
        assert_eq!(parse_ps_etime("00:05"), Some(5));
        assert_eq!(parse_ps_etime("01:02:03"), Some(3723));
        assert_eq!(parse_ps_etime("2-01:02:03"), Some(176523));
        assert_eq!(parse_ps_etime("bad"), None);
    }

    #[tokio::test]
    async fn loads_and_saves_room() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        let room = StoredRoom {
            tunnel_id: 7,
            tunnel_name: "SRL_test".into(),
            edition: Edition::Java,
            instance_id: "abc".into(),
            instance_name: "Test".into(),
            version_id: "1.21.1".into(),
            node_id: 1,
            node_name: "node".into(),
            public_host: "example.52frp.com".into(),
            public_port: 25565,
            local_ip: "127.0.0.1".into(),
            local_port: 25565,
            local_pid: Some(123),
            local_started_at: None,
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
