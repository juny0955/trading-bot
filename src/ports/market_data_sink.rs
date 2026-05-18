use crate::domain::event::MarketDataEvent;
use async_trait::async_trait;
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait MarketDataSink: Send + Sync {
    async fn run(self: Box<Self>, rx: Receiver<MarketDataEvent>, token: CancellationToken);
}
