use crate::domain::order::{Fill, Order, OrderError, OrderStatus};
use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Order Not Found: {0}")]
    NotFound(String),
    #[error("Storage Error: {0}")]
    Storage(String),
}

impl From<RepositoryError> for OrderError {
    fn from(value: RepositoryError) -> Self {
        match value {
            RepositoryError::NotFound(s) => OrderError::NotFound(s),
            RepositoryError::Storage(s) => OrderError::NotFound(s),
        }
    }
}

#[async_trait]
pub trait OrderRepository: Send + Sync {
    async fn upsert_order(&self, order: &Order) -> Result<(), RepositoryError>;
    async fn record_fill(&self, fill: &Fill) -> Result<(), RepositoryError>;
    async fn find_open(&self, symbol: &str) -> Result<Vec<Order>, RepositoryError>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, RepositoryError>;
    async fn find_by_exchange_id(&self, exchange_id: i64)
    -> Result<Option<Order>, RepositoryError>;
    async fn find_by_status(&self, status: OrderStatus) -> Result<Vec<Order>, RepositoryError>;
    /// DB에 없는 ORDER_TRADE_UPDATE를 orphan_orders 테이블에 기록한다.
    async fn record_orphan(&self, exchange_order_id: i64, raw: Value)
    -> Result<(), RepositoryError>;
}
