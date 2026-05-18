use crate::domain::backtest::{Bar, DepthSnapshot};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait MarketDataSource: Send + Sync {
    async fn fetch_bars(&self, q: BarQuery) -> Result<Vec<Bar>>;
    async fn fetch_depth_snapshots(&self, q: DepthQuery) -> Result<Vec<DepthSnapshot>>;
}

pub struct BarQuery {
    pub symbol: String,
    pub from_ns: i64,
    pub to_ns: i64,
    pub interval: String, // "1m", "5m" 등 QuestDB SAMPLE BY 문법
}

pub struct DepthQuery {
    pub symbol: String,
    pub from_ns: i64,
    pub to_ns: i64,
    pub every: String, // "5s" 등 샘플링 주기
}
