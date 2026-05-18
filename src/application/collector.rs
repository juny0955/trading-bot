use crate::domain::event::MarketDataEvent;
use crate::ports::fng_feed::FngFeed;
use crate::ports::live_market_feed::LiveMarketFeed;
use crate::ports::market_data_sink::MarketDataSink;
use anyhow::Result;
use futures_util::future::join_all;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub async fn run(
    feed: Box<dyn LiveMarketFeed>,
    fng: Box<dyn FngFeed>,
    sink: Box<dyn MarketDataSink>,
) -> Result<()> {
    let (market_tx, market_rx) = mpsc::channel::<MarketDataEvent>(1000);
    let token = CancellationToken::new();

    let mut handles = vec![
        tokio::spawn(sink.run(market_rx, token.clone())),
        tokio::spawn(fng.run(market_tx.clone(), token.clone())),
        tokio::spawn(feed.run(market_tx, token.clone())),
    ];

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl+C graceful shutdown 시작");
            token.cancel();
        }
        _ = join_all(handles.iter_mut()) => {
            warn!("핸들 조기 종료 - 전체 shutdown");
        }
    }

    token.cancel();
    let _ = join_all(handles).await;
    Ok(())
}
