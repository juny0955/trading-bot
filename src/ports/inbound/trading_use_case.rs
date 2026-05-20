use crate::domain::backtest::Position;
use crate::domain::strategies::Strategy;
use crate::ports::market_data_provider::MarketDataProvider;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait TradingUseCase: Send {
    async fn run(
        self: Box<Self>,
        feed: Box<dyn MarketDataProvider>,
        strategy: Box<dyn Strategy>,
        position: Arc<Mutex<Position>>,
        token: CancellationToken,
    ) -> Result<()>;
}
