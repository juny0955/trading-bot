use std::sync::Arc;
use std::time::Duration;

use crate::adapters::binance::api::BinanceOrderApi;
use anyhow::anyhow;
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

impl BinanceUserStreamHandler {
    pub(crate) fn new(api: Arc<BinanceOrderApi>) -> Self {
        Self { api }
    }

    pub(crate) async fn run_user_stream(&self, tx: Sender<Value>, token: CancellationToken) {
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!("User WS Worker 종료");
                    return;
                }
                res = self.user_stream_session(&tx, token.clone()) => {
                    if res.is_ok() { return; }
                    error!("User WS 에러, 3초 후 재연결");
                    tokio::select! {
                        _ = token.cancelled() => return,
                        _ = time::sleep(Duration::from_secs(3)) => {}
                    }
                }
            }
        }
    }

    async fn user_stream_session(
        &self,
        tx: &Sender<Value>,
        token: CancellationToken,
    ) -> anyhow::Result<()> {
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
                              if let Ok(val) = serde_json::from_str::<Value>(&text)
                              && val["e"].as_str() == Some("ORDER_TRADE_UPDATE") {
                                  tx.send(val).await.ok();
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
