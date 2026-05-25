use crate::application::risk_controller::{RiskController, RiskError};
use crate::domain::order::{ExchangeOpenOrder, Order, OrderError, OrderRequest, OrderStatus};
use crate::ports::outbound::order_executor::OrderExecutor;
use crate::ports::outbound::order_repository::OrderRepository;
use crate::ports::outbound::order_service::OrderService;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

impl From<RiskError> for OrderError {
    fn from(e: RiskError) -> Self {
        match e {
            RiskError::InsufficientMargin { .. } => OrderError::InsufficientMargin,
            other => OrderError::InvalidRequest(other.to_string()),
        }
    }
}

/// 위험 검증 + Outbox 패턴으로 주문을 처리하는 서비스.
///
/// 거래소 전송 전 `PendingSubmit`을 DB에 기록하여 중간 장애에도
/// `OrderReconciler`가 최종 상태를 복원할 수 있도록 한다.
pub struct OutboxOrderService {
    executor: Arc<dyn OrderExecutor>,
    repo: Arc<dyn OrderRepository>,
    risk: Arc<RiskController>,
    dry_run: bool,
}

impl OutboxOrderService {
    /// 새 `OutboxOrderService`를 생성한다.
    pub fn new(
        executor: Arc<dyn OrderExecutor>,
        repo: Arc<dyn OrderRepository>,
        risk: Arc<RiskController>,
    ) -> Self {
        Self {
            executor,
            repo,
            risk,
            dry_run: false,
        }
    }

    /// dry_run 모드를 설정한다. true이면 위험 검증만 수행하고 실제 전송을 건너뜀.
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }
}

#[async_trait]
impl OrderService for OutboxOrderService {
    /// 위험 검증 후 주문을 DB에 기록하고 거래소에 전송한다.
    ///
    /// - 성공: `New` 상태로 확정
    /// - 명확한 거부(InsufficientMargin 등): `Rejected`로 확정
    /// - 모호한 실패(네트워크 등): `PendingSubmit` 유지 → Reconciler가 정정
    async fn submit(&self, request: OrderRequest) -> Result<Order, OrderError> {
        self.risk.check(&request).await.map_err(OrderError::from)?;

        let (pending, req) = create_pending_order(request)?;

        if self.dry_run {
            info!(
                client_order_id = %pending.client_order_id,
                symbol = %pending.symbol,
                side = ?pending.side,
                qty = %pending.qty,
                "DRY RUN — 주문 전송 건너뜀"
            );
            return Ok(pending);
        }

        self.repo.upsert_order(&pending).await?;

        match self.executor.submit(req).await {
            Ok(mut ack) => {
                ack.id = pending.id;
                ack.client_order_id = pending.client_order_id.clone();
                ack.status = OrderStatus::New;
                self.repo.upsert_order(&ack).await?;
                Ok(ack)
            }
            Err(e) if is_definitive_rejection(&e) => {
                let rejected = Order {
                    status: OrderStatus::Rejected,
                    ..pending.clone()
                };
                let _ = self.repo.upsert_order(&rejected).await;
                Err(e)
            }
            Err(e) => {
                warn!(
                    client_order_id = %pending.client_order_id,
                    error = %e,
                    "submit ambiguous failure — left as PendingSubmit, reconciler will resolve"
                );
                Err(e)
            }
        }
    }

    /// 지정 ID 주문을 취소한다.
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
        self.executor.open_orders(symbol).await
    }
}

/// `PendingSubmit` 상태의 주문을 생성하고 `client_order_id`를 request에 주입한다.
fn create_pending_order(request: OrderRequest) -> Result<(Order, OrderRequest), OrderError> {
    let mut pending = Order::new_from_request(request.clone())?;
    pending.status = OrderStatus::PendingSubmit;
    let mut req = request;
    req.client_order_id = Some(pending.client_order_id.clone());
    Ok((pending, req))
}

fn is_definitive_rejection(e: &OrderError) -> bool {
    matches!(
        e,
        OrderError::InsufficientMargin
            | OrderError::InvalidRequest(_)
            | OrderError::ExchangeRejected { .. }
    )
}
