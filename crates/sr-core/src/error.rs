use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable machine-readable error codes. The UI maps these to actionable copy;
/// they must not change casually.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidInput,
    PathUnsafe,
    NotFound,
    AlreadyExists,
    /// The requested operation is not allowed in the current lifecycle state.
    InvalidState,
    Io,
    Network,
    Http,
    ChecksumMismatch,
    Cancelled,
    Timeout,
    Unsupported,
    NotConfigured,
    AuthenticationRequired,
    VersionInvalid,
    JavaInvalid,
    JavaIncompatible,
    ProcessFailed,
    ZipUnsafe,
    Interrupted,
    /// Structural Connect: the EasyTier runtime is missing or too old.
    #[serde(rename = "CONNECT_RUNTIME_MISSING")]
    ConnectRuntimeMissing,
    /// Structural Connect: the local environment cannot run the feature.
    #[serde(rename = "CONNECT_UNAVAILABLE")]
    ConnectUnavailable,
    /// Structural Connect: a network/session operation failed.
    #[serde(rename = "CONNECT_FAILED")]
    ConnectFailed,
    /// Structural Connect: invite code is malformed, unknown or already used.
    #[serde(rename = "INVITE_INVALID")]
    InviteInvalid,
    /// Structural Connect: invite (or its credential) has expired.
    #[serde(rename = "INVITE_EXPIRED")]
    InviteExpired,
    /// Structural Connect: no Minecraft LAN listener could be confirmed.
    #[serde(rename = "LAN_PORT_NOT_FOUND")]
    LanPortNotFound,
    /// Structural Connect: the remote Minecraft port is not reachable.
    #[serde(rename = "PEER_UNREACHABLE")]
    PeerUnreachable,
    /// Structural Connect: strict P2P was requested but only a relay path exists.
    #[serde(rename = "STRICT_P2P_FAILED")]
    StrictP2PFailed,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppError {
    pub code: ErrorCode,
    /// Human-facing message (Chinese UI copy). Must state what happened.
    pub message: String,
    /// Extra context, never containing tokens or secrets.
    pub detail: Option<String>,
    pub retryable: bool,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
            retryable: false,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn retryable(mut self, yes: bool) -> Self {
        self.retryable = yes;
        self
    }

    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, msg)
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, msg)
    }
    pub fn unsupported(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unsupported, msg)
    }
    pub fn not_configured(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotConfigured, msg)
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, msg)
    }
    pub fn cancelled() -> Self {
        Self::new(ErrorCode::Cancelled, "任务已取消")
    }
    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Timeout, msg).retryable(true)
    }
    pub fn network(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::Network, msg).retryable(true)
    }
    pub fn checksum(expected: &str, actual: &str) -> Self {
        Self::new(
            ErrorCode::ChecksumMismatch,
            "文件校验失败，下载内容可能损坏",
        )
        .with_detail(format!("期望 SHA-1 {expected}，实际 {actual}"))
        .retryable(true)
    }
    pub fn path_unsafe(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::PathUnsafe, msg)
    }
    pub fn java_incompatible(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::JavaIncompatible, msg)
    }
    pub fn connect_runtime_missing(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::ConnectRuntimeMissing, msg)
    }
    pub fn connect_unavailable(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::ConnectUnavailable, msg)
    }
    pub fn connect_failed(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::ConnectFailed, msg).retryable(true)
    }
    pub fn invite_invalid(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InviteInvalid, msg)
    }
    pub fn invite_expired(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::InviteExpired, msg)
    }
    pub fn lan_port_not_found(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::LanPortNotFound, msg)
    }
    pub fn peer_unreachable(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::PeerUnreachable, msg).retryable(true)
    }
    pub fn strict_p2p_failed(msg: impl Into<String>) -> Self {
        Self::new(ErrorCode::StrictP2PFailed, msg).retryable(true)
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.detail {
            Some(d) => write!(f, "[{:?}] {} ({})", self.code, self.message, d),
            None => write!(f, "[{:?}] {}", self.code, self.message),
        }
    }
}

impl std::error::Error for AppError {}

impl From<zip::result::ZipError> for AppError {
    fn from(e: zip::result::ZipError) -> Self {
        Self::new(ErrorCode::ZipUnsafe, format!("ZIP 操作失败：{e}")).with_detail(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        let retryable = matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::TimedOut
        );
        Self::new(ErrorCode::Io, format!("文件操作失败：{}", e))
            .with_detail(e.to_string())
            .retryable(retryable)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::new(ErrorCode::Internal, "数据格式错误").with_detail(e.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            return Self::timeout("网络请求超时").with_detail(e.to_string());
        }
        if e.is_status() {
            let status = e.status().map(|s| s.as_u16()).unwrap_or(0);
            return Self::new(ErrorCode::Http, format!("服务器返回 HTTP {status}"))
                .with_detail(e.to_string())
                .retryable(status == 429 || status >= 500);
        }
        if e.is_connect() || e.is_request() {
            return Self::network("无法连接到服务器，请检查网络").with_detail(e.to_string());
        }
        Self::new(ErrorCode::Network, "网络请求失败")
            .with_detail(e.to_string())
            .retryable(true)
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_stable_code() {
        let e = AppError::checksum("aaa", "bbb");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "CHECKSUM_MISMATCH");
        assert_eq!(v["retryable"], true);
        assert!(v["message"].as_str().unwrap().contains("校验"));
    }

    #[test]
    fn connect_codes_are_stable() {
        assert_eq!(
            serde_json::to_value(ErrorCode::StrictP2PFailed).unwrap(),
            "STRICT_P2P_FAILED"
        );
        assert_eq!(
            serde_json::to_value(ErrorCode::LanPortNotFound).unwrap(),
            "LAN_PORT_NOT_FOUND"
        );
        assert_eq!(
            serde_json::to_value(ErrorCode::ConnectRuntimeMissing).unwrap(),
            "CONNECT_RUNTIME_MISSING"
        );
    }

    #[test]
    fn io_error_keeps_context() {
        let e: AppError =
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied").into();
        assert_eq!(e.code, ErrorCode::Io);
        assert!(e.detail.unwrap().contains("denied"));
    }
}
