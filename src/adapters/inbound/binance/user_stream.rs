use std::sync::Arc;
use std::time::Duration;

use crate::adapters::binance::api::BinanceOrderApi;
use crate::adapters::binance::dto_account::{BinanceOrderTradeUpdate, parse_account_update_event};
use crate::adapters::binance::ws::reconnect::{
    ReconnectConfig, WsError, WsSession, run_with_reconnect,
};
use crate::application::account_state::AccountState;
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
    account_state: Arc<AccountState>,
}

#[async_trait]
impl WsSession for UserStreamSession {
    async fn run_once(&mut self, token: CancellationToken) -> Result<(), WsError> {
        let listen_key = self.api.get_listen_key().await.map_err(|e| {
            let msg = format!("{e:#}");
            if msg.contains("429") {
                WsError::RateLimited { retry_after: None }
            } else if msg.contains("401") || msg.contains("403") {
                WsError::fatal(e)
            } else {
                WsError::retryable(e)
            }
        })?;
        let url = format!("{}/{}", self.api.user_stream_base, listen_key);
        let (ws_stream, _) = connect_async(&url)
            .await
            .inspect_err(|e| error!(error = %e, "User WS 연결 실패"))
            .map_err(WsError::retryable)?;
        info!(url = %url, "User WS 연결됨");
        let (mut write, mut read) = ws_stream.split();

        let mut renew_interval = time::interval(Duration::from_secs(30 * 60));
        renew_interval.tick().await;

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    if let Err(e) = write.send(Message::Close(None)).await {
                        warn!(error = %e, "User WS Close Frame 전송 실패");
                    }
                    info!("User WS Close Frame 전송 후 종료");
                    return Ok(());
                }
                _ = renew_interval.tick() => {
                    if let Err(e) = self.api.renew_listen_key(&listen_key).await {
                        warn!(error = %e, "listenKey 갱신 실패");
                        let msg = format!("{e:#}");
                        if msg.contains("429") {
                            return Err(WsError::RateLimited { retry_after: None });
                        }
                        return Err(WsError::Retryable(e));
                    }
                    info!("User WS listenKey 갱신 성공");
                }
                msg = time::timeout(Duration::from_secs(240), read.next()) => {
                    match msg {
                        Ok(Some(Ok(Message::Text(text)))) => {
                            if let Ok(val) = serde_json::from_str::<Value>(&text) {
                                match val["e"].as_str() {
                                    Some("ORDER_TRADE_UPDATE") => {
                                        match BinanceOrderTradeUpdate::from_value(&val) {
                                            Ok(dto) => {
                                                let event = dto.into_execution_event();
                                                if self.tx.send(event).await.is_err() {
                                                    warn!("OrderExecutionEvent 채널 닫힘 — 체결 이벤트 손실");
                                                    return Ok(());
                                                }
                                            }
                                            Err(e) => {
                                                warn!(error = %e, "ORDER_TRADE_UPDATE 파싱 실패");
                                            }
                                        }
                                    }
                                    Some("ACCOUNT_UPDATE") => {
                                        if let Some(ev) = parse_account_update_event(&val) {
                                            self.account_state.apply_account_update(&ev);
                                        } else {
                                            warn!("ACCOUNT_UPDATE 파싱 실패");
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Ok(Some(Ok(Message::Ping(payload)))) => {
                            write.send(Message::Pong(payload)).await
                            .map_err(WsError::retryable)?;
                        }
                        Ok(Some(Ok(Message::Close(_)))) => {
                            return Err(WsError::Retryable(anyhow!("서버 측 Close")));
                        }
                        Ok(None) => return Err(WsError::Retryable(anyhow!("스트림 종료"))),
                        Err(_) => return Err(WsError::Retryable(anyhow!("타임아웃"))),
                        _ => {}
                    }
                }
            }
        }
    }
}

impl BinanceUserStreamHandler {
    /// 새 Binance user stream handler를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use std::sync::Arc;
    /// # use trading_bot::adapters::binance::api::BinanceOrderApi;
    /// # use trading_bot::adapters::inbound::binance::user_stream::BinanceUserStreamHandler;
    /// # fn example(api: Arc<BinanceOrderApi>) {
    /// let handler = BinanceUserStreamHandler::new(api);
    /// # }
    /// ```
    pub fn new(api: Arc<BinanceOrderApi>) -> Self {
        Self { api }
    }

    pub async fn run_user_stream(
        &self,
        tx: Sender<OrderExecutionEvent>,
        account_state: Arc<AccountState>,
        token: CancellationToken,
    ) {
        run_with_reconnect(
            UserStreamSession {
                api: Arc::clone(&self.api),
                tx,
                account_state,
            },
            ReconnectConfig::defaults("User WS"),
            token,
        )
        .await
    }
}
