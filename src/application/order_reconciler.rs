use crate::domain::order::{Order, OrderStatus};
use crate::ports::outbound::order_executor::OrderExecutor;
use crate::ports::outbound::order_repository::OrderRepository;
use crate::ports::outbound::order_service::OrderService;
use crate::ports::outbound::risk_config_source::RiskLimits;
use arc_swap::ArcSwap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::warn;

/// 주기적으로 PendingSubmit 정정, TTL 취소, 거래소 drift 감지를 수행하는 백그라운드 서비스.
pub struct OrderReconciler {
    order_svc: Arc<dyn OrderService>,
    executor: Arc<dyn OrderExecutor>,
    repo: Arc<dyn OrderRepository>,
    limits: Arc<ArcSwap<RiskLimits>>,
    symbols: Vec<String>,
}

impl OrderReconciler {
    /// 새 `OrderReconciler`를 생성한다.
    pub fn new(
        order_svc: Arc<dyn OrderService>,
        executor: Arc<dyn OrderExecutor>,
        repo: Arc<dyn OrderRepository>,
        limits: Arc<ArcSwap<RiskLimits>>,
        symbols: Vec<String>,
    ) -> Self {
        Self {
            order_svc,
            executor,
            repo,
            limits,
            symbols,
        }
    }

    /// 취소 신호까지 60초 간격으로 reconcile sweep을 반복한다.
    pub async fn run(self, token: CancellationToken) {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = tick.tick() => {
                    let _ = self.resolve_all_pending().await;
                    let _ = self.enforce_ttl_and_detect_drift().await;
                }
            }
        }
    }

    async fn resolve_all_pending(&self) -> anyhow::Result<()> {
        if let Ok(pendings) = self.repo.find_by_status(OrderStatus::PendingSubmit).await {
            for pending in pendings {
                self.resolve_pending(&pending).await;
            }
        }
        Ok(())
    }

    async fn enforce_ttl_and_detect_drift(&self) -> anyhow::Result<()> {
        let ttl = Duration::from_secs(self.limits.load().order_ttl_sec);
        let now = chrono::Utc::now();

        for symbol in &self.symbols {
            if let Ok(db_open) = self.repo.find_open(symbol).await {
                for order in &db_open {
                    if let Ok(duration) = (now - order.created_at).to_std()
                        && duration > ttl
                    {
                        let _ = self.order_svc.cancel(order.id).await;
                        warn!(client_order_id=%order.client_order_id, "Reconciler: TTL 초과 주문 취소");
                    }
                }

                if let Ok(ex_open) = self.executor.open_orders(symbol).await {
                    self.detect_drift(symbol, &db_open, &ex_open).await;
                }
            }
        }
        Ok(())
    }

    async fn resolve_pending(&self, pending: &Order) {
        match self.executor.query_by_client_id(&pending.symbol, &pending.client_order_id).await {
            Ok(remote) => {
                let updated = Order {
                    exchange_order_id: remote.exchange_order_id,
                    status: remote.status,
                    filled_qty: remote.filled_qty,
                    avg_fill_price: remote.avg_fill_price,
                    ..pending.clone()
                };
                let _ = self.repo.upsert_order(&updated).await;
            }
            Err(crate::domain::order::OrderError::NotFound(_)) => {
                let rejected = Order { status: OrderStatus::Rejected, ..pending.clone() };
                let _ = self.repo.upsert_order(&rejected).await;
            }
            Err(e) => {
                warn!(client_order_id=%pending.client_order_id, error=%e,
                      "pending resolve transient failure — retry next sweep");
            }
        }
    }

    async fn detect_drift(
        &self,
        _symbol: &str,
        db_open: &[Order],
        ex_open: &[crate::domain::order::ExchangeOpenOrder],
    ) {
        for db_order in db_open {
            let still_on_exchange = ex_open
                .iter()
                .any(|ex| Some(ex.exchange_order_id) == db_order.exchange_order_id);
            if !still_on_exchange && let Ok(remote) = self.executor.query(db_order).await {
                let _ = self.repo.upsert_order(&remote).await;
                warn!(
                    client_order_id = %db_order.client_order_id,
                    new_status = ?remote.status,
                    "Reconciler: DB open -> 거래소 최종 상태로 갱신 (drift해소)"
                );
            }
        }
    }
}
