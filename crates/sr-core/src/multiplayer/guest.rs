//! Guest-side network monitor.
//!
//! The monitor samples the target server with the real status protocol. It is
//! explicitly not a game-session tracker: SRL cannot prove that Minecraft
//! actually joined the world, and never reports that it did.

use super::server_status::{parse_address, ping_server, OnlineMode, ServerStatusReport};
use crate::error::{AppError, Result};
use crate::util;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const MIN_INTERVAL_SECS: u64 = 10;
const MAX_INTERVAL_SECS: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestSample {
    pub checked_at: String,
    pub latency_ms: Option<u64>,
    pub ok: bool,
    pub version_name: Option<String>,
    pub protocol: Option<i32>,
    pub online_players: Option<u64>,
    pub max_players: Option<u64>,
    pub motd: Option<String>,
    pub error_stage: Option<String>,
    pub error_message: Option<String>,
    pub consecutive_failures: u32,
    pub last_ok_at: Option<String>,
    pub online_mode: OnlineMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestMonitorStatus {
    pub running: bool,
    pub target: Option<String>,
    pub interval_seconds: u64,
    pub samples: u64,
    pub started_at: Option<String>,
    pub sample: Option<GuestSample>,
    pub last_report: Option<ServerStatusReport>,
}

impl Default for GuestMonitorStatus {
    fn default() -> Self {
        Self {
            running: false,
            target: None,
            interval_seconds: 12,
            samples: 0,
            started_at: None,
            sample: None,
            last_report: None,
        }
    }
}

struct Inner {
    state: Mutex<GuestMonitorStatus>,
    cancel: Mutex<Option<CancellationToken>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone)]
pub struct GuestMonitor {
    inner: Arc<Inner>,
}

impl Default for GuestMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl GuestMonitor {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(GuestMonitorStatus::default()),
                cancel: Mutex::new(None),
                handle: Mutex::new(None),
            }),
        }
    }

    pub async fn start(
        &self,
        target: &str,
        interval_secs: Option<u64>,
    ) -> Result<GuestMonitorStatus> {
        let parsed =
            parse_address(target).map_err(|failure| AppError::invalid_input(failure.message))?;
        let interval = interval_secs
            .unwrap_or(12)
            .clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS);
        self.stop().await;
        let cancel = CancellationToken::new();
        {
            let mut state = self.inner.state.lock().await;
            *state = GuestMonitorStatus {
                running: true,
                target: Some(parsed.display()),
                interval_seconds: interval,
                samples: 0,
                started_at: Some(util::now_rfc3339()),
                sample: None,
                last_report: None,
            };
        }
        *self.inner.cancel.lock().await = Some(cancel.clone());
        let inner = self.inner.clone();
        let address = parsed.display();
        let handle = tokio::spawn(async move {
            loop {
                let report = ping_server(&address).await;
                let mut state = inner.state.lock().await;
                if !state.running {
                    return;
                }
                let now = util::now_rfc3339();
                let previous_ok = state
                    .sample
                    .as_ref()
                    .and_then(|sample| sample.last_ok_at.clone());
                let failures = if report.ok {
                    0
                } else {
                    state
                        .sample
                        .as_ref()
                        .map(|sample| sample.consecutive_failures)
                        .unwrap_or(0)
                        + 1
                };
                let last_ok = if report.ok {
                    Some(now.clone())
                } else {
                    previous_ok
                };
                state.samples = state.samples.saturating_add(1);
                state.sample = Some(GuestSample {
                    checked_at: now,
                    latency_ms: report.latency_ms,
                    ok: report.ok,
                    version_name: report.version_name.clone(),
                    protocol: report.protocol,
                    online_players: report.online_players,
                    max_players: report.max_players,
                    motd: report.motd.clone(),
                    error_stage: report.error.as_ref().map(|error| error.stage.clone()),
                    error_message: report.error.as_ref().map(|error| error.message.clone()),
                    consecutive_failures: failures,
                    last_ok_at: last_ok,
                    online_mode: report.online_mode.clone(),
                });
                state.last_report = Some(report);
                drop(state);
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
                }
            }
            let mut state = inner.state.lock().await;
            state.running = false;
        });
        *self.inner.handle.lock().await = Some(handle);
        Ok(self.status().await)
    }

    pub async fn stop(&self) -> GuestMonitorStatus {
        if let Some(cancel) = self.inner.cancel.lock().await.take() {
            cancel.cancel();
        }
        if let Some(handle) = self.inner.handle.lock().await.take() {
            handle.abort();
        }
        let mut state = self.inner.state.lock().await;
        state.running = false;
        state.clone()
    }

    pub async fn status(&self) -> GuestMonitorStatus {
        self.inner.state.lock().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_invalid_target() {
        let monitor = GuestMonitor::new();
        assert!(monitor.start("not a host", Some(12)).await.is_err());
        assert!(!monitor.status().await.running);
    }

    #[tokio::test]
    async fn clamps_interval_and_stops() {
        let monitor = GuestMonitor::new();
        // No server is started; the first sample is allowed to fail, but the
        // monitor must report a real failure rather than inventing a session.
        let status = monitor
            .start("127.0.0.1:1", Some(1))
            .await
            .expect("address is syntactically valid");
        assert!(status.running);
        assert_eq!(status.interval_seconds, MIN_INTERVAL_SECS);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let stopped = monitor.stop().await;
        assert!(!stopped.running);
    }
}
