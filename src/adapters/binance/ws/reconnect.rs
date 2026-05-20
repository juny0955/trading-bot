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
                info!("{} 종료", cfg.label);
                return;
            }
            res = session.run_once(token.clone()) => {
                match res {
                    Ok(()) => return,
                    Err(e) => {
                        session.on_disconnect().await;
                        error!("{} 에러:{e} {}s 후 재연결", cfg.label, cfg.reconnect_delay.as_secs());
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
