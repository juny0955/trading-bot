use crate::domain::order::{ExchangeOpenOrder, Order, OrderError, OrderRequest};
use crate::ports::order_executor::OrderExecutor;
use crate::ports::order_repository::OrderRepository;
use crate::ports::order_service::OrderService;
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

pub struct LiveOrderService {
    executor: Arc<dyn OrderExecutor>,
    repo: Arc<dyn OrderRepository>,
}

impl LiveOrderService {
    pub fn new(executor: Arc<dyn OrderExecutor>, repo: Arc<dyn OrderRepository>) -> Self {
        Self { executor, repo }
    }
}

#[async_trait]
impl OrderService for LiveOrderService {
    async fn submit(&self, request: OrderRequest) -> Result<Order, OrderError> {
        let order = self.executor.submit(request).await?;
        self.repo.upsert_order(&order).await?;
        Ok(order)
    }

    async fn cancel(&self, order_id: Uuid) -> Result<Order, OrderError> {
        let order = self
            .repo
            .find_by_id(order_id)
            .await?
            .ok_or_else(|| OrderError::NotFound(order_id.to_string()))?;

        let updated = self.executor.cancel(&order).await?;
        self.repo.upsert_order(&updated).await?;
        Ok(updated)
    }

    async fn sync_open_orders(&self, symbol: &str) -> Result<Vec<ExchangeOpenOrder>, OrderError> {
        let orders = self.executor.open_orders(symbol).await?;
        Ok(orders)
    }
}
