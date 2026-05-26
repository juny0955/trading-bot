use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 주문 실행 방식입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::domain::order::OrderType;
///
/// let order_type = OrderType::Market;
/// assert_eq!(format!("{order_type:?}"), "Market");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OrderType {
    #[default]
    Market,
    Limit,
    StopMarket,
    StopLimit,
}

/// 주문 방향입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::domain::order::OrderSide;
///
/// let side = OrderSide::Buy;
/// assert_eq!(side, OrderSide::Buy);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OrderSide {
    #[default]
    Buy,
    Sell,
}

/// 주문의 수명주기 상태입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::domain::order::OrderStatus;
///
/// assert!(OrderStatus::Filled.is_terminal());
/// assert!(!OrderStatus::New.is_terminal());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    /// Outbox 단계: DB에 기록되었으나 거래소 응답 미수신.
    /// 모호한 송신 실패(Timeout/Connection) 시 이 상태로 유지되며
    /// Reconciler가 query_by_client_id로 실제 상태를 확정한다.
    PendingSubmit,
    New,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    Expired,
}

/// 허용되지 않는 주문 상태 전이를 나타냅니다.
///
/// # Examples
///
/// ```
/// use trading_bot::domain::order::OrderStatus;
///
/// let err = OrderStatus::Filled
///     .can_transition_to(OrderStatus::New)
///     .unwrap_err();
/// assert_eq!(err.from, OrderStatus::Filled);
/// ```
#[derive(Debug, thiserror::Error)]
#[error("invalid transition: {from:?} → {to:?}")]
pub struct InvalidTransition {
    pub from: OrderStatus,
    pub to: OrderStatus,
}

impl OrderStatus {
    /// 더 이상 일반적인 주문 진행이 없는 최종 상태인지 반환합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use trading_bot::domain::order::OrderStatus;
    ///
    /// assert!(OrderStatus::Cancelled.is_terminal());
    /// ```
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Filled | Self::Cancelled | Self::Rejected | Self::Expired
        )
    }

    /// 현재 상태에서 다음 상태로 전이할 수 있는지 검증합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use trading_bot::domain::order::OrderStatus;
    ///
    /// assert!(OrderStatus::New
    ///     .can_transition_to(OrderStatus::PartiallyFilled)
    ///     .is_ok());
    /// ```
    pub fn can_transition_to(self, next: OrderStatus) -> Result<(), InvalidTransition> {
        use OrderStatus::*;
        let ok = match (self, next) {
            // PendingSubmit은 모호한 송신 실패 후 Reconciler가 실제 상태를 확정한다.
            (PendingSubmit, New | PartiallyFilled | Filled | Cancelled | Rejected | Expired) => {
                true
            }
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

/// 지정가 주문의 체결 조건입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::domain::order::TimeInForce;
///
/// let tif = TimeInForce::Gtc;
/// assert_eq!(format!("{tif:?}"), "Gtc");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TimeInForce {
    #[default]
    Gtc,
    Ioc,
    Fok,
}

/// 시스템 내부에서 추적하는 주문입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::order::{Order, OrderRequest, OrderSide, OrderType};
///
/// let order = Order::new_from_request(OrderRequest {
///     symbol: "BTCUSDT".into(),
///     order_type: OrderType::Market,
///     side: OrderSide::Buy,
///     qty: dec!(1),
///     ..Default::default()
/// }).unwrap();
///
/// assert_eq!(order.symbol, "BTCUSDT");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: Uuid,
    pub client_order_id: String,
    pub exchange_order_id: Option<i64>,
    pub strategy_instance_id: i64,
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

/// 체결 내역입니다.
///
/// # Examples
///
/// ```
/// use chrono::Utc;
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::order::{Fill, OrderSide};
/// use uuid::Uuid;
///
/// let fill = Fill {
///     order_id: Uuid::nil(),
///     strategy_instance_id: 1,
///     symbol: "BTCUSDT".into(),
///     side: OrderSide::Buy,
///     qty: dec!(1),
///     price: dec!(100),
///     fee: dec!(0.1),
///     fee_asset: "USDT".into(),
///     filled_at: Utc::now(),
///     exchange_trade_id: 42,
///     position_lifecycle_id: None,
/// };
/// assert_eq!(fill.qty, dec!(1));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
    pub order_id: Uuid,
    pub strategy_instance_id: i64,
    pub symbol: String,
    pub side: OrderSide,
    pub qty: Decimal,
    pub price: Decimal,
    pub fee: Decimal,
    pub fee_asset: String,
    pub filled_at: DateTime<Utc>,
    /// Binance trade ID (`t` field). -1 이면 non-TRADE 이벤트.
    pub exchange_trade_id: i64,
    /// ML 데이터 귀속용. Some이면 fills 테이블 기록 완료, None이면 unresolved_fills 격리.
    pub position_lifecycle_id: Option<Uuid>,
}

