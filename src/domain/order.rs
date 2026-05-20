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

#[derive(Debug, thiserror::Error)]
#[error("invalid transition: {from:?} → {to:?}")]
pub struct InvalidTransition {
    pub from: OrderStatus,
    pub to: OrderStatus,
}

impl OrderStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Filled | Self::Cancelled | Self::Rejected | Self::Expired
        )
    }

    pub fn can_transition_to(self, next: OrderStatus) -> Result<(), InvalidTransition> {
        use OrderStatus::*;
        let ok = match (self, next) {
            (New, PartiallyFilled | Filled | Cancelled | Rejected | Expired) => true,
            (PartiallyFilled, PartiallyFilled | Filled | Cancelled | Expired) => true,
            // terminal → terminal 은 거래소가 사후 정정하는 경우만 허용
            (Filled, Cancelled) => true, // 드물지만 가능 (정정)
            _ if self == next => true,   // idempotent
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            Err(InvalidTransition {
                from: self,
                to: next,
            })
        }
    }
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
    #[error("주문 없음: {0}")]
    NotFound(String),
    #[error("증거금 부족")]
    InsufficientMargin,
    #[error("잘못된 요청: {0}")]
    InvalidRequest(String),
    #[error("거래소 거부: code={code} msg={msg}")]
    ExchangeRejected { code: i32, msg: String },
    #[error("연결 실패: {0}")]
    ConnectionLost(String),
    #[error("타임아웃")]
    Timeout,
    #[error("내부 오류: {0:#}")]
    Internal(#[source] anyhow::Error),
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
