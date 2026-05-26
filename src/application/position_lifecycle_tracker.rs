use crate::domain::lifecycle::LifecycleSide;
use crate::ports::order_repository::RepositoryError;
use crate::ports::outbound::lifecycle_repository::LifecycleRepository;
use rust_decimal::Decimal;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// order_id → lifecycle_id 매핑을 관리하고 lifecycle DB 영속화를 담당합니다.
///
/// # 설계 원칙
/// - fill 귀속은 "현재 활성 포지션" 기준이 아닌 order_id 기반으로 수행한다.
///   position flip 시 오귀속을 방지하기 위한 것.
/// - 매핑 실패 fill은 fills 테이블에 insert하지 않고 unresolved_fills에 격리한다.
/// - order cleanup은 이벤트 드리븐(terminal 상태 + fill stream idle + reconciliation)으로 수행한다.
pub struct PositionLifecycleTracker {
    repo: Arc<dyn LifecycleRepository>,
    /// order_id → lifecycle_id (fill 귀속 정확도 보장)
    map: RwLock<HashMap<Uuid, Uuid>>,
    /// (strategy_instance_id, symbol) → lifecycle_id (현재 활성 포지션 조회용)
    active: RwLock<HashMap<(i64, String), Uuid>>,
}

impl PositionLifecycleTracker {
    pub fn new(repo: Arc<dyn LifecycleRepository>) -> Self {
        Self {
            repo,
            map: RwLock::new(HashMap::new()),
            active: RwLock::new(HashMap::new()),
        }
    }

    /// 주문 생성 시 lifecycle과 연결합니다. LiveTrader에서 주문 제출 후 호출합니다.
    pub async fn register_order(&self, order_id: Uuid, lifecycle_id: Uuid) {
        self.map.write().await.insert(order_id, lifecycle_id);
    }

    /// fill 수신 시 lifecycle_id를 조회합니다.
    ///
    /// - `Some(id)` → 매핑 성공, fills 테이블에 기록 가능
    /// - `None` → 매핑 실패, unresolved_fills에 격리 필요
    pub async fn resolve_lifecycle(&self, order_id: Uuid) -> Option<Uuid> {
        self.map.read().await.get(&order_id).copied()
    }

    /// 주문이 terminal 상태에 도달하고 fill stream idle + reconciliation 완료 후 호출합니다.
    /// 시간 기반 제거 금지 — partial fill / late fill 오제거 방지.
    pub async fn deregister_order(&self, order_id: Uuid) {
        self.map.write().await.remove(&order_id);
    }

    /// 새 포지션 lifecycle을 DB에 생성하고 lifecycle_id를 반환합니다.
    /// LiveTrader에서 진입 주문 제출 전 호출합니다.
    pub async fn open_lifecycle(
        &self,
        strategy_instance_id: i64,
        symbol: &str,
        side: LifecycleSide,
        entry_atr: Option<Decimal>,
    ) -> Result<Uuid, RepositoryError> {
        self.repo.open(strategy_instance_id, symbol, side, entry_atr).await
    }

    /// lifecycle을 청산 완료 상태로 표시합니다 (closed_at = NOW()).
    pub async fn close_lifecycle(&self, id: Uuid) -> Result<(), RepositoryError> {
        self.repo.close(id).await
    }

    /// lifecycle 매핑 실패 체결을 unresolved_fills에 격리합니다.
    /// fills 테이블에는 insert하지 않습니다.
    pub async fn quarantine_fill(
        &self,
        exchange_trade_id: Option<i64>,
        raw_payload: Value,
        reason: &str,
    ) -> Result<(), RepositoryError> {
        self.repo.save_unresolved(exchange_trade_id, raw_payload, reason).await
    }

    /// high/low water mark을 DB에 flush합니다.
    /// 메모리는 PositionRuntimeState에 즉시 갱신, DB는 30초/1%/포지션 종료 시 여기서 flush.
    pub async fn flush_water_marks(
        &self,
        id: Uuid,
        high: Option<Decimal>,
        low: Option<Decimal>,
    ) -> Result<(), RepositoryError> {
        self.repo.update_water_marks(id, high, low).await
    }

    /// (strategy_instance_id, symbol)에 활성 lifecycle을 등록합니다.
    /// open_lifecycle() 성공 후 LiveTrader에서 호출합니다.
    pub async fn set_active_lifecycle(&self, instance_id: i64, symbol: &str, lifecycle_id: Uuid) {
        self.active.write().await.insert((instance_id, symbol.to_string()), lifecycle_id);
    }

    /// (strategy_instance_id, symbol)의 현재 활성 lifecycle_id를 조회합니다.
    /// ManagementCommand 실행 시 fill 귀속에 사용합니다.
    pub async fn get_active_lifecycle(&self, instance_id: i64, symbol: &str) -> Option<Uuid> {
        self.active.read().await.get(&(instance_id, symbol.to_string())).copied()
    }

    /// 포지션 종료 시 활성 lifecycle 매핑을 제거합니다.
    pub async fn clear_active_lifecycle(&self, instance_id: i64, symbol: &str) {
        self.active.write().await.remove(&(instance_id, symbol.to_string()));
    }

    pub fn repo(&self) -> &Arc<dyn LifecycleRepository> {
        &self.repo
    }
}
