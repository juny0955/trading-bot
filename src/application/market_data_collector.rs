use crate::domain::event::MarketDataEvent;
use crate::ports::collection_use_case::CollectionUseCase;
use crate::ports::fng_provider::FngProvider;
use crate::ports::market_data_provider::MarketDataProvider;
use crate::ports::market_data_sink::MarketDataSink;
use anyhow::Result;
use async_trait::async_trait;
use futures_util::future::join_all;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// 실시간 시장 데이터와 공포탐욕지수를 수집해 저장 sink로 전달하는 유스케이스입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::application::market_data_collector::MarketDataCollector;
///
/// let collector = MarketDataCollector::new();
/// let _ = collector;
/// ```
pub struct MarketDataCollector;

impl Default for MarketDataCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MarketDataCollector {
    /// 새 시장 데이터 수집 유스케이스를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use trading_bot::application::market_data_collector::MarketDataCollector;
    ///
    /// let collector = MarketDataCollector::new();
    /// let _ = collector;
    /// ```
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CollectionUseCase for MarketDataCollector {
    async fn run(
        self: Box<Self>,
        feed: Box<dyn MarketDataProvider>,
        fng: Box<dyn FngProvider>,
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
}
