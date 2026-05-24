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

/// Binance WebSocket API를 통해 주문을 실행하는 어댑터입니다.
///
/// # Examples
///
/// ```ignore
/// # use std::sync::Arc;
/// # use trading_bot::adapters::binance::api::BinanceOrderApi;
/// # use trading_bot::adapters::outbound::binance::order::executor::BinanceOrderExecutor;
/// # fn example(api: Arc<BinanceOrderApi>) {
/// let executor = BinanceOrderExecutor::new(api);
/// # }
/// ```
pub struct BinanceOrderExecutor {
    api: Arc<BinanceOrderApi>,
}

impl BinanceOrderExecutor {
    /// 새 Binance 주문 실행 어댑터를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use std::sync::Arc;
    /// # use trading_bot::adapters::binance::api::BinanceOrderApi;
    /// # use trading_bot::adapters::outbound::binance::order::executor::BinanceOrderExecutor;
    /// # fn example(api: Arc<BinanceOrderApi>) {
    /// let executor = BinanceOrderExecutor::new(api);
    /// # }
    /// ```
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
        let order = Order::new_from_request(request.clone())?;
        let id = order.id.to_string();
        let ts = timestamp_ms()?;
        let params = signed_params(
            submit_order_params(&self.api.api_key, &request, ts),
            &self.api.secret,
        )?;
        let req = ws_request(&id, "order.place", params);
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
        let ts = timestamp_ms()?;
        let params = signed_params(
            order_lookup_params(&self.api.api_key, &order.symbol, exchange_id, ts),
            &self.api.secret,
        )?;
        let req = ws_request(&id, "order.cancel", params);
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
        let ts = timestamp_ms()?;
        let params = signed_params(
            order_lookup_params(&self.api.api_key, &order.symbol, exchange_id, ts),
            &self.api.secret,
        )?;
        let req = ws_request(&id, "order.status", params);
        let result = self.api.send_and_wait(id, req).await?;

        let mut updated = order.clone();
        updated.status = parse_order_status(result["status"].as_str()).unwrap_or(updated.status);
        updated.updated_at = Utc::now();
        Ok(updated)
    }

    async fn open_orders(&self, symbol: &str) -> Result<Vec<ExchangeOpenOrder>, OrderError> {
        let id = Uuid::now_v7().to_string();
        let ts = timestamp_ms()?;
        let params = signed_params(
            open_orders_params(&self.api.api_key, symbol, ts),
            &self.api.secret,
        )?;
        let req = ws_request(&id, "openOrders.status", params);
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

fn timestamp_ms() -> Result<u64, OrderError> {
    signing::timestamp_ms().map_err(|e| OrderError::Internal(anyhow::anyhow!(e)))
}

fn signed_params<'a>(
    mut params: BTreeMap<&'a str, Value>,
    secret: &str,
) -> Result<BTreeMap<&'a str, Value>, OrderError> {
    let sig = signing::sign(secret, &BinanceOrderExecutor::build_params_str(&params))
        .map_err(|e| OrderError::Internal(anyhow::anyhow!(e)))?;
    params.insert("signature", json!(sig));
    Ok(params)
}

fn ws_request(id: &str, method: &str, params: BTreeMap<&str, Value>) -> Value {
    json!({ "id": id, "method": method, "params": params })
}

fn submit_order_params<'a>(
    api_key: &'a str,
    request: &'a OrderRequest,
    ts: u64,
) -> BTreeMap<&'a str, Value> {
    let mut params = BTreeMap::new();
    params.insert("apiKey", json!(api_key));
    params.insert("quantity", json!(request.qty.to_string()));
    params.insert("recvWindow", json!(5000u64));
    params.insert("side", json!(order_side_str(request.side)));
    params.insert("symbol", json!(request.symbol));
    params.insert("timestamp", json!(ts));
    params.insert("type", json!(order_type_str(&request.order_type)));

    if let Some(price) = request.price {
        params.insert("price", json!(price.to_string()));
    }
    if request.reduce_only {
        params.insert("reduceOnly", json!("true"));
    }
    if let Some(stop_price) = request.stop_price {
        params.insert("stopPrice", json!(stop_price.to_string()));
    }
    if let Some(time_in_force) = order_time_in_force(request) {
        params.insert("timeInForce", json!(time_in_force));
    }
    params
}

