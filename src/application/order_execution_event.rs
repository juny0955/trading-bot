use crate::domain::order::{OrderSide, OrderStatus};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

pub struct OrderExecutionEvent {
    pub exchange_order_id: i64,
    pub fill: Option<FillData>,
    pub new_status: Option<OrderStatus>,
    pub cumulative_filled_qty: Decimal,
    pub avg_fill_price: Option<Decimal>,
}

pub struct FillData {
    pub side: OrderSide,
    pub qty: Decimal,
    pub price: Decimal,
    pub fee: Decimal,
    pub fee_asset: String,
    pub filled_at: DateTime<Utc>,
}
