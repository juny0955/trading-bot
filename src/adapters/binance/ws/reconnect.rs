use async_trait::async_trait;
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

#[derive(Debug, thiserror::Error)]
pub enum WsError {
    #[error("retryable: {0:#}")]
    Retryable(#[source] anyhow::Error),
    #[error("fatal: {0:#}")]
    Fatal(#[source] anyhow::Error),
}

impl WsError {
    pub fn retryable(e: impl Into<anyhow::Error>) -> Self {
        Self::Retryable(e.into())
    }
    pub fn fatal(e: impl Into<anyhow::Error>) -> Self {
        Self::Fatal(e.into())
    }
}

pub struct ReconnectConfig {
    pub label: String,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
    pub max_retries: Option<u32>, // None=무한, Some(N)=상한
}

impl ReconnectConfig {
    pub fn defaults(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            max_retries: None,
        }
    }
}

#[async_trait]
pub trait WsSession: Send {
    async fn run_once(&mut self, token: CancellationToken) -> Result<(), WsError>;
    async fn on_disconnect(&mut self) {}
}

pub async fn run_with_reconnect<S: WsSession>(
    mut session: S,
    cfg: ReconnectConfig,
    token: CancellationToken,
) {
    let mut attempt: u32 = 0;
    let mut delay = cfg.initial_delay;

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                info!(label = %cfg.label, "WS 종료");
                return;
            }
            res = session.run_once(token.clone()) => match res {
                Ok(()) => return,
                Err(WsError::Fatal(e)) => {
                    error!(label = %cfg.label, error = %e, "재연결 중단");
                    session.on_disconnect().await;
                    return;
                }
                Err(WsError::Retryable(e)) => {
                    session.on_disconnect().await;
                    attempt += 1;
                    if let Some(limit) = cfg.max_retries && attempt > limit {
                        error!(label = %cfg.label, attempt = %attempt, limit = %limit, error = %e, "재시도 초과");
                        return;
                    }

                    warn!(label = %cfg.label, error = %e, delay_sec = delay.as_secs(), "재연결 예정");
                    tokio::select! {
                        _ = token.cancelled() => return,
                        _ = sleep(delay) => {}
                    }
                    let next = delay.mul_f64(cfg.backoff_multiplier);
                    delay = if next > cfg.max_delay { cfg.max_delay } else { next };
                }
            }
        }
    }
}