fn order_lookup_params<'a>(
    api_key: &'a str,
    symbol: &'a str,
    exchange_id: i64,
    ts: u64,
) -> BTreeMap<&'a str, Value> {
    let mut params = BTreeMap::new();
    params.insert("apiKey", json!(api_key));
    params.insert("orderId", json!(exchange_id));
    params.insert("symbol", json!(symbol));
    params.insert("timestamp", json!(ts));
    params
}

fn open_orders_params<'a>(api_key: &'a str, symbol: &'a str, ts: u64) -> BTreeMap<&'a str, Value> {
    let mut params = BTreeMap::new();
    params.insert("apiKey", json!(api_key));
    params.insert("symbol", json!(symbol));
    params.insert("timestamp", json!(ts));
    params
}

fn order_time_in_force(request: &OrderRequest) -> Option<&'static str> {
    if request.post_only {
        return Some("GTX");
    }
    match request.order_type {
        OrderType::Limit | OrderType::StopLimit => Some(time_in_force_str(&request.time_in_force)),
        _ => None,
    }
}

fn order_side_str(side: OrderSide) -> &'static str {
    match side {
        OrderSide::Buy => "BUY",
        OrderSide::Sell => "SELL",
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

fn parse_order_status(status: Option<&str>) -> Option<OrderStatus> {
    match status {
        Some("FILLED") => Some(OrderStatus::Filled),
        Some("CANCELED") => Some(OrderStatus::Cancelled),
        Some("PARTIALLY_FILLED") => Some(OrderStatus::PartiallyFilled),
        Some("EXPIRED") => Some(OrderStatus::Expired),
        Some("REJECTED") => Some(OrderStatus::Rejected),
        _ => None,
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
        client_order_id: o["clientOrderId"].as_str().unwrap_or_default().to_string(),
        symbol: symbol.to_string(),
        side: match o["side"].as_str()? {
            "BUY" => OrderSide::Buy,
            "SELL" => OrderSide::Sell,
            _ => return None,
        },
        orig_qty: decimal_field(o, "origQty").unwrap_or_default(),
        executed_qty: decimal_field(o, "executedQty").unwrap_or_default(),
        price: o["price"].as_str().and_then(|v| v.parse().ok()),
        status: o["status"].as_str().unwrap_or("UNKNOWN").to_string(),
    })
}

fn decimal_field(o: &Value, field: &str) -> Option<rust_decimal::Decimal> {
    o[field].as_str()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use rust_decimal_macros::dec;

    #[test]
    fn submit_order_params_include_limit_tif() -> anyhow::Result<()> {
        let request = OrderRequest {
            symbol: "BTCUSDT".into(),
            order_type: OrderType::Limit,
            side: OrderSide::Buy,
            qty: dec!(1),
            price: Some(dec!(100)),
            ..Default::default()
        };

        let params = submit_order_params("key", &request, 10);
        assert_eq!(params["side"], json!("BUY"));
        assert_eq!(params["timeInForce"], json!("GTC"));
        assert!(BinanceOrderExecutor::build_params_str(&params).contains("symbol=BTCUSDT"));
        Ok(())
    }

    #[test]
    fn parse_open_order_maps_business_fields() -> anyhow::Result<()> {
        let raw = json!({
            "orderId": 7,
            "clientOrderId": "client-1",
            "side": "SELL",
            "origQty": "2.5",
            "executedQty": "1.0",
            "price": "100.5",
            "status": "NEW"
        });

        let order = parse_open_order(&raw, "ETHUSDT").context("valid open order")?;
        assert_eq!(order.side, OrderSide::Sell);
        assert_eq!(order.orig_qty, dec!(2.5));
        assert_eq!(order.executed_qty, dec!(1.0));
        Ok(())
    }
}
