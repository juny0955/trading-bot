use crate::config::AppConfig;
use async_trait::async_trait;

#[async_trait]
pub trait ConfigRepository: Send + Sync {
    async fn load(&self) -> anyhow::Result<AppConfig>;
}
