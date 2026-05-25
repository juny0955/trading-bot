use crate::domain::order::OrderSide;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// REST API로 조회한 계정 잔고 스냅샷입니다.
///
/// # Examples
///
/// ```
/// use chrono::Utc;
/// use trading_bot::domain::account::AccountSnapshot;
///
/// let snap = AccountSnapshot::zero(Utc::now());
/// assert_eq!(snap.available_balance, rust_decimal::Decimal::ZERO);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub available_balance: Decimal,
    pub total_wallet_balance: Decimal,
    pub total_unrealized_pnl: Decimal,
    pub fetched_at: DateTime<Utc>,
}

impl AccountSnapshot {
    /// 모든 잔고가 0인 초기 스냅샷을 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use chrono::Utc;
    /// use trading_bot::domain::account::AccountSnapshot;
    ///
    /// let snap = AccountSnapshot::zero(Utc::now());
    /// assert!(snap.total_wallet_balance.is_zero());
    /// ```
    pub fn zero(now: DateTime<Utc>) -> Self {
        Self {
            available_balance: Decimal::ZERO,
            total_wallet_balance: Decimal::ZERO,
            total_unrealized_pnl: Decimal::ZERO,
            fetched_at: now,
        }
    }
}

/// 거래소에서 조회한 단일 심볼의 포지션 상태입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::order::OrderSide;
/// use trading_bot::domain::account::ExchangePositionSnapshot;
///
/// let snap = ExchangePositionSnapshot {
///     symbol: "BTCUSDT".into(),
///     side: OrderSide::Buy,
///     qty: dec!(0.1),
///     entry_price: dec!(42000),
///     leverage: 10,
/// };
/// assert_eq!(snap.leverage, 10);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangePositionSnapshot {
    pub symbol: String,
    pub side: OrderSide,
    pub qty: Decimal,
    pub entry_price: Decimal,
    pub leverage: u32,
}

/// USER_DATA_STREAM에서 수신한 계정 잔고 업데이트 이벤트입니다.
#[derive(Debug, Clone)]
pub struct AccountUpdateEvent {
    pub available_balance: Decimal,
    pub total_wallet_balance: Decimal,
    pub total_unrealized_pnl: Decimal,
    pub event_time: DateTime<Utc>,
}

/// 계정 API 호출 시 발생할 수 있는 오류입니다.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("계정 API 호출 실패: {0}")]
    Network(String),
    #[error("계정 응답 파싱 실패: {0}")]
    Parse(String),
    #[error("재시도 초과: {0}")]
    Unavailable(String),
    #[error("요청 초과 (429)")]
    RateLimited,
    #[error("IP 밴 (418)")]
    IpBanned,
}
