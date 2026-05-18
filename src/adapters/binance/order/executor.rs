use crate::adapters::binance::order::api::BinanceOrderApi;
use crate::adapters::binance::signing;
use crate::domain::order::{Order, OrderError, OrderRequest, OrderStatus};
use crate::ports::order_executor::OrderExecutor;
use crate::ports::order_repository::OrderRepository;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

pub struct BinanceOrderExecutor {
    api: Arc<BinanceOrderApi>,
    repo: Arc<dyn OrderRepository>,
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

        let mut filled = order.clone();
        filled.exchange_order_id = result["orderId"].as_i64();
        filled.status = OrderStatus::New;
        self.repo
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

        let mut cancelled = order;
        if result["status"].as_str() == Some("CANCELED") {
            cancelled.status = OrderStatus::Cancelled;
            cancelled.updated_at = Utc::now();
            self.repo
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
        self.repo
            .upsert_order(&updated)
            .await
            .map_err(OrderError::Storage)?;
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

        // Binance에 살아있는 orderId 집합
        let binance_ids: std::collections::HashSet<i64> = result
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|o| o["orderId"].as_i64())
            .collect();

        let mut db_orders = self
            .repo
            .find_open(symbol)
            .await
            .map_err(OrderError::Storage)?;

        for order in &mut db_orders {
            if let Some(eid) = order.exchange_order_id
                && !binance_ids.contains(&eid)
            {
                order.status = OrderStatus::Expired;
                order.updated_at = Utc::now();
                let _ = self.repo.upsert_order(order).await;
            }
        }

        Ok(db_orders
            .into_iter()
            .filter(|o| matches!(o.status, OrderStatus::New | OrderStatus::PartiallyFilled))
            .collect())
    }
}

impl BinanceOrderExecutor {
    async fn get_order(&self, order_id: Uuid) -> Result<Order, OrderError> {
        self.repo
            .find_by_id(order_id)
            .await
            .map_err(OrderError::Storage)?
            .ok_or_else(|| OrderError::NotFound(order_id.to_string()))
    }
}
