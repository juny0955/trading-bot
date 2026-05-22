use crate::adapters::binance::api::BinanceOrderApi;
use crate::adapters::binance::signing;
use crate::domain::order::{
    ExchangeOpenOrder, Order, OrderError, OrderRequest, OrderSide, OrderStatus, OrderType,
    TimeInForce,
};
use crate::ports::order_executor::OrderExecutor;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

pub struct BinanceOrderExecutor {
    api: Arc<BinanceOrderApi>,
}

impl BinanceOrderExecutor {
    pub fn new(api: Arc<BinanceOrderApi>) -> Self {
        Self { api }
    }

    fn build_params_str(params: &BTreeMap<&str, Value>) -> String {
        params
            .iter()
            .map(|(k, v)| format!("{}={}", k, value_to_str(v)))
            .collect::<Vec<_>>()
            .join("&")
    }
}

#[async_trait]
impl OrderExecutor for BinanceOrderExecutor {
    async fn submit(&self, request: OrderRequest) -> Result<Order, OrderError> {
        let order = Order::new_from_request(request.clone());
        let id = order.id.to_string();
        let ts = signing::timestamp_ms();
        let side = format!("{:?}", request.side).to_uppercase();
        let order_type = order_type_str(&request.order_type);

        let mut params: BTreeMap<&str, Value> = BTreeMap::new();
        params.insert("apiKey", json!(self.api.api_key));

        if let Some(price) = request.price {
            params.insert("price", json!(price.to_string()));
        }

        params.insert("quantity", json!(request.qty.to_string()));
        params.insert("recvWindow", json!(5000u64));
        params.insert("side", json!(side));

        if request.reduce_only {
            params.insert("reduceOnly", json!("true"));
        }

        if let Some(sp) = request.stop_price {
            params.insert("stopPrice", json!(sp.to_string()));
        }

        params.insert("symbol", json!(request.symbol));

        if request.post_only {
            params.insert("timeInForce", json!("GTX"));
        } else {
            match request.order_type {
                OrderType::Limit | OrderType::StopLimit => {
                    params.insert(
                        "timeInForce",
                        json!(time_in_force_str(&request.time_in_force)),
                    );
                }
                _ => {}
            }
        }

        params.insert("timestamp", json!(ts));
        params.insert("type", json!(order_type));

        let sig = signing::sign(&self.api.secret, &Self::build_params_str(&params));
        params.insert("signature", json!(sig));

        let req = json!({ "id": id, "method": "order.place", "params": params });
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

        let mut params: BTreeMap<&str, Value> = BTreeMap::new();
        params.insert("apiKey", json!(self.api.api_key));
        params.insert("orderId", json!(exchange_id));
        params.insert("symbol", json!(order.symbol));
        params.insert("timestamp", json!(ts));

        let sig = signing::sign(&self.api.secret, &Self::build_params_str(&params));
        params.insert("signature", json!(sig));

        let req = json!({ "id": id, "method": "order.cancel", "params": params });
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

        let mut params: BTreeMap<&str, Value> = BTreeMap::new();
        params.insert("apiKey", json!(self.api.api_key));
        params.insert("orderId", json!(exchange_id));
        params.insert("symbol", json!(order.symbol));
        params.insert("timestamp", json!(ts));

        let sig = signing::sign(&self.api.secret, &Self::build_params_str(&params));
        params.insert("signature", json!(sig));

        let req = json!({ "id": id, "method": "order.status", "params": params });
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

    async fn open_orders(&self, symbol: &str) -> Result<Vec<ExchangeOpenOrder>, OrderError> {
        let id = Uuid::now_v7().to_string();
        let ts = signing::timestamp_ms();

        let mut params: BTreeMap<&str, Value> = BTreeMap::new();
        params.insert("apiKey", json!(self.api.api_key));
        params.insert("symbol", json!(symbol));
        params.insert("timestamp", json!(ts));

        let sig = signing::sign(&self.api.secret, &Self::build_params_str(&params));
        params.insert("signature", json!(sig));

        let req = json!({ "id": id, "method": "openOrders.status", "params": params });
        let result = self.api.send_and_wait(id, req).await?;

        let Some(orders) = result.as_array() else {
            return Ok(Vec::new());
        };

        Ok(orders
            .iter()
            .filter_map(|o| parse_open_order(o, symbol))
            .collect())
    }
}

fn order_type_str(t: &OrderType) -> &'static str {
    match t {
        OrderType::Market => "MARKET",
        OrderType::Limit => "LIMIT",
        OrderType::StopMarket => "STOP_MARKET",
        OrderType::StopLimit => "STOP",
    }
}

fn time_in_force_str(t: &TimeInForce) -> &'static str {
    match t {
        TimeInForce::Gtc => "GTC",
        TimeInForce::Ioc => "IOC",
        TimeInForce::Fok => "FOK",
    }
}

fn value_to_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn parse_open_order(o: &Value, symbol: &str) -> Option<ExchangeOpenOrder> {
    Some(ExchangeOpenOrder {
        exchange_order_id: o["orderId"].as_i64()?,
        client_order_id: o["clientOrderId"].as_str().unwrap_or("").to_string(),
        symbol: symbol.to_string(),
        side: match o["side"].as_str()? {
            "BUY" => OrderSide::Buy,
            "SELL" => OrderSide::Sell,
            _ => return None,
        },
        orig_qty: o["origQty"]
            .as_str()
            .unwrap_or("0")
            .parse()
            .unwrap_or_default(),
        executed_qty: o["executedQty"]
            .as_str()
            .unwrap_or("0")
            .parse()
            .unwrap_or_default(),
        price: o["price"].as_str().and_then(|v| v.parse().ok()),
        status: o["status"].as_str().unwrap_or("UNKNOWN").to_string(),
    })
}
