use crate::adapters::binance::api::BinanceOrderApi;
use crate::adapters::binance::signing;
use crate::domain::order::{
    Order, OrderError, OrderRequest, OrderSide, OrderStatus, OrderType, TimeInForce,
};
use crate::ports::order_executor::OrderExecutor;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub struct BinanceOrderExecutor {
    api: Arc<BinanceOrderApi>,
}

impl BinanceOrderExecutor {
    pub fn new(api: Arc<BinanceOrderApi>) -> Self {
        Self { api }
    }
}

#[async_trait]
impl OrderExecutor for BinanceOrderExecutor {
    async fn submit(&self, request: OrderRequest) -> Result<Order, OrderError> {
        let order = Order::new_from_request(request.clone());
        let id = order.id.to_string();
        let ts = signing::timestamp_ms();

        let params_str = format!(
            "symbol={}&side={}&type=MARKET&quantity={}&timestamp={}",
            request.symbol,
            format!("{:?}", request.side).to_uppercase(),
            request.qty,
            ts
        );
        let sig = signing::sign(&self.api.secret, &params_str);

        let req = json!({
            "id": id,
            "method": "order.place",
            "params": {
                "symbol": request.symbol,
                "side": format!("{:?}", request.side).to_uppercase(),
                "type": "MARKET",
                "quantity": request.qty,
                "timestamp": ts,
                "apiKey": self.api.api_key,
                "signature": sig,
            }
        });

        let result = self.api.send_and_wait(id, req).await?;
        let mut filled = order;
        filled.exchange_order_id = result["orderId"].as_i64();

        Ok(filled)
    }

    async fn cancel(&self, order: &Order) -> Result<Order, OrderError> {
        let exchange_id = order
            .exchange_order_id
            .ok_or_else(|| OrderError::NotFound("exchange_order_id 없음".into()))?;

        let id = Uuid::now_v7().to_string();
        let ts = signing::timestamp_ms();
        let params_str = format!(
            "symbol={}&orderId={}&timestamp={}",
            order.symbol, exchange_id, ts
        );
        let sig = signing::sign(&self.api.secret, &params_str);

        let req = json!({
            "id": id,
            "method": "order.cancel",
            "params": {
                "symbol": order.symbol,
                "orderId": exchange_id,
                "timestamp": ts,
                "apiKey": self.api.api_key,
                "signature": sig
            }
        });
        let result = self.api.send_and_wait(id, req).await?;

        let mut cancelled = order.clone();
        if result["status"].as_str() == Some("CANCELED") {
            cancelled.status = OrderStatus::Cancelled;
            cancelled.updated_at = Utc::now();
        }

        Ok(cancelled)
    }

    async fn query(&self, order: &Order) -> Result<Order, OrderError> {
        let exchange_id = order
            .exchange_order_id
            .ok_or_else(|| OrderError::NotFound("exchange_order_id 없음".into()))?;

        let id = Uuid::now_v7().to_string();
        let ts = signing::timestamp_ms();
        let params_str = format!(
            "symbol={}&orderId={}&timestamp={}",
            order.symbol, exchange_id, ts
        );
        let sig = signing::sign(&self.api.secret, &params_str);

        let req = json!({
            "id": id,
            "method": "order.status",
            "params": {
                "symbol": order.symbol,
                "orderId": exchange_id,
                "timestamp": ts,
                "apiKey": self.api.api_key,
                "signature": sig
            }
        });
        let result = self.api.send_and_wait(id, req).await?;

        let mut updated = order.clone();
        updated.status = match result["status"].as_str() {
            Some("FILLED") => OrderStatus::Filled,
            Some("CANCELED") => OrderStatus::Cancelled,
            Some("PARTIALLY_FILLED") => OrderStatus::PartiallyFilled,
            Some("EXPIRED") => OrderStatus::Expired,
            Some("REJECTED") => OrderStatus::Rejected,
            _ => updated.status,
        };
        updated.updated_at = Utc::now();
        Ok(updated)
    }

    async fn open_orders(&self, symbol: &str) -> Result<Vec<Order>, OrderError> {
        let id = Uuid::now_v7().to_string();
        let ts = signing::timestamp_ms();
        let params_str = format!("symbol={}&timestamp={}", symbol, ts);
        let sig = signing::sign(&self.api.secret, &params_str);

        let req = json!({
            "id": id,
            "method": "openOrders.status",
            "params": {
                "symbol": symbol,
                "timestamp": ts,
                "apiKey": self.api.api_key,
                "signature": sig
            }
        });
        let result = self.api.send_and_wait(id, req).await?;

        Ok(result
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|o| {
                let exchange_id = o["orderId"].as_i64()?;
                let side = match o["side"].as_str()? {
                    "BUY" => OrderSide::Buy,
                    "SELL" => OrderSide::Sell,
                    _ => return None,
                };
                Some(Order {
                    id: Uuid::nil(),
                    client_order_id: o["clientOrderId"].as_str().unwrap_or("").to_string(),
                    exchange_order_id: Some(exchange_id),
                    symbol: symbol.into(),
                    order_type: OrderType::Market,
                    side,
                    status: OrderStatus::New,
                    qty: o["origQty"]
                        .as_str()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or_default(),
                    price: None,
                    stop_price: None,
                    filled_qty: o["executedQty"]
                        .as_str()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or_default(),
                    avg_fill_price: None,
                    time_in_force: TimeInForce::Gtc,
                    reduce_only: false,
                    post_only: false,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                })
            })
            .collect())
    }
}
