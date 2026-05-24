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
        }
    }
}

#[async_trait]
pub trait RiskConfigSource: Send + Sync {
    async fn load(&self) -> Result<RiskLimits, RepositoryError>;
}
