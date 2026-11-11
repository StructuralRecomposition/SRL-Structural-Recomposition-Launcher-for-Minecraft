//! Minecraft Multiplayer: provider-neutral tunnel orchestration.
//!
//! 52FRP is the only provider. Credentials are API tokens stored in the OS
//! keyring; FRPC configuration files are private, short-lived temporary files.

pub mod frp52;
pub mod frpc;
pub mod guest;
pub mod journal;
pub mod manager;
pub mod minecraft;
pub mod nodes;
pub mod provider;
pub mod server_status;
pub mod token;
pub mod types;

pub use guest::{GuestMonitor, GuestMonitorStatus, GuestSample};
pub use manager::MultiplayerManager;
pub use server_status::{ping_server, OnlineMode, ServerStatusReport};
pub use types::{
    AccountView, CreateRoomRequest, Edition, FrpcView, LocalServiceView, MembersView,
    MultiplayerStatus, NodeView, OrphanTunnel, PlayerMember, ProgressEvent, ProgressSink,
    RoomCapabilities, RoomLogEntry, RoomView, StopRoomResult, TunnelStateView,
};
