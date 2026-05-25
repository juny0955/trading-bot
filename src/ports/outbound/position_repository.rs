use crate::domain::account::ExchangePositionSnapshot;
use crate::domain::order::Fill;
use crate::domain::position::{PositionAction, StoredPosition};
use crate::ports::order_repository::RepositoryError;
use async_trait::async_trait;

#[async_trait]
pub trait PositionRepository: Send + Sync {
    async fn get(
        &self,
        instance_id: i64,
        symbol: &str,
    ) -> Result<Option<StoredPosition>, RepositoryError>;
    async fn apply_fill(
        &self,
        instance_id: i64,
        fill: &Fill,
    ) -> Result<(StoredPosition, PositionAction), RepositoryError>;
    /// 거래소 포지션 스냅샷으로 DB를 덮어씁니다. 부팅 시 동기화에 사용합니다.
    async fn upsert_from_snapshot(
        &self,
        instance_id: i64,
        snap: &ExchangePositionSnapshot,
    ) -> Result<(), RepositoryError>;
}
