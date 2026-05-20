use crate::ports::fng_provider::FngProvider;
use crate::ports::market_data_provider::MarketDataProvider;
use crate::ports::market_data_sink::MarketDataSink;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait CollectionUseCase: Send {
    async fn run(
        self: Box<Self>,
        feed: Box<dyn MarketDataProvider>,
        fng: Box<dyn FngProvider>,
        sink: Box<dyn MarketDataSink>,
    ) -> Result<()>;
}
