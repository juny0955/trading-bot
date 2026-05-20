use crate::adapters::binance::api::BinanceOrderApi;
use crate::adapters::binance::ws::reconnect::{ReconnectConfig, WsSession, run_with_reconnect};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tokio::time;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub struct OrderWsLoop {
    api: Arc<BinanceOrderApi>,
}

struct OrderStreamSession {
    api: Arc<BinanceOrderApi>,
    ws_rx: Receiver<String>,
}

#[async_trait]
impl WsSession for OrderStreamSession {
    async fn run_once(&mut self, token: CancellationToken) -> Result<()> {
        let (ws_stream, _) = connect_async(&self.api.order_ws_url)
            .await
            .inspect_err(|e| error!(error = %e, "Order WS 연결 실패"))?;
        info!(url = %self.api.order_ws_url, "Order WS 연결");
        let (mut write, mut read) = ws_stream.split();

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    let _ = write.send(Message::Close(None)).await;
                    info!("Order WS Close Frame 전송 후 종료");
                    return Ok(());
                }
                msg = self.ws_rx.recv() => match msg {
                    Some(text) => { write.send(Message::Text(text.into())).await?; }
                    None => { info!("Order WS 채널 닫힘"); return Ok(()); }
                },
                msg = time::timeout(Duration::from_secs(30), read.next()) => {
                    match msg {
                        Ok(Some(Ok(Message::Text(text)))) => {
                            if let Ok(val) = serde_json::from_str::<Value>(&text)
                            && let Some(id) = val["id"].as_str()
                            && let Some(tx) = self.api.pending.lock().await.remove(id) {
                                let _ = tx.send(val);
                            }
                        }
                        Ok(Some(Ok(Message::Ping(payload)))) => {
                            write.send(Message::Pong(payload)).await?;
                        }
                        Ok(Some(Ok(Message::Close(_)))) => return Err(anyhow!("Order WS 서버 측 Close")),
                        Ok(None) => return Err(anyhow!("Order WS 스트림 종료")),
                        Err(_) => return Err(anyhow!("Order WS 타임아웃 30s")),
                        _ => {}
                    }
                }
            }
        }
    }

    async fn on_disconnect(&mut self) {
        let mut p = self.api.pending.lock().await;
        let n = p.len();
        p.clear();
        if n > 0 {
            warn!(count = n, "Order WS 재연결 — 대기 중 요청 실패 처리");
        }
    }
}

impl OrderWsLoop {
    pub fn new(api: Arc<BinanceOrderApi>) -> Self {
        Self { api }
    }

    pub async fn run_order_stream(&self, ws_rx: Receiver<String>, token: CancellationToken) {
        run_with_reconnect(
            OrderStreamSession {
                api: Arc::clone(&self.api),
                ws_rx,
            },
            ReconnectConfig {
                label: "Order WS".to_string(),
                reconnect_delay: Duration::from_secs(3),
            },
            token,
        )
        .await;
    }
}
