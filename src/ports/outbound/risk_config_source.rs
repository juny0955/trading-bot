use crate::ports::order_repository::RepositoryError;
use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[derive(Debug, Clone)]
pub struct RiskLimits {
    pub max_leverage: u32,
    pub max_open_orders_per_symbol: u32,
    pub max_position_notional: Decimal,
    pub order_ttl_sec: u64,
    pub min_available_balance: Decimal,
    pub slippage_buffer_bps: u32,

    /// CB: 연속 거래소 실패 임계치(글로벌). 0이면 비활성.
    pub cb_consecutive_failure_threshold: u32,
    /// CB: 실패 카운트 슬라이딩 윈도우(초).
    pub cb_failure_window_sec: u64,
    /// CB: trip 후 자동 해제까지 대기 시간(초).
    pub cb_open_duration_sec: u64,
    /// CB: 잔고 드로다운 % (예: 5.0 = 세션 고점 대비 5% 하락). 0이면 비활성.
    pub cb_drawdown_pct: Decimal,
    /// CB: 드로다운 추적 lookback(초). 이 시간 동안의 최고 equity를 기준.
    pub cb_drawdown_lookback_sec: u64,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            max_leverage: 5,
            max_open_orders_per_symbol: 10,
            max_position_notional: dec!(1000),
            order_ttl_sec: 300,
            min_available_balance: dec!(50),
            slippage_buffer_bps: 100,
            cb_consecutive_failure_threshold: 5,
            cb_failure_window_sec: 60,
            cb_open_duration_sec: 120,
            cb_drawdown_pct: dec!(5),
            cb_drawdown_lookback_sec: 3600,
        }
    }
}

#[async_trait]
pub trait RiskConfigSource: Send + Sync {
    async fn load(&self) -> Result<RiskLimits, RepositoryError>;
}
