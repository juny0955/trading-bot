use crate::domain::account::{AccountError, AccountSnapshot, ExchangePositionSnapshot};
use async_trait::async_trait;

#[async_trait]
pub trait AccountSource: Send + Sync {
    /// 가용 잔고와 미실현 손익을 포함한 계정 스냅샷을 조회한다.
    async fn fetch_account(&self) -> Result<AccountSnapshot, AccountError>;

    /// 거래소가 관리하는 모든 활성 포지션을 조회한다.
    async fn fetch_positions(&self) -> Result<Vec<ExchangePositionSnapshot>, AccountError>;

    /// 심볼별 레버리지 및 마진 모드를 거래소에 설정한다.
    /// 기본 구현은 아무것도 하지 않는다 (테스트용 mock에서 오버라이드 불필요).
    async fn configure_symbol(
        &self,
        _symbol: &str,
        _leverage: i16,
        _margin_type: &str,
    ) -> Result<(), AccountError> {
        Ok(())
    }
}
