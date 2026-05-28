use crate::adapters::binance::api::BinanceOrderApi;
use crate::adapters::binance::ws::reconnect::{
    ReconnectConfig, WsError, WsSession, run_with_reconnect,
};
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
    async fn run_once(&mut self, token: CancellationToken) -> Result<(), WsError> {
        let (ws_stream, _) = match connect_async(&self.api.order_ws_url).await {
            Ok(v) => v,
            Err(e) => {
                error!(error = %e, "Order WS 연결 실패");
                if let tokio_tungstenite::tungstenite::Error::Http(resp) = &e
                    && resp.status() == 429
                {
                    return Err(WsError::RateLimited { retry_after: None });
                }
                return Err(WsError::retryable(e));
            }
        };
        info!(url = %self.api.order_ws_url, "Order WS 연결");
        let (mut write, mut read) = ws_stream.split();

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    if let Err(e) = write.send(Message::Close(None)).await {
                        warn!(error = %e, "Order WS Close Frame 전송 실패");
                    }
                    info!("Order WS Close Frame 전송 후 종료");
                    return Ok(());
                }
                msg = self.ws_rx.recv() => match msg {
                    Some(text) => { write.send(Message::Text(text.into())).await.map_err(WsError::retryable)?; }
                    None => { info!("Order WS 채널 닫힘"); return Ok(()); }
                },
                msg = time::timeout(Duration::from_secs(240), read.next()) => {
                    match msg {
                        Ok(Some(Ok(Message::Text(text)))) => {
                            route_order_response(&self.api, &text).await;
                        }
                        Ok(Some(Ok(Message::Ping(payload)))) => {
                            write.send(Message::Pong(payload)).await.map_err(WsError::retryable)?;
                        }
                        Ok(Some(Ok(Message::Close(_)))) => return Err(WsError::retryable(anyhow!("서버 측 Close"))),
                        Ok(None) => return Err(WsError::retryable(anyhow!("스트림 종료"))),
                        Err(_) => return Err(WsError::retryable(anyhow!("타임아웃"))),
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

async fn route_order_response(api: &BinanceOrderApi, text: &str) {
    let Ok(val) = serde_json::from_str::<Value>(text) else {
        return;
    };
    let Some(id) = val["id"].as_str().map(str::to_string) else {
        return;
    };
    let Some(tx) = api.pending.lock().await.remove(&id) else {
        return;
    };

    if tx.send(val).is_err() {
        warn!(id = %id, "Order WS 응답 수신자가 이미 닫힘");
    }
}

impl OrderWsLoop {
    /// 새 주문 WebSocket 루프를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use std::sync::Arc;
    /// # use trading_bot::adapters::binance::api::BinanceOrderApi;
    /// # use trading_bot::adapters::outbound::binance::order::order_stream::OrderWsLoop;
    /// # fn example(api: Arc<BinanceOrderApi>) {
    /// let loop_runner = OrderWsLoop::new(api);
    /// # }
    /// ```
    pub fn new(api: Arc<BinanceOrderApi>) -> Self {
        Self { api }
    }

    pub async fn run_order_stream(&self, ws_rx: Receiver<String>, token: CancellationToken) {
        run_with_reconnect(
            OrderStreamSession {
                api: Arc::clone(&self.api),
                ws_rx,
            },
            ReconnectConfig::defaults("Order WS"),
            token,
        )
        .await;
    }
}
