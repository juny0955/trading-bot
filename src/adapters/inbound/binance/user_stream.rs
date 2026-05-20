use std::sync::Arc;
use std::time::Duration;

use crate::adapters::binance::api::BinanceOrderApi;
use crate::adapters::binance::dto_account::BinanceOrderTradeUpdate;
use crate::adapters::binance::ws::reconnect::{ReconnectConfig, WsSession, run_with_reconnect};
use crate::application::order_execution_event::OrderExecutionEvent;
use anyhow::anyhow;
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc::Sender;
use tokio::time;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub struct BinanceUserStreamHandler {
    api: Arc<BinanceOrderApi>,
}

struct UserStreamSession {
    api: Arc<BinanceOrderApi>,
    tx: Sender<OrderExecutionEvent>,
}

#[async_trait]
impl WsSession for UserStreamSession {
    async fn run_once(&mut self, token: CancellationToken) -> anyhow::Result<()> {
        let listen_key = self.api.get_listen_key().await?;
        let url = format!("{}/{}", self.api.user_stream_base, listen_key);
        let (ws_stream, _) = connect_async(&url)
            .await
            .inspect_err(|e| error!("User WS 연결 실패: {e}"))?;
        info!("User WS 연결됨: {}", url);
        let (mut write, mut read) = ws_stream.split();

        let mut renew_interval = time::interval(Duration::from_secs(30 * 60));
        renew_interval.tick().await;

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    let _ = write.send(Message::Close(None)).await;
                    info!("User WS Close Frame 전송 후 종료");
                    return Ok(());
                }
                _ = renew_interval.tick() => {
                    if let Err(e) = self.api.renew_listen_key(&listen_key).await {
                          warn!("User WS listenKey 갱신 실패: {e}");
                          return Err(anyhow!("listenKey 갱신 실패"));
                    }
                    info!("User WS listenKey 갱신됨");
                }
                msg = time::timeout(Duration::from_secs(60), read.next()) => {
                    match msg {
                        Ok(Some(Ok(Message::Text(text)))) => {
                            if let Ok(val) = serde_json::from_str::<Value>(&text) &&
                            let Ok(dto) = BinanceOrderTradeUpdate::from_value(&val) {
                                let event = dto.into_execution_event();
                                let _ = self.tx.send(event).await.ok();
                            } else {
                                warn!("ORDER_TRADE_UPDATE 파싱 실패");
                            }
                        }
                        Ok(Some(Ok(Message::Ping(payload)))) => {
                            write.send(Message::Pong(payload)).await?;
                        }
                        Ok(Some(Ok(Message::Close(_)))) => {
                            return Err(anyhow!("User WS 서버 측 Close"));
                        }
                        Ok(None) => return Err(anyhow!("User WS 스트림 종료")),
                        Err(_) => return Err(anyhow!("User WS 타임아웃 60s")),
                        _ => {}
                    }
                }
            }
        }
    }
}

impl BinanceUserStreamHandler {
    pub fn new(api: Arc<BinanceOrderApi>) -> Self {
        Self { api }
    }

    pub async fn run_user_stream(&self, tx: Sender<OrderExecutionEvent>, token: CancellationToken) {
        run_with_reconnect(
            UserStreamSession {
                api: Arc::clone(&self.api),
                tx,
            },
            ReconnectConfig {
                label: "User WS".to_string(),
                reconnect_delay: Duration::from_secs(3),
            },
            token,
        )
        .await
    }
}
