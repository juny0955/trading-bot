use async_trait::async_trait;
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub struct ReconnectConfig {
    pub label: String,
    pub reconnect_delay: Duration,
}

#[async_trait]
pub trait WsSession: Send {
    async fn run_once(&mut self, token: CancellationToken) -> anyhow::Result<()>;
    async fn on_disconnect(&mut self) {}
}

pub async fn run_with_reconnect<S: WsSession>(
    mut session: S,
    cfg: ReconnectConfig,
    token: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = token.cancelled() => {
                info!(label = %cfg.label, "WS 종료");
                return;
            }
            res = session.run_once(token.clone()) => {
                match res {
                    Ok(()) => return,
                    Err(e) => {
                        session.on_disconnect().await;
                        error!(label = %cfg.label, error = %e, delay_sec = cfg.reconnect_delay.as_secs(), "재연결 예정");
                        tokio::select! {
                            _ = token.cancelled() => return,
                            _ = sleep(cfg.reconnect_delay) => {}
                        }
                    }
                }
            }
        }
    }
}
