use crate::application::notification_formatter::format_daily_summary;
use crate::ports::outbound::lifecycle_repository::LifecycleRepository;
use crate::ports::outbound::notification_sink::NotificationSink;
use crate::ports::outbound::order_repository::OrderRepository;
use chrono::{Duration, Utc};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::error;

/// 매일 UTC 자정에 전날 체결 요약을 전송하는 태스크.
///
/// # Examples
///
/// ```ignore
/// let reporter = DailyReporter::new(repo, lifecycle_repo, notifier);
/// reporter.run(CancellationToken::new()).await;
/// ```
pub struct DailyReporter {
    repo: Arc<dyn OrderRepository>,
    lifecycle_repo: Arc<dyn LifecycleRepository>,
    notifier: Arc<dyn NotificationSink>,
}

impl DailyReporter {
    pub fn new(
        repo: Arc<dyn OrderRepository>,
        lifecycle_repo: Arc<dyn LifecycleRepository>,
        notifier: Arc<dyn NotificationSink>,
    ) -> Self {
        Self {
            repo,
            lifecycle_repo,
            notifier,
        }
    }

    /// 다음 UTC 자정까지 대기한 뒤 24시간마다 일별 요약을 전송한다.
    pub async fn run(self, token: CancellationToken) {
        loop {
            let secs = secs_until_next_utc_midnight();
            tokio::select! {
                _ = token.cancelled() => return,
                _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => {}
            }

            // 자정 도달 — 전날(지난 24시간) 체결 내역 조회
            let since = Utc::now() - Duration::hours(24);
            let daily_pnl = match self.lifecycle_repo.net_pnl_since(since).await {
                Ok(pnl) => Some(pnl),
                Err(e) => {
                    error!(error = %e, "일별 리포트 PnL 조회 실패");
                    None
                }
            };
            match self.repo.fills_since(since).await {
                Ok(fills) => {
                    self.notifier
                        .send(format_daily_summary(&fills, daily_pnl, since))
                        .await
                }
                Err(e) => error!(error = %e, "일별 리포트 체결 조회 실패"),
            }
        }
    }
}

/// 현재 시각부터 다음 UTC 자정까지 남은 초를 반환한다.
fn secs_until_next_utc_midnight() -> u64 {
    let now = Utc::now();
    let tomorrow = now.date_naive() + Duration::days(1);
    let next_midnight = tomorrow
        .and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc())
        .unwrap_or(now);
    (next_midnight - now).num_seconds().max(1) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn secs_until_next_utc_midnight_is_in_valid_range() -> Result<()> {
        // 결과는 (0, 86400] 범위여야 한다
        let secs = secs_until_next_utc_midnight();
        assert!(secs > 0, "남은 초는 양수여야 한다");
        assert!(secs <= 86400, "최대 24시간(86400초) 이하여야 한다");
        Ok(())
    }
}
