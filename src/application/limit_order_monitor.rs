use crate::application::position_lifecycle_tracker::PositionLifecycleTracker;
use crate::application::trade_manager::{PositionKey, TradeManager};
use crate::domain::lifecycle::PositionRuntimeState;
use crate::domain::order::{OrderRequest, OrderType};
use crate::ports::order_service::OrderService;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

struct LimitEntry {
    /// 타임아웃 기한
    deadline: Instant,
    /// 타임아웃 시 제출할 시장가 대체 주문
    market_replace: OrderRequest,
    /// lifecycle 등록용 (None이면 등록 건너뜀)
    lifecycle_id: Option<Uuid>,
    /// TradeManager 초기화 키
    key: PositionKey,
}

/// 지정가 진입 주문을 감시하고 타임아웃 시 시장가로 전환합니다.
///
/// # 동작 원리
/// - `register()`: 지정가 주문 제출 후 등록
/// - `check_timeouts()`: 각 BookTicker 수신 시 호출 — 만료된 항목을 cancel → 시장가 대체
/// - deregister 불필요: cancel 실패(이미 체결)는 조용히 무시
pub struct LimitOrderMonitor {
    entries: RwLock<HashMap<Uuid, LimitEntry>>,
}

impl LimitOrderMonitor {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// 지정가 주문을 모니터에 등록합니다.
    pub async fn register(
        &self,
        order_id: Uuid,
        timeout_secs: u64,
        market_replace: OrderRequest,
        lifecycle_id: Option<Uuid>,
        key: PositionKey,
    ) {
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        self.entries.write().await.insert(
            order_id,
            LimitEntry {
                deadline,
                market_replace,
                lifecycle_id,
                key,
            },
        );
    }

    /// 만료된 지정가 주문을 취소하고 시장가로 대체합니다.
    /// BookTicker 수신마다 호출합니다.
    pub async fn check_timeouts(
        &self,
        order_svc: &Arc<dyn OrderService>,
        lifecycle_tracker: &Arc<PositionLifecycleTracker>,
        trade_manager: &Arc<TradeManager>,
    ) {
        let now = Instant::now();

        // 만료된 항목 수집
        let expired: Vec<(Uuid, LimitEntry)> = {
            let mut entries = self.entries.write().await;
            let expired_ids: Vec<Uuid> = entries
                .iter()
                .filter(|(_, e)| now >= e.deadline)
                .map(|(id, _)| *id)
                .collect();
            expired_ids
                .into_iter()
                .filter_map(|id| entries.remove(&id).map(|e| (id, e)))
                .collect()
        };

        for (order_id, entry) in expired {
            let symbol = &entry.key.1;
            // 지정가 주문 취소 시도
            match order_svc.cancel(order_id).await {
                Ok(_) => {
                    info!(order_id = %order_id, symbol, "지정가 타임아웃 → 시장가 전환");
                    // 시장가 대체 주문 제출
                    match order_svc.submit(entry.market_replace).await {
                        Ok(order) => {
                            if let Some(lid) = entry.lifecycle_id {
                                lifecycle_tracker.register_order(order.id, lid).await;
                            }
                            // TradeManager가 아직 초기화되지 않은 경우 초기화
                            if trade_manager.get_state(&entry.key).await.is_none() {
                                trade_manager
                                    .init_position(
                                        entry.key.clone(),
                                        PositionRuntimeState::default(),
                                        None,
                                    )
                                    .await;
                            }
                            info!(id = %order.id, symbol, "시장가 대체 주문 제출");
                        }
                        Err(e) => warn!(error = %e, symbol, "시장가 대체 주문 실패"),
                    }
                }
                Err(e) => {
                    // 이미 체결됐거나 취소됨 — 정상 케이스, 조용히 무시
                    info!(error = %e, order_id = %order_id, symbol, "지정가 이미 처리됨 (취소 불필요)");
                }
            }
        }
    }
}

impl Default for LimitOrderMonitor {
    fn default() -> Self {
        Self::new()
    }
}

fn _market_replace(mut req: OrderRequest) -> OrderRequest {
    req.order_type = OrderType::Market;
    req.price = None;
    req
}
