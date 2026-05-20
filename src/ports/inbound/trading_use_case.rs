use crate::domain::backtest::Position;
use crate::domain::strategies::Strategy;
use crate::ports::live_market_feed::LiveMarketFeed;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait TradingUseCase: Send {
    async fn run(
        self: Box<Self>,
        feed: Box<dyn LiveMarketFeed>,
        strategy: Box<dyn Strategy>,
        position: Arc<Mutex<Position>>,
        token: CancellationToken,
    ) -> Result<()>;
}
