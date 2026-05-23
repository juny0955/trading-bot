use crate::domain::strategies::StrategyInstance;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait StrategyRepository: Send + Sync {
    async fn load_enabled(&self) -> Result<Vec<StrategyInstance>>;
}
