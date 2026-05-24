use crate::domain::order::{OrderSide, OrderStatus};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

/// 거래소 주문 실행 이벤트를 애플리케이션 내부 형식으로 표현합니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::application::order_execution_event::OrderExecutionEvent;
/// use trading_bot::domain::order::OrderStatus;
///
/// let event = OrderExecutionEvent {
///     exchange_order_id: 1,
///     fill: None,
///     new_status: Some(OrderStatus::Filled),
///     cumulative_filled_qty: dec!(1),
///     avg_fill_price: Some(dec!(100)),
/// };
/// assert_eq!(event.exchange_order_id, 1);
/// ```
pub struct OrderExecutionEvent {
    pub exchange_order_id: i64,
    pub fill: Option<FillData>,
    pub new_status: Option<OrderStatus>,
    pub cumulative_filled_qty: Decimal,
    pub avg_fill_price: Option<Decimal>,
}

/// 거래소 체결 이벤트의 체결 상세입니다.
///
/// # Examples
///
/// ```
/// use chrono::Utc;
/// use rust_decimal_macros::dec;
/// use trading_bot::application::order_execution_event::FillData;
/// use trading_bot::domain::order::OrderSide;
///
/// let fill = FillData {
///     side: OrderSide::Buy,
///     qty: dec!(1),
///     price: dec!(100),
///     fee: dec!(0.01),
///     fee_asset: "USDT".into(),
///     filled_at: Utc::now(),
/// };
/// assert_eq!(fill.side, OrderSide::Buy);
/// ```
pub struct FillData {
    pub side: OrderSide,
    pub qty: Decimal,
    pub price: Decimal,
    pub fee: Decimal,
    pub fee_asset: String,
    pub filled_at: DateTime<Utc>,
}
