use crate::order::executor::OrderExecutor;
use crate::order::storage::OrderStorage;
use crate::order::types::{Order, OrderError, OrderRequest, OrderStatus};
use crate::storage::event::StorageEvent;
use async_trait::async_trait;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{Mutex, mpsc, oneshot};
use uuid::Uuid;

pub struct LiveOrderExecutor {
    pub(crate) api_key: String,
    pub(crate) secret: String,
    pub(crate) order_ws_url: String,
    pub(crate) user_stream_base: String,
    pub(crate) rest_base: String,

    pub(crate) ws_tx: Sender<String>,
    pub(crate) pending: Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>>,

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

        let request_json = serde_json::json!({
            "id": id,
            "method": "order.place",
            "params": {
                "symbol": request.symbol,
                "side": format!("{:?}", request.side).to_uppercase(),
                "type": "MARKET",
                "quantity": request.qty,
                "timestamp": ts,
                "apikey": self.api_key,
                "signature": sig,
            }
        });

        let (resp_tx, resp_rx) = oneshot::channel();
        self.pending.lock().await.insert(id, resp_tx);

        self.ws_tx
            .send(request_json.to_string())
            .await
            .map_err(|e| OrderError::Connection(e.to_string()))?;

        let resp = tokio::time::timeout(Duration::from_secs(5), resp_rx)
            .await
            .map_err(|_| OrderError::Connection("timeout".into()))?
            .map_err(|_| OrderError::Connection("channel closed".into()))?;

        if resp["status"].as_u64() != Some(200) {
            let code = resp["error"]["code"].as_i64().unwrap_or(-1) as i32;
            let msg = resp["error"]["msg"]
                .as_str()
                .unwrap_or("unknown")
                .to_string();
            return Err(OrderError::ExchangeRejected { code, msg });
        }

        let mut filled = order.clone();
        filled.exchange_order_id = resp["result"]["orderId"].as_i64();
        filled.status = OrderStatus::New;
        self.storage
            .upsert_order(&filled)
            .await
            .map_err(OrderError::Storage)?;

        Ok(filled)
    }

    async fn cancel(&self, _order_id: Uuid) -> Result<Order, OrderError> {
        unimplemented!()
    }

    async fn query(&self, _order_id: Uuid) -> Result<Order, OrderError> {
        unimplemented!()
    }

    async fn open_orders(&self, _symbol: &str) -> Result<Vec<Order>, OrderError> {
        unimplemented!()
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
