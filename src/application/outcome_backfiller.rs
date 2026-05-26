use crate::ports::order_repository::RepositoryError;
use crate::ports::outbound::decision_repository::DecisionRepository;
use crate::ports::outbound::lifecycle_repository::LifecycleRepository;
use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

const BACKFILL_BATCH: i64 = 50;
const UNRESOLVED_BATCH: i64 = 20;
const DEAD_LETTER_THRESHOLD: i16 = 10;

/// trade_decisions의 RL reward 라벨을 백필하고 unresolved_fills를 재처리합니다.
///
/// # 동작
/// - `pnl_after_5m`이 NULL인 결정 중 결정 시각으로부터 5분 이상 경과한 것을 백필합니다.
/// - pnl_after_Xm 값은 현재 미구현 (QuestDB mark_price 연동 필요) — NULL로 기록합니다.
/// - PENDING 상태의 unresolved_fills를 재처리합니다 (retry_count > 10이면 DEAD 처리).
pub struct OutcomeBackfiller {
    decision_repo: Arc<dyn DecisionRepository>,
    lifecycle_repo: Arc<dyn LifecycleRepository>,
}

impl OutcomeBackfiller {
    pub fn new(
        decision_repo: Arc<dyn DecisionRepository>,
        lifecycle_repo: Arc<dyn LifecycleRepository>,
    ) -> Self {
        Self {
            decision_repo,
            lifecycle_repo,
        }
    }

    /// 정기 실행 루프 (60초 간격 권장).
    pub async fn run(&self, interval: std::time::Duration, token: CancellationToken) {
        let mut ticker = tokio::time::interval(interval);
        loop {
            tokio::select! {
                _ = token.cancelled() => return,
                _ = ticker.tick() => {
                    self.backfill_decisions().await;
                    self.process_unresolved().await;
                }
            }
        }
    }

    async fn backfill_decisions(&self) {
        let decisions = match self.decision_repo.find_pending_backfill(BACKFILL_BATCH).await {
            Ok(d) => d,
            Err(e) => {
                error!(error = %e, "trade_decisions 백필 조회 실패");
                return;
            }
        };

        let now = Utc::now();
        let mut filled = 0usize;

        for decision in &decisions {
            // 5분이 경과하지 않은 결정은 아직 백필 불가
            if now - decision.decided_at < Duration::minutes(5) {
                continue;
            }
            // QuestDB mark_price 연동 전까지는 NULL로 기록 (index에서 제외됨)
            if let Err(e) = self
                .decision_repo
                .backfill_outcome(decision.id, None, None, None, None)
                .await
            {
                error!(id = %decision.id, error = %e, "결정 백필 실패");
            } else {
                filled += 1;
            }
        }

        if filled > 0 {
            info!(count = filled, "trade_decisions 백필 완료");
        }
    }

    async fn process_unresolved(&self) {
        let fills = match self
            .lifecycle_repo
            .find_unresolved_pending(UNRESOLVED_BATCH)
            .await
        {
            Ok(f) => f,
            Err(e) => {
                error!(error = %e, "unresolved_fills 조회 실패");
                return;
            }
        };

        for fill in &fills {
            if fill.retry_count >= DEAD_LETTER_THRESHOLD {
                let msg = format!("retry_count({}) exceeded threshold", fill.retry_count);
                warn!(id = %fill.id, exchange_trade_id = ?fill.exchange_trade_id, "unresolved_fill DEAD 처리");
                if let Err(e) = self.lifecycle_repo.mark_unresolved_dead(fill.id, &msg).await {
                    error!(id = %fill.id, error = %e, "DEAD 처리 실패");
                }
                continue;
            }
            // lifecycle_id 재매핑 시도 (현재는 단순 retry count 증가만 수행)
            // 실제 재매핑 로직은 LiveTrader와 order_lifecycle_map 연동 후 구현
            if let Err(e) = self
                .lifecycle_repo
                .increment_unresolved_retry(fill.id, "retry pending")
                .await
            {
                error!(id = %fill.id, error = %e, "retry count 증가 실패");
            }
        }
    }
}

// ── 공개 유틸리티: lifecycle 종료 후 수동 집계 백필 ─────────────────────────

impl OutcomeBackfiller {
    /// 청산 완료된 lifecycle의 집계 데이터를 fills에서 계산해 백필합니다.
    pub async fn backfill_lifecycle(
        &self,
        lifecycle_id: Uuid,
        exit_avg_price: Decimal,
        realized_pnl: Decimal,
        fee_total: Decimal,
        net_pnl: Decimal,
        mfe_pct: Option<Decimal>,
        mae_pct: Option<Decimal>,
    ) -> Result<(), RepositoryError> {
        self.lifecycle_repo
            .backfill_close(
                lifecycle_id,
                exit_avg_price,
                realized_pnl,
                fee_total,
                net_pnl,
                mfe_pct,
                mae_pct,
            )
            .await
    }
}
