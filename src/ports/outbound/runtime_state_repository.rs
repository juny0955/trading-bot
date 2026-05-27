use crate::domain::lifecycle::PositionRuntimeState;
use crate::ports::order_repository::RepositoryError;
use async_trait::async_trait;
use rust_decimal::Decimal;

#[async_trait]
pub trait RuntimeStateRepository: Send + Sync {
    /// 포지션 runtime state를 upsert합니다 (포지션 open 또는 상태 변경 시).
    async fn upsert(
        &self,
        strategy_instance_id: i64,
        symbol: &str,
        state: &PositionRuntimeState,
        entry_atr: Option<Decimal>,
    ) -> Result<(), RepositoryError>;

    /// 포지션 종료 시 runtime state를 삭제합니다.
    async fn delete(&self, strategy_instance_id: i64, symbol: &str) -> Result<(), RepositoryError>;

    /// 저장된 모든 runtime state를 반환합니다 (재시작 복구용).
    async fn load_all(
        &self,
    ) -> Result<Vec<(i64, String, PositionRuntimeState, Option<Decimal>)>, RepositoryError>;
}
