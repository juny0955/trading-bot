use crate::domain::order::Fill;
use async_trait::async_trait;

#[async_trait]
pub trait FillSink: Send + Sync {
    async fn emit(&self, fill: Fill);
}
