use crate::domain::order::{ExchangeOpenOrder, Order, OrderError, OrderRequest};
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait OrderService: Send + Sync {
    async fn submit(&self, request: OrderRequest) -> Result<Order, OrderError>;
    async fn cancel(&self, order_id: Uuid) -> Result<Order, OrderError>;
    async fn sync_open_orders(&self, symbol: &str) -> Result<Vec<ExchangeOpenOrder>, OrderError>;
}
