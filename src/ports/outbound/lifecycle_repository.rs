use crate::domain::lifecycle::{LifecycleSide, PositionLifecycle, UnresolvedFill};
use crate::ports::order_repository::RepositoryError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use uuid::Uuid;

#[async_trait]
pub trait LifecycleRepository: Send + Sync {
    /// 새 포지션 lifecycle을 DB에 생성하고 UUID를 반환합니다.
    async fn open(
        &self,
        strategy_instance_id: i64,
        symbol: &str,
        side: LifecycleSide,
        entry_atr: Option<Decimal>,
    ) -> Result<Uuid, RepositoryError>;

    /// lifecycle을 청산 완료 상태로 표시합니다 (closed_at = NOW()).
    async fn close(&self, id: Uuid) -> Result<(), RepositoryError>;

    /// 해당 strategy_instance + symbol의 열린 lifecycle을 반환합니다.
    async fn find_open(
        &self,
        strategy_instance_id: i64,
        symbol: &str,
    ) -> Result<Option<PositionLifecycle>, RepositoryError>;

    /// high/low water mark을 DB에 flush합니다 (주기적 또는 포지션 종료 시).
    async fn update_water_marks(
        &self,
        id: Uuid,
        high: Option<Decimal>,
        low: Option<Decimal>,
    ) -> Result<(), RepositoryError>;

    /// 청산 후 집계 데이터를 백필합니다 (OutcomeBackfiller 용).
    #[allow(clippy::too_many_arguments)]
    async fn backfill_close(
        &self,
        id: Uuid,
        exit_avg_price: Decimal,
        realized_pnl: Decimal,
        fee_total: Decimal,
        net_pnl: Decimal,
        mfe_pct: Option<Decimal>,
        mae_pct: Option<Decimal>,
    ) -> Result<(), RepositoryError>;

    /// lifecycle 매핑 실패 체결을 unresolved_fills에 격리합니다.
    async fn save_unresolved(
        &self,
        exchange_trade_id: Option<i64>,
        raw_payload: Value,
        reason: &str,
    ) -> Result<(), RepositoryError>;

    /// PENDING 상태의 unresolved_fills를 조회합니다 (OutcomeBackfiller 용).
    async fn find_unresolved_pending(
        &self,
        limit: i64,
    ) -> Result<Vec<UnresolvedFill>, RepositoryError>;

    async fn mark_unresolved_resolved(&self, id: Uuid) -> Result<(), RepositoryError>;
    async fn mark_unresolved_dead(&self, id: Uuid, error: &str) -> Result<(), RepositoryError>;
    async fn increment_unresolved_retry(
        &self,
        id: Uuid,
        error: &str,
    ) -> Result<(), RepositoryError>;

    /// `since` 이후 청산된 포지션의 net_pnl 합계를 반환한다. 일별 리포트용.
    async fn net_pnl_since(&self, since: DateTime<Utc>) -> Result<Decimal, RepositoryError>;
}
