use crate::domain::order::{Fill, Order, OrderError};
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait OrderRepository: Send + Sync {
    async fn upsert_order(&self, order: &Order) -> Result<(), OrderError>;
    async fn record_fill(&self, fill: &Fill) -> Result<(), OrderError>;
    async fn find_open(&self, symbol: &str) -> Result<Vec<Order>, OrderError>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, OrderError>;
    async fn find_by_exchange_id(&self, exchange_id: i64) -> Result<Option<Order>, OrderError>;
}
