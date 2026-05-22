use crate::domain::order::{ExchangeOpenOrder, Order, OrderError, OrderRequest};
use async_trait::async_trait;

#[async_trait]
pub trait OrderExecutor: Send + Sync {
    async fn submit(&self, request: OrderRequest) -> Result<Order, OrderError>;
    async fn cancel(&self, order: &Order) -> Result<Order, OrderError>;
    async fn query(&self, order: &Order) -> Result<Order, OrderError>;
    async fn open_orders(&self, symbol: &str) -> Result<Vec<ExchangeOpenOrder>, OrderError>;
}
