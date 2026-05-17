use crate::order::executor::OrderExecutor;
use crate::order::storage::OrderStorage;
use crate::order::types::{Fill, Order, OrderError, OrderRequest, OrderSide, OrderStatus};
use crate::storage::event::StorageEvent;
use anyhow::anyhow;
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

pub struct LiveOrderExecutor {
    pub(crate) api_key: String,
    pub(crate) secret: String,
    pub(crate) order_ws_url: String,
    pub(crate) user_stream_base: String,
    pub(crate) rest_base: String,

    pub(crate) ws_tx: Sender<String>,
    pub(crate) pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,

    pub(crate) storage: OrderStorage,
    pub(crate) db_tx: Sender<StorageEvent>,
}

#[async_trait]
impl OrderExecutor for LiveOrderExecutor {
    async fn submit(&self, request: OrderRequest) -> Result<Order, OrderError> {
        let order = Order::new_from_request(request.clone());
        let id = order.id.to_string();
        let ts = timestamp_ms();

        let params_str = format!(
            "symbol={}&side={}&type=MARKET&quantity={}&timestamp={}",
            request.symbol,
            format!("{:?}", request.side).to_uppercase(),
            request.qty,
            ts
        );
        let sig = sign(&self.secret, &params_str);

        let req = json!({
            "id": id,
            "method": "order.place",
            "params": {
                "symbol": request.symbol,
                "side": format!("{:?}", request.side).to_uppercase(),
                "type": "MARKET",
                "quantity": request.qty,
                "timestamp": ts,
                "apiKey": self.api_key,
                "signature": sig,
            }
        });
        let result = self.send_and_wait(id, req).await?;

        let mut filled = order.clone();
        filled.exchange_order_id = result["orderId"].as_i64();
        filled.status = OrderStatus::New;
        self.storage
            .upsert_order(&filled)
            .await
            .map_err(OrderError::Storage)?;

        Ok(filled)
    }

    async fn cancel(&self, order_id: Uuid) -> Result<Order, OrderError> {
        let order = self.get_order(order_id).await?;
        let exchange_id = order
            .exchange_order_id
            .ok_or_else(|| OrderError::NotFound("exchange_order_id 없음".into()))?;

        let id = Uuid::now_v7().to_string();
        let ts = timestamp_ms();
        let params_str = format!(
            "symbol={}&orderId={}&timestamp={}",
            order.symbol, exchange_id, ts
        );
        let sig = sign(&self.secret, &params_str);

        let req = json!({
            "id": id,
            "method": "order.cancel",
            "params": {
                "symbol": order.symbol,
                "orderId": exchange_id,
                "timestamp": ts,
                "apiKey": self.api_key,
                "signature": sig
            }
        });
        let result = self.send_and_wait(id, req).await?;

        let mut cancelled = order;
        if result["status"].as_str() == Some("CANCELED") {
            cancelled.status = OrderStatus::Cancelled;
            cancelled.updated_at = Utc::now();
            self.storage
                .upsert_order(&cancelled)
                .await
                .map_err(OrderError::Storage)?;
        }
        Ok(cancelled)
    }

    async fn query(&self, order_id: Uuid) -> Result<Order, OrderError> {
        let order = self.get_order(order_id).await?;
        let exchange_id = order
            .exchange_order_id
            .ok_or_else(|| OrderError::NotFound("exchange_order_id 없음".into()))?;

        let id = Uuid::now_v7().to_string();
        let ts = timestamp_ms();
        let params_str = format!(
            "symbol={}&orderId={}&timestamp={}",
            order.symbol, exchange_id, ts
        );
        let sig = sign(&self.secret, &params_str);

        let req = json!({
            "id": id,
            "method": "order.status",
            "params": {
                "symbol": order.symbol,
                "orderId": exchange_id,
                "timestamp": ts,
                "apiKey": self.api_key,
                "signature": sig
            }
        });
        let result = self.send_and_wait(id, req).await?;

        let mut updated = order;
        updated.status = match result["status"].as_str() {
            Some("FILLED") => OrderStatus::Filled,
            Some("CANCELED") => OrderStatus::Cancelled,
            Some("PARTIALLY_FILLED") => OrderStatus::PartiallyFilled,
            Some("EXPIRED") => OrderStatus::Expired,
            Some("REJECTED") => OrderStatus::Rejected,
            _ => updated.status,
        };
        updated.updated_at = Utc::now();
        self.storage
            .upsert_order(&updated)
            .await
            .map_err(OrderError::Storage)?;
        Ok(updated)
    }

    async fn open_orders(&self, symbol: &str) -> Result<Vec<Order>, OrderError> {
        let id = Uuid::now_v7().to_string();
        let ts = timestamp_ms();
        let params_str = format!("symbol={}&timestamp={}", symbol, ts);
        let sig = sign(&self.secret, &params_str);

        let req = json!({
            "id": id,
            "method": "openOrders.status",
            "params": {
                "symbol": symbol,
                "timestamp": ts,
                "apiKey": self.api_key,
                "signature": sig
            }
        });
        let result = self.send_and_wait(id, req).await?;

        // Binance에 살아있는 orderId 집합
        let binance_ids: std::collections::HashSet<i64> = result
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|o| o["orderId"].as_i64())
            .collect();

