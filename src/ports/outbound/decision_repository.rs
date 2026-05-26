use crate::domain::trade_decision::{TradeDecision, TradeDecisionInsert};
use crate::ports::order_repository::RepositoryError;
use async_trait::async_trait;
use rust_decimal::Decimal;
use uuid::Uuid;

#[async_trait]
pub trait DecisionRepository: Send + Sync {
    /// 새 trade_decision을 PENDING 상태로 insert하고 UUID를 반환합니다.
    async fn insert(&self, decision: &TradeDecisionInsert) -> Result<Uuid, RepositoryError>;

    /// 거래소 ACK 수신 시 상태를 갱신합니다.
    async fn ack(&self, id: Uuid, latency_ms: i32) -> Result<(), RepositoryError>;

    /// 거래소 거부 시 상태를 갱신합니다.
    async fn reject(&self, id: Uuid, error: &str) -> Result<(), RepositoryError>;

    /// ACK 타임아웃 시 상태를 갱신합니다.
    async fn timeout(&self, id: Uuid) -> Result<(), RepositoryError>;

    /// pnl_after_5m이 NULL인 레코드를 조회합니다 (OutcomeBackfiller 용).
    async fn find_pending_backfill(
        &self,
        limit: i64,
    ) -> Result<Vec<TradeDecision>, RepositoryError>;

    /// RL reward 라벨을 백필합니다 (OutcomeBackfiller 용).
    async fn backfill_outcome(
        &self,
        id: Uuid,
        pnl_after_5m: Option<Decimal>,
        pnl_after_10m: Option<Decimal>,
        pnl_after_30m: Option<Decimal>,
        max_drawdown_until_close: Option<Decimal>,
    ) -> Result<(), RepositoryError>;
}
