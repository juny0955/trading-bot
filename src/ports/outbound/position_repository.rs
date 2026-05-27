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
    /// 포지션을 flat(qty=0)으로 정정합니다.
    ///
    /// `fetch_positions()`는 flat 포지션을 목록에서 제외하므로 `upsert_from_snapshot`만으로는
    /// "거래소엔 없는데 DB엔 남아 있는" 유령 포지션을 청소할 수 없습니다. 이 메서드로 qty를
    /// 0으로 만들면 `get()`이 flat(side `None`)으로 매핑합니다.
    async fn mark_flat(&self, instance_id: i64, symbol: &str) -> Result<(), RepositoryError>;
}
