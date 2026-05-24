use crate::domain::order::Fill;
use crate::domain::position::StoredPosition;
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
    ) -> Result<StoredPosition, RepositoryError>;
}
