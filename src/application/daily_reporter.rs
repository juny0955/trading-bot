use crate::domain::order::{Fill, OrderSide};
use crate::ports::outbound::notification_sink::NotificationSink;
use crate::ports::outbound::order_repository::OrderRepository;
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::error;

/// 매일 UTC 자정에 전날 체결 요약을 전송하는 태스크.
pub struct DailyReporter {
    repo: Arc<dyn OrderRepository>,
    notifier: Arc<dyn NotificationSink>,
}

impl DailyReporter {
    pub fn new(repo: Arc<dyn OrderRepository>, notifier: Arc<dyn NotificationSink>) -> Self {
        Self { repo, notifier }
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
            match self.repo.fills_since(since).await {
                Ok(fills) => {
                    let msg = format_daily_summary(&fills, since);
                    self.notifier.send(msg).await;
                }
                Err(e) => error!(error = %e, "일별 리포트 체결 조회 실패"),
            }
        }
    }
}

/// 체결 목록을 일별 요약 메시지로 변환한다.
fn format_daily_summary(fills: &[Fill], since: chrono::DateTime<Utc>) -> String {
    let date_label = since.format("%Y-%m-%d");
    if fills.is_empty() {
        return format!("📊 일일 리포트 {date_label} — 체결 없음");
    }

    let count = fills.len();
    let buy_count = fills.iter().filter(|f| f.side == OrderSide::Buy).count();
    let sell_count = count - buy_count;
    let total_volume: Decimal = fills.iter().map(|f| f.qty * f.price).sum();
    let total_fee: Decimal = fills.iter().map(|f| f.fee).sum();

    format!(
        "📊 일일 리포트 {date_label}\n거래: {count}건 (매수 {buy_count} / 매도 {sell_count})\n거래량: {total_volume:.2} USDT\n수수료: {total_fee:.4} USDT"
    )
}

/// 현재 시각부터 다음 UTC 자정까지 남은 초를 반환한다.
fn secs_until_next_utc_midnight() -> u64 {
    let now = Utc::now();
    // 다음 날 00:00:00 UTC
    let tomorrow = now.date_naive() + Duration::days(1);
    let next_midnight = tomorrow
        .and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc())
        .unwrap_or(now);
    (next_midnight - now).num_seconds().max(1) as u64
}
