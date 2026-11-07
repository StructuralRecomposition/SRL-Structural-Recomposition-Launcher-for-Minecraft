//! Minecraft Multiplayer: provider-neutral tunnel orchestration.
//!
//! The UI talks to `MultiplayerManager`; provider-specific HTTP details live in
//! `locyan`. Credentials are stored through the existing OS keyring abstraction
//! and FRPC config files are private, short-lived temporary files.

pub mod auth;
pub mod frpc;
pub mod locyan;
pub mod manager;
pub mod minecraft;
pub mod nodes;
pub mod provider;
pub mod types;

pub use manager::MultiplayerManager;
pub use types::{
    AccountView, CreateRoomRequest, Edition, FrpcView, LocalServiceView, MultiplayerConfig,
    MultiplayerStatus, NodeView, OrphanTunnel, ProgressEvent, ProgressSink, RoomView,
    StopRoomResult,
};
