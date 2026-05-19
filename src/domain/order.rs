use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OrderType {
    #[default]
    Market,
    Limit,
    StopMarket,
    StopLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OrderSide {
    #[default]
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimeInForce {
    #[default]
    Gtc,
    Ioc,
    Fok,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: Uuid,
    pub client_order_id: String,
    pub exchange_order_id: Option<i64>,
    pub symbol: String,
    pub order_type: OrderType,
    pub side: OrderSide,
    pub status: OrderStatus,
    pub qty: Decimal,
    pub price: Option<Decimal>,
    pub stop_price: Option<Decimal>,
    pub filled_qty: Decimal,
    pub avg_fill_price: Option<Decimal>,
    pub time_in_force: TimeInForce,
    pub reduce_only: bool,
    pub post_only: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub order_id: Uuid,
    pub symbol: String,
    pub side: OrderSide,
    pub qty: Decimal,
    pub price: Decimal,
    pub fee: Decimal,
    pub fee_asset: String,
    pub filled_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct OrderRequest {
    pub symbol: String,
    pub order_type: OrderType,
    pub side: OrderSide,
    pub qty: Decimal,
    pub price: Option<Decimal>,
    pub stop_price: Option<Decimal>,
    pub client_order_id: Option<String>,
    pub time_in_force: TimeInForce,
    pub reduce_only: bool,
    pub post_only: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum OrderError {
    #[error("Order Not Found: {0}")]
    NotFound(String),
    #[error("Insufficient Margin")]
    InsufficientMargin,
    #[error("Exchange Rejected: {code} {msg}")]
    ExchangeRejected { code: i32, msg: String },
    #[error("Storage Error: {0}")]
    Storage(String),
    #[error("Connection Error: {0}")]
    Connection(String),
}

impl Order {
    pub fn new_from_request(req: OrderRequest) -> Self {
        let now = Utc::now();
        let id = Uuid::now_v7();
        let client_order_id = req.client_order_id.unwrap_or_else(|| id.to_string());

        Self {
            id,
            client_order_id,
            exchange_order_id: None,
            symbol: req.symbol,
            order_type: req.order_type,
            side: req.side,
            status: OrderStatus::New,
            qty: req.qty,
            price: req.price,
            stop_price: req.stop_price,
            filled_qty: Decimal::ZERO,
            avg_fill_price: None,
            time_in_force: req.time_in_force,
            reduce_only: req.reduce_only,
            post_only: req.post_only,
            created_at: now,
            updated_at: now,
        }
    }
}
