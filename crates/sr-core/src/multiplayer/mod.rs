//! Minecraft Multiplayer: provider-neutral tunnel orchestration.
//!
//! 52FRP is the only provider. Credentials are API tokens stored in the OS
//! keyring; FRPC configuration files are private, short-lived temporary files.

pub mod frp52;
pub mod frpc;
pub mod manager;
pub mod minecraft;
pub mod nodes;
pub mod provider;
pub mod token;
pub mod types;

pub use manager::MultiplayerManager;
pub use types::{
    AccountView, CreateRoomRequest, Edition, FrpcView, LocalServiceView, MultiplayerStatus,
    NodeView, OrphanTunnel, ProgressEvent, ProgressSink, RoomView, StopRoomResult,
};
