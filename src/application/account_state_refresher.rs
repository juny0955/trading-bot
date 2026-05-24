use crate::application::account_state::AccountState;
use crate::ports::outbound::account_source::AccountSource;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

/// REST 폴링으로 계정 상태를 주기적으로 갱신하는 백그라운드 태스크.
pub struct AccountStateRefresher {
    client: Arc<dyn AccountSource>,
    state: Arc<AccountState>,
}

impl AccountStateRefresher {
    /// 새 `AccountStateRefresher`를 생성한다.
    pub fn new(client: Arc<dyn AccountSource>, state: Arc<AccountState>) -> Self {
        Self { client, state }
    }

    /// 취소 신호까지 10초 간격으로 계정 스냅샷을 갱신한다.
    pub async fn run(self, token: CancellationToken) {
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = tick.tick() => {
                    match self.client.fetch_account().await {
                        Ok(snap) => {
                            let replaced = self.state.try_replace(snap);
                            debug!(replaced, "AccountStateRefresher: 계정 스냅샷 갱신");
                        }
                        Err(e) => warn!(error=%e, "AccountStateRefresher: 계정 조회 실패"),
                    }
                }
            }
        }
    }
}
