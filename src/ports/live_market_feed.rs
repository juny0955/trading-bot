use crate::domain::event::MarketDataEvent;
use async_trait::async_trait;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait LiveMarketFeed: Send + Sync {
    async fn run(self: Box<Self>, tx: Sender<MarketDataEvent>, token: CancellationToken);
}