/// 새 주문을 만들기 위한 요청 값입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::order::{OrderRequest, OrderSide, OrderType};
///
/// let request = OrderRequest {
///     symbol: "BTCUSDT".into(),
///     order_type: OrderType::Market,
///     side: OrderSide::Sell,
///     qty: dec!(0.5),
///     ..Default::default()
/// };
/// assert_eq!(request.symbol, "BTCUSDT");
/// ```
#[derive(Debug, Clone, Default)]
pub struct OrderRequest {
    pub strategy_instance_id: i64,
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

/// 거래소에 열려 있는 주문을 표현하는 조회 결과입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::order::{ExchangeOpenOrder, OrderSide};
///
/// let open = ExchangeOpenOrder {
///     exchange_order_id: 1,
///     client_order_id: "client-1".into(),
///     symbol: "BTCUSDT".into(),
///     side: OrderSide::Buy,
///     orig_qty: dec!(1),
///     executed_qty: dec!(0),
///     price: None,
///     status: "NEW".into(),
/// };
/// assert_eq!(open.exchange_order_id, 1);
/// ```
#[derive(Debug, Clone)]
pub struct ExchangeOpenOrder {
    pub exchange_order_id: i64,
    pub client_order_id: String,
    pub symbol: String,
    pub side: OrderSide,
    pub orig_qty: Decimal,
    pub executed_qty: Decimal,
    pub price: Option<Decimal>,
    pub status: String,
}

/// 주문 처리 중 발생할 수 있는 오류입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::domain::order::OrderError;
///
/// let err = OrderError::InvalidRequest("symbol is required".into());
/// assert!(err.to_string().contains("잘못된 요청"));
/// ```
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
    /// 주문 요청을 검증하고 새 주문을 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::domain::order::{Order, OrderRequest, OrderSide, OrderType};
    ///
    /// let order = Order::new_from_request(OrderRequest {
    ///     symbol: "ETHUSDT".into(),
    ///     order_type: OrderType::Market,
    ///     side: OrderSide::Buy,
    ///     qty: dec!(2),
    ///     ..Default::default()
    /// }).unwrap();
    ///
    /// assert_eq!(order.filled_qty, dec!(0));
    /// ```
    pub fn new_from_request(req: OrderRequest) -> Result<Self, OrderError> {
        validate_request(&req)?;
        let now = Utc::now();
        let id = Uuid::now_v7();
        let client_order_id = req.client_order_id.unwrap_or_else(|| id.to_string());

        Ok(Self {
            id,
            client_order_id,
            exchange_order_id: None,
            strategy_instance_id: req.strategy_instance_id,
            symbol: req.symbol.trim().to_string(),
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
        })
    }
}

fn validate_request(req: &OrderRequest) -> Result<(), OrderError> {
    validate_symbol(&req.symbol)?;
    validate_positive("qty", req.qty)?;
    validate_order_type_prices(req)
}

fn validate_symbol(symbol: &str) -> Result<(), OrderError> {
    if symbol.trim().is_empty() {
        return Err(OrderError::InvalidRequest("symbol is required".into()));
    }
    Ok(())
}

fn validate_order_type_prices(req: &OrderRequest) -> Result<(), OrderError> {
    match req.order_type {
        OrderType::Market => Ok(()),
        OrderType::Limit => validate_required_positive("price", req.price),
        OrderType::StopMarket => validate_required_positive("stop_price", req.stop_price),
        OrderType::StopLimit => {
            validate_required_positive("price", req.price)?;
            validate_required_positive("stop_price", req.stop_price)
        }
    }
}

fn validate_required_positive(name: &str, value: Option<Decimal>) -> Result<(), OrderError> {
    let value = value.ok_or_else(|| OrderError::InvalidRequest(format!("{name} is required")))?;
    validate_positive(name, value)
}

fn validate_positive(name: &str, value: Decimal) -> Result<(), OrderError> {
    if value <= Decimal::ZERO {
        return Err(OrderError::InvalidRequest(format!(
            "{name} must be positive"
        )));
    }
    Ok(())
}

impl TryFrom<&Order> for ExchangeOpenOrder {
    type Error = OrderError;

    fn try_from(o: &Order) -> Result<Self, Self::Error> {
        Ok(Self {
            exchange_order_id: o
                .exchange_order_id
                .ok_or_else(|| OrderError::InvalidRequest("exchange_order_id 없음".into()))?,
            client_order_id: o.client_order_id.clone(),
            symbol: o.symbol.clone(),
            side: o.side,
            orig_qty: o.qty,
            executed_qty: o.filled_qty,
            price: o.price,
            status: format!("{:?}", o.status),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use rust_decimal_macros::dec;

    fn market_request() -> OrderRequest {
        OrderRequest {
            symbol: "BTCUSDT".into(),
            order_type: OrderType::Market,
            side: OrderSide::Buy,
            qty: dec!(1),
            ..Default::default()
        }
    }

    #[test]
    fn new_from_request_rejects_invalid_business_inputs() -> Result<()> {
        let invalid_requests = [
            OrderRequest {
                symbol: " ".into(),
                ..market_request()
            },
            OrderRequest {
                qty: Decimal::ZERO,
                ..market_request()
            },
            OrderRequest {
                order_type: OrderType::Limit,
                price: None,
                ..market_request()
            },
        ];

        assert!(invalid_requests.into_iter().all(|req| matches!(
            Order::new_from_request(req),
            Err(OrderError::InvalidRequest(_))
        )));
        Ok(())
    }

    #[test]
    fn pending_submit_can_transition_to_active_and_terminal_states() {
        use OrderStatus::*;
        for next in [New, PartiallyFilled, Filled, Cancelled, Rejected, Expired] {
            assert!(
                PendingSubmit.can_transition_to(next).is_ok(),
                "PendingSubmit -> {next:?} should be allowed"
            );
        }
    }

    #[test]
    fn active_states_cannot_transition_back_to_pending_submit() {
        use OrderStatus::*;
        for from in [New, PartiallyFilled, Filled, Cancelled] {
            assert!(
                from.can_transition_to(PendingSubmit).is_err(),
                "{from:?} -> PendingSubmit should be rejected"
            );
        }
    }
}
