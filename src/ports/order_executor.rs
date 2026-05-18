use crate::domain::order::{Order, OrderError, OrderRequest};
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait OrderExecutor: Send + Sync {
    async fn submit(&self, request: OrderRequest) -> Result<Order, OrderError>;
    async fn cancel(&self, order_id: Uuid) -> Result<Order, OrderError>;
    async fn query(&self, order_id: Uuid) -> Result<Order, OrderError>;
    async fn open_orders(&self, symbol: &str) -> Result<Vec<Order>, OrderError>;
}
