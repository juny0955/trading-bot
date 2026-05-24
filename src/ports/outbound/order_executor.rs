use crate::domain::order::{ExchangeOpenOrder, Order, OrderError, OrderRequest};
use async_trait::async_trait;

#[async_trait]
pub trait OrderExecutor: Send + Sync {
    async fn submit(&self, request: OrderRequest) -> Result<Order, OrderError>;
    async fn cancel(&self, order: &Order) -> Result<Order, OrderError>;
    async fn query(&self, order: &Order) -> Result<Order, OrderError>;
    async fn open_orders(&self, symbol: &str) -> Result<Vec<ExchangeOpenOrder>, OrderError>;

    /// origClientOrderId 기반 단일 주문 조회.
    /// 모호한 송신 실패 후 Reconciler가 거래소 실재 여부를 판별할 때 사용.
    /// 거래소에 존재하지 않으면 `OrderError::NotFound`.
    async fn query_by_client_id(
        &self,
        symbol: &str,
        client_order_id: &str,
    ) -> Result<Order, OrderError>;
}
