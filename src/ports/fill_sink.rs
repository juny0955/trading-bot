use crate::domain::order::Fill;
use async_trait::async_trait;
use tokio::sync::mpsc::Sender;

#[async_trait]
pub trait FillSink: Send + Sync {
    async fn emit(&self, fill: Fill);
}

#[async_trait]
impl FillSink for Sender<Fill> {
    async fn emit(&self, fill: Fill) {
        let _ = self.send(fill).await;
    }
}