        let mut db_orders = self
            .storage
            .find_open(symbol)
            .await
            .map_err(OrderError::Storage)?;

        for order in &mut db_orders {
            if let Some(eid) = order.exchange_order_id
                && !binance_ids.contains(&eid)
            {
                order.status = OrderStatus::Expired;
                order.updated_at = Utc::now();
                let _ = self.storage.upsert_order(order).await;
            }
        }

        Ok(db_orders
            .into_iter()
            .filter(|o| matches!(o.status, OrderStatus::New | OrderStatus::PartiallyFilled))
            .collect())
    }
}

impl LiveOrderExecutor {
    pub fn new(
        api_key: String,
        secret: String,
        pool: PgPool,
        db_tx: Sender<StorageEvent>,
        testnet: bool,
    ) -> (Arc<Self>, Receiver<String>) {
        let (ws_tx, ws_rx) = mpsc::channel(100);
        let order_ws_url = if testnet {
            "wss://testnet.binancefuture.com/ws-fapi/v1".to_string()
        } else {
            "wss://ws-fapi.binance.com/ws-fapi/v1".to_string()
        };

        let user_stream_base = if testnet {
            "wss://stream.binancefuture.com/ws".to_string()
        } else {
            "wss://fstream.binance.com/ws".to_string()
        };

        let rest_base = if testnet {
            "https://testnet.binancefuture.com".to_string()
        } else {
            "https://fapi.binance.com".to_string()
        };

        let executor = Arc::new(Self {
            api_key,
            secret,
            order_ws_url,
            user_stream_base,
            rest_base,
            ws_tx,
            db_tx,
            pending: Arc::new(Mutex::new(HashMap::new())),
            storage: OrderStorage::new(pool),
        });
        (executor, ws_rx)
    }

    pub fn start(self: Arc<Self>, ws_rx: Receiver<String>, token: CancellationToken) {
        info!("Order WS Worker 시작");
        tokio::spawn(async move {
            self.run_order_ws(ws_rx, token).await;
        });
    }

    pub fn start_user_stream(self: Arc<Self>, token: CancellationToken) {
        info!("User WS Worker 시작");
        tokio::spawn(async move {
            self.run_user_ws(token).await;
        });
    }

    async fn run_order_ws(&self, mut ws_rx: Receiver<String>, token: CancellationToken) {
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!("Order WS Worker 종료");
                    return;
                }
                res = self.order_ws_session(&mut ws_rx, token.clone()) => {
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

    async fn run_user_ws(&self, token: CancellationToken) {
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!("User WS Worker 종료");
                    return;
                }
                res = self.user_ws_session(token.clone()) => {
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

    async fn order_ws_session(
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

    async fn user_ws_session(&self, token: CancellationToken) -> anyhow::Result<()> {
        let listen_key = self.get_listen_key().await?;
        let url = format!("{}/{}", self.user_stream_base, listen_key);
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
                     if let Err(e) = self.renew_listen_key(&listen_key).await {
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

    async fn get_listen_key(&self) -> anyhow::Result<String> {
        let url = format!("{}/fapi/v1/listenKey", self.rest_base);
        let val = reqwest::Client::new()
            .post(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?
            .json::<Value>()
            .await?;

        val["listenKey"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("listenKey 없음"))
    }

    async fn renew_listen_key(&self, listen_key: &str) -> anyhow::Result<()> {
        let url = format!("{}/fapi/v1/listenKey", self.rest_base);
        reqwest::Client::new()
            .put(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .query(&[("listenKey", listen_key)])
            .send()
            .await?
            .error_for_status()?;

        Ok(())
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
        let order = match self.storage.find_by_exchange_id(exchange_order_id).await {
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

            if let Err(e) = self.storage.record_fill(&fill).await {
                error!("Fill PG 저장 실패 (orderId={}): {e}", exchange_order_id);
            }
            if let Err(e) = self.db_tx.send(StorageEvent::Fill(fill)).await {
                warn!("Fill QuestDB 채널 전송 실패: {e}");
            }

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
            if let Err(e) = self.storage.upsert_order(&updated).await {
                error!("ORDER_TRADE_UPDATE 주문 갱신 실패: {e}");
            }
        }
    }
    async fn get_order(&self, order_id: Uuid) -> Result<Order, OrderError> {
        self.storage
            .find_by_id(order_id)
            .await
            .map_err(OrderError::Storage)?
            .ok_or_else(|| OrderError::NotFound(order_id.to_string()))
    }

    async fn send_and_wait(&self, id: String, request: Value) -> Result<Value, OrderError> {
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

fn sign(secret: &str, payload: &str) -> String {
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
