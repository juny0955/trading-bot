use crate::adapters::outbound::cache::symbol_filters_cache::SymbolFiltersCache;
use crate::ports::outbound::exchange_info_source::ExchangeInfoSource;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// 거래소 심볼 필터를 30분 간격으로 갱신하는 백그라운드 태스크.
pub struct ExchangeInfoRefresher {
    client: Arc<dyn ExchangeInfoSource>,
    cache: Arc<SymbolFiltersCache>,
}

impl ExchangeInfoRefresher {
    /// 새 `ExchangeInfoRefresher`를 생성한다.
    pub fn new(client: Arc<dyn ExchangeInfoSource>, cache: Arc<SymbolFiltersCache>) -> Self {
        Self { client, cache }
    }

    /// 취소 신호까지 30분 간격으로 심볼 필터 캐시를 갱신한다.
    pub async fn run(self, token: CancellationToken) {
        let mut tick = tokio::time::interval(Duration::from_secs(30 * 60));
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = tick.tick() => {
                    match self.cache.refresh(self.client.as_ref()).await {
                        Ok(n) => info!(count=n, "SymbolFiltersCache 갱신 완료"),
                        Err(e) => warn!(error=%e, "SymbolFiltersCache 갱신 실패"),
                    }
                }
            }
        }
    }
}
