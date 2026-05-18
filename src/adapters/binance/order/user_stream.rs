use std::sync::Arc;
use std::time::Duration;

use crate::adapters::binance::order::api::BinanceOrderApi;
use crate::domain::order::{Fill, OrderSide, OrderStatus};
use crate::ports::fill_sink::FillSink;
use crate::ports::order_repository::OrderRepository;
use anyhow::anyhow;
use chrono::{TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde_json::Value;
use tokio::time;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub struct BinanceUserStreamHandler {
    api: Arc<BinanceOrderApi>,
    repo: Arc<dyn OrderRepository>,
    fill_sink: Arc<dyn FillSink>,
}

impl BinanceUserStreamHandler {
    pub(crate) async fn run_user_stream(&self, token: CancellationToken) {
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!("User WS Worker 종료");
                    return;
                }
                res = self.user_stream_session(token.clone()) => {
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

    async fn user_stream_session(&self, token: CancellationToken) -> anyhow::Result<()> {
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
                                  self.handle_order_trade_update(&val).await;
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

    // o.x == "TRADE" 일 때만 Fill 생성. 주문 상태 갱신은 항상.
    //
    // Binance 필드 매핑:
    // - o.i → exchange_order_id (DB 조회 키)
    // - o.x → execution type ("TRADE" = 체결)
    // - o.X → order status ("FILLED", "PARTIALLY_FILLED", etc.)
    // - o.S → side ("BUY"/"SELL")
    // - o.l → last filled qty, o.L → last fill price
    // - o.n → fee, o.N → fee asset
    // - o.T → trade time ms
    // - o.z → cumulative filled qty, o.ap → avg fill price
    async fn handle_order_trade_update(&self, val: &Value) {
        let o = &val["o"];
        let exchange_order_id = match o["i"].as_i64() {
            Some(id) => id,
            None => {
                warn!("ORDER_TRADE_UPDATE: orderId 없음");
                return;
            }
        };
        let order = match self.repo.find_by_exchange_id(exchange_order_id).await {
            Ok(Some(o)) => o,
            Ok(None) => {
                warn!("ORDER_TRADE_UPDATE: orderId={exchange_order_id} DB에 없음");
                return;
            }
            Err(e) => {
                error!("ORDER_TRADE_UPDATE DB 조회 실패: {e}");
                return;
            }
        };

        if o["x"].as_str() == Some("TRADE") {
            let side = if o["S"].as_str() == Some("BUY") {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            };
            let qty: Decimal = o["l"].as_str().unwrap_or("0").parse().unwrap_or_default();
            let price: Decimal = o["L"].as_str().unwrap_or("0").parse().unwrap_or_default();
            let fee: Decimal = o["n"].as_str().unwrap_or("0").parse().unwrap_or_default();
            let fee_asset = o["N"].as_str().unwrap_or("USDT").to_string();
            let trade_ms = o["T"].as_i64().unwrap_or(0);
            let filled_at = Utc
                .timestamp_millis_opt(trade_ms)
                .single()
                .unwrap_or_else(Utc::now);

            let fill = Fill {
                order_id: order.id,
                symbol: order.symbol.clone(),
                side,
                qty,
                price,
                fee,
                fee_asset,
                filled_at,
            };

            if let Err(e) = self.repo.record_fill(&fill).await {
                error!("Fill PG 저장 실패 (orderId={}): {e}", exchange_order_id);
            }
            self.fill_sink.emit(fill).await;

            info!(
                "Fill 수신: symbol={}, side={side:?}, qty={qty}, price={price}",
                order.symbol
            );
        }

        let new_status = match o["X"].as_str() {
            Some("FILLED") => Some(OrderStatus::Filled),
            Some("PARTIALLY_FILLED") => Some(OrderStatus::PartiallyFilled),
            Some("CANCELED") => Some(OrderStatus::Cancelled),
            Some("EXPIRED") => Some(OrderStatus::Expired),
            Some("REJECTED") => Some(OrderStatus::Rejected),
            _ => None,
        };
        if let Some(status) = new_status {
            let filled_qty: Decimal = o["z"].as_str().unwrap_or("0").parse().unwrap_or_default();
            let avg_price: Decimal = o["ap"].as_str().unwrap_or("0").parse().unwrap_or_default();
            let mut updated = order;
            updated.status = status;
            updated.filled_qty = filled_qty;
            updated.avg_fill_price = if avg_price.is_zero() {
                None
            } else {
                Some(avg_price)
            };
            updated.updated_at = Utc::now();
            if let Err(e) = self.repo.upsert_order(&updated).await {
                error!("ORDER_TRADE_UPDATE 주문 갱신 실패: {e}");
            }
        }
    }
}
