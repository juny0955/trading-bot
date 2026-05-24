use crate::domain::order::OrderSide;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub available_balance: Decimal,
    pub total_wallet_balance: Decimal,
    pub total_unrealized_pnl: Decimal,
    pub fetched_at: DateTime<Utc>,
}

impl AccountSnapshot {
    pub fn zero(now: DateTime<Utc>) -> Self {
        Self {
            available_balance: Decimal::ZERO,
            total_wallet_balance: Decimal::ZERO,
            total_unrealized_pnl: Decimal::ZERO,
            fetched_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangePositionSnapshot {
    pub symbol: String,
    pub side: OrderSide,
    pub qty: Decimal,
    pub entry_price: Decimal,
    pub leverage: u32,
}

#[derive(Debug, Clone)]
pub struct AccountUpdateEvent {
    pub available_balance: Decimal,
    pub total_wallet_balance: Decimal,
    pub total_unrealized_pnl: Decimal,
    pub event_time: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("계정 API 호출 실패: {0}")]
    Network(String),
    #[error("계정 응답 파싱 실패: {0}")]
    Parse(String),
    #[error("재시도 초과: {0}")]
    Unavailable(String),
}
