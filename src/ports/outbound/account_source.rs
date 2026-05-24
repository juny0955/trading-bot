use crate::domain::account::{AccountError, AccountSnapshot, ExchangePositionSnapshot};
use async_trait::async_trait;

#[async_trait]
pub trait AccountSource: Send + Sync {
    /// 가용 잔고와 미실현 손익을 포함한 계정 스냅샷을 조회한다.
    async fn fetch_account(&self) -> Result<AccountSnapshot, AccountError>;

    /// 거래소가 관리하는 모든 활성 포지션을 조회한다.
    async fn fetch_positions(&self) -> Result<Vec<ExchangePositionSnapshot>, AccountError>;
}
