use crate::domain::order::{ExchangeOpenOrder, Order, OrderError, OrderRequest};
use crate::ports::order_executor::OrderExecutor;
use crate::ports::order_repository::OrderRepository;
use crate::ports::order_service::OrderService;
use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

/// 실거래 주문 실행기와 저장소를 연결하는 주문 서비스입니다.
///
/// # Examples
///
/// ```ignore
/// # use std::sync::Arc;
/// # use trading_bot::application::live_order_service::LiveOrderService;
/// # use trading_bot::ports::order_executor::OrderExecutor;
/// # use trading_bot::ports::order_repository::OrderRepository;
/// # fn example(executor: Arc<dyn OrderExecutor>, repo: Arc<dyn OrderRepository>) {
/// let service = LiveOrderService::new(executor, repo);
/// # }
/// ```
pub struct LiveOrderService {
    executor: Arc<dyn OrderExecutor>,
    repo: Arc<dyn OrderRepository>,
}

impl LiveOrderService {
    /// 새 실거래 주문 서비스를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use std::sync::Arc;
    /// # use trading_bot::application::live_order_service::LiveOrderService;
    /// # use trading_bot::ports::order_executor::OrderExecutor;
    /// # use trading_bot::ports::order_repository::OrderRepository;
    /// # fn example(executor: Arc<dyn OrderExecutor>, repo: Arc<dyn OrderRepository>) {
    /// let service = LiveOrderService::new(executor, repo);
    /// # }
    /// ```
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
