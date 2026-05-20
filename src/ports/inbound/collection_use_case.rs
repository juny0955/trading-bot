use crate::ports::fng_feed::FngFeed;
use crate::ports::live_market_feed::LiveMarketFeed;
use crate::ports::market_data_sink::MarketDataSink;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait CollectionUseCase: Send {
    async fn run(
        self: Box<Self>,
        feed: Box<dyn LiveMarketFeed>,
        fng: Box<dyn FngFeed>,
        sink: Box<dyn MarketDataSink>,
    ) -> Result<()>;
}
