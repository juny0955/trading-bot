use crate::domain::exchange_info::SymbolFilters;
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum ExchangeInfoError {
    #[error("거래소 통신 실패: {0}")]
    Network(String),
    #[error("응답 파싱 실패: {0}")]
    Parse(String),
    #[error("재시도 초과: {0}")]
    Unavailable(String),
}

#[async_trait]
pub trait ExchangeInfoSource: Send + Sync {
    /// 활성 심볼 전체의 거래소 필터(MIN_NOTIONAL, LOT_SIZE, PRICE_FILTER 등)를 조회한다.
    async fn fetch_all(&self) -> Result<Vec<SymbolFilters>, ExchangeInfoError>;
}
