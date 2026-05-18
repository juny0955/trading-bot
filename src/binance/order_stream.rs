use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::{
    sync::{mpsc::Receiver, oneshot},
    time,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

use crate::{binance::LiveOrderExecutor, order::types::OrderError};
use anyhow::anyhow;
use tracing::{error, info, warn};

impl LiveOrderExecutor {
    pub(super) async fn run_order_stream(
        &self,
        mut ws_rx: Receiver<String>,
        token: CancellationToken,
    ) {
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!("Order WS Worker 종료");
                    return;
                }
                res = self.order_stream_session(&mut ws_rx, token.clone()) => {
                    if res.is_ok() { return; }
                    let pending_count = {
                        let mut p = self.pending.lock().await;
                        let n = p.len();
                        p.clear();
                        n
                    };
                    if pending_count > 0 {
                        warn!("Order WS 재연결 — 대기 중 요청 {} 건 실패 처리", pending_count);
                    }
                    error!("Order WS 에러, 3초 후 재연결");
                    tokio::select! {
                        _ = token.cancelled() => return,
                        _ = time::sleep(Duration::from_secs(3)) => {}
                    }
                }
            }
        }
    }

    async fn order_stream_session(
        &self,
        ws_rx: &mut Receiver<String>,
        token: CancellationToken,
    ) -> anyhow::Result<()> {
        let (ws_stream, _) = connect_async(&self.order_ws_url)
            .await
            .inspect_err(|e| error!("Order WS 연결 실패: {e}"))?;
        info!("Order WS 연결됨: {}", self.order_ws_url);
        let (mut write, mut read) = ws_stream.split();

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    let _ = write.send(Message::Close(None)).await;
                    info!("Order WS Close Frame 전송 후 종료");
                    return Ok(());
                }
                msg = ws_rx.recv() => match msg {
                    Some(text) => {
                        write.send(Message::Text(text.into())).await?;
                    }
                    None => {
                        info!("Order WS 채널 닫힘 — 종료");
                        return Ok(());
                    }
                },
                msg = time::timeout(Duration::from_secs(30), read.next()) => {
                    match msg {
                        Ok(Some(Ok(Message::Text(text)))) => {
                            if let Ok(val) = serde_json::from_str::<Value>(&text)
                            && let Some(id) = val["id"].as_str()
                            && let Some(tx) = self.pending.lock().await.remove(id) {
                                let _ = tx.send(val);
                            }
                        }
                        Ok(Some(Ok(Message::Ping(payload)))) => {
                            write.send(Message::Pong(payload)).await?;
                        }
                        Ok(Some(Ok(Message::Close(_)))) => {
                            return Err(anyhow!("Order WS 서버 측 Close"));
                        }
                        Ok(None) => {
                            return Err(anyhow!("Order WS 스트림 종료"));
                        }
                        Err(_) => {
                            return Err(anyhow!("Order WS 타임아웃 30s"));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub(super) async fn send_and_wait(
        &self,
        id: String,
        request: Value,
    ) -> Result<Value, OrderError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), resp_tx);

        if let Err(e) = self.ws_tx.send(request.to_string()).await {
            self.pending.lock().await.remove(&id);
            warn!("Order WS 송신 실패 (id={id}): {e}");
            return Err(OrderError::Connection(e.to_string()));
        }

        match time::timeout(Duration::from_secs(5), resp_rx).await {
            Ok(Ok(val)) => {
                if val["status"].as_u64() != Some(200) {
                    let code = val["error"]["code"].as_i64().unwrap_or(-1) as i32;
                    let msg = val["error"]["msg"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string();
                    warn!("Order WS 거래소 거부 (id={id}, code={code}): {msg}");
                    return Err(OrderError::ExchangeRejected { code, msg });
                }
                Ok(val["result"].clone())
            }
            Ok(Err(_)) => {
                warn!("Order WS 채널 닫힘 (id={id})");
                Err(OrderError::Connection("channel closed".into()))
            }
            Err(_) => {
                self.pending.lock().await.remove(&id);
                warn!("Order WS 응답 timeout (id={id})");
                Err(OrderError::Connection("timeout".into()))
            }
        }
    }
}
