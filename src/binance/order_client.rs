use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::{
    Mutex,
    mpsc::{self, Receiver, Sender},
    oneshot,
};
use tokio_util::sync::CancellationToken;
use tracing::info;
use uuid::Uuid;

use crate::order::types::Fill;
use crate::storage::postgres::order::OrderStorage;
use crate::{
    binance::signing::{sign, timestamp_ms},
    order::{
        executor::OrderExecutor,
        types::{Order, OrderError, OrderRequest, OrderStatus},
    },
};

pub struct LiveOrderExecutor {
    pub(crate) api_key: String,
    pub(crate) secret: String,
    pub(crate) order_ws_url: String,
    pub(crate) user_stream_base: String,
    pub(crate) rest_base: String,

    pub(crate) ws_tx: Sender<String>,
    pub(crate) pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,

    pub(crate) storage: OrderStorage,
    pub(crate) fill_tx: Sender<Fill>,
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
        fill_tx: Sender<Fill>,
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
            fill_tx,
            pending: Arc::new(Mutex::new(HashMap::new())),
            storage: OrderStorage::new(pool),
        });
        (executor, ws_rx)
    }

    pub fn start_order_stream(self: Arc<Self>, ws_rx: Receiver<String>, token: CancellationToken) {
        info!("Order WS Worker 시작");
        tokio::spawn(async move {
            self.run_order_stream(ws_rx, token).await;
        });
    }

    pub fn start_user_stream(self: Arc<Self>, token: CancellationToken) {
        info!("User WS Worker 시작");
        tokio::spawn(async move {
            self.run_user_stream(token).await;
        });
    }

    async fn get_order(&self, order_id: Uuid) -> Result<Order, OrderError> {
        self.storage
            .find_by_id(order_id)
            .await
            .map_err(OrderError::Storage)?
            .ok_or_else(|| OrderError::NotFound(order_id.to_string()))
    }
}
