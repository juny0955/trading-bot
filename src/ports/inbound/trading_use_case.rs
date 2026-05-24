use crate::domain::strategies::Strategy;
use crate::ports::market_data_provider::MarketDataProvider;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait TradingUseCase: Send {
    async fn run(
        self: Box<Self>,
        feed: Box<dyn MarketDataProvider>,
        strategies: HashMap<(i64, String), Box<dyn Strategy>>,
        token: CancellationToken,
    ) -> Result<()>;
}
