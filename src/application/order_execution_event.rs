use crate::adapters::binance::dto_account::BinanceOrderTradeUpdate;
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

impl BinanceOrderTradeUpdate {
    pub fn into_execution_event(self) -> OrderExecutionEvent {
        use chrono::TimeZone;
        let fill = if self.execution_type == "TRADE" {
            let side = if self.side == "BUY" {
                OrderSide::Buy
            } else {
                OrderSide::Sell
            };
            Some(FillData {
                side,
                qty: self.last_filled_qty,
                price: self.last_fill_price,
                fee: self.fee,
                fee_asset: self.fee_asset,
                filled_at: Utc
                    .timestamp_millis_opt(self.trade_time_ms)
                    .single()
                    .unwrap_or_else(Utc::now),
            })
        } else {
            None
        };

        let new_status = match self.status.as_str() {
            "FILLED" => Some(OrderStatus::Filled),
            "PARTIALLY_FILLED" => Some(OrderStatus::PartiallyFilled),
            "CANCELED" => Some(OrderStatus::Cancelled),
            "EXPIRED" => Some(OrderStatus::Expired),
            "REJECTED" => Some(OrderStatus::Rejected),
            _ => None,
        };

        let avg_fill_price = if self.avg_fill_price.is_zero() {
            None
        } else {
            Some(self.avg_fill_price)
        };

        OrderExecutionEvent {
            exchange_order_id: self.exchange_order_id,
            fill,
            new_status,
            cumulative_filled_qty: self.cumulative_filled_qty,
            avg_fill_price,
        }
    }
}
