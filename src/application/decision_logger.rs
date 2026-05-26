use crate::domain::lifecycle::{
    DecisionMarketContext, DecisionPositionContext, ManagementCommand,
};
use crate::domain::trade_decision::TradeDecisionInsert;
use crate::ports::order_repository::RepositoryError;
use crate::ports::outbound::decision_repository::DecisionRepository;
use rust_decimal::Decimal;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// ML 학습 데이터를 trade_decisions 테이블에 기록합니다.
///
/// # 과기록 방지
/// ReplaceSL은 SL 변화율이 `min_sl_change_pct` 미만이면 기록하지 않습니다.
/// trailing stop 매 tick 미세 이동 시 log 폭증 방지 + ML 데이터셋 noise 감소.
pub struct DecisionLogger {
    repo: Arc<dyn DecisionRepository>,
    /// trailing stop 노이즈 필터: SL 변화율이 이 값 미만이면 기록 건너뜀 (기본 0.1%)
    min_sl_change_pct: Decimal,
    /// lifecycle_id → 마지막으로 기록된 SL price
    last_sl_prices: RwLock<HashMap<Uuid, Decimal>>,
}

impl DecisionLogger {
    pub fn new(repo: Arc<dyn DecisionRepository>) -> Self {
        Self::with_threshold(repo, Decimal::new(1, 3)) // 0.001 = 0.1%
    }

    pub fn with_threshold(repo: Arc<dyn DecisionRepository>, min_sl_change_pct: Decimal) -> Self {
        Self {
            repo,
            min_sl_change_pct,
            last_sl_prices: RwLock::new(HashMap::new()),
        }
    }

    /// 포지션 관리 결정을 기록합니다.
    ///
    /// 반환값:
    /// - `Ok(Some(id))` — 기록됨, id로 ACK/REJECT/TIMEOUT 업데이트 가능
    /// - `Ok(None)` — 필터링됨 (ReplaceSL이지만 SL 변화 threshold 미만)
    pub async fn record(
        &self,
        lifecycle_id: Uuid,
        symbol: &str,
        command: &ManagementCommand,
        market: &DecisionMarketContext,
        position: &DecisionPositionContext,
    ) -> Result<Option<Uuid>, RepositoryError> {
        if !self.should_record(lifecycle_id, command).await {
            return Ok(None);
        }

        let insert = build_insert(lifecycle_id, symbol, command, market, position);
        let id = self.repo.insert(&insert).await?;

        if let ManagementCommand::ReplaceSL { desired_price, .. } = command {
            self.last_sl_prices.write().await.insert(lifecycle_id, *desired_price);
        }

        Ok(Some(id))
    }

    /// 거래소 ACK 수신 시 호출합니다.
    pub async fn ack(&self, id: Uuid, latency_ms: i32) -> Result<(), RepositoryError> {
        self.repo.ack(id, latency_ms).await
    }

    /// 거래소 거부 시 호출합니다.
    pub async fn reject(&self, id: Uuid, error: &str) -> Result<(), RepositoryError> {
        self.repo.reject(id, error).await
    }

    /// ACK 타임아웃 시 호출합니다.
    pub async fn timeout(&self, id: Uuid) -> Result<(), RepositoryError> {
        self.repo.timeout(id).await
    }

    /// lifecycle이 종료되면 SL 추적 상태를 정리합니다.
    pub async fn on_lifecycle_closed(&self, lifecycle_id: Uuid) {
        self.last_sl_prices.write().await.remove(&lifecycle_id);
    }

    async fn should_record(&self, lifecycle_id: Uuid, command: &ManagementCommand) -> bool {
        let ManagementCommand::ReplaceSL { desired_price, .. } = command else {
            return true; // PartialClose / EarlyExit / EmergencyClose는 항상 기록
        };
        let guard = self.last_sl_prices.read().await;
        let Some(&last) = guard.get(&lifecycle_id) else {
            return true; // 첫 SL 기록은 항상 통과
        };
        if last.is_zero() {
            return true;
        }
        let change_pct = ((*desired_price - last) / last).abs();
        change_pct >= self.min_sl_change_pct
    }
}

fn build_insert(
    lifecycle_id: Uuid,
    symbol: &str,
    command: &ManagementCommand,
    market: &DecisionMarketContext,
    position: &DecisionPositionContext,
) -> TradeDecisionInsert {
    let (decision_type, action_taken) = command_to_record(command);
    TradeDecisionInsert {
        position_lifecycle_id: lifecycle_id,
        symbol: symbol.to_string(),
        decision_type,
        atr: market.atr,
        atr_pct: market.atr_pct,
        adx: market.adx,
        spread_pct: market.spread_pct,
        fng_value: market.fng_value,
        ema_alignment: market.ema_alignment,
        ob_imbalance: market.ob_imbalance,
        unrealized_pnl_pct: position.unrealized_pnl_pct,
        time_in_trade_secs: position.time_in_trade_secs,
        dist_to_sl_atr: position.dist_to_sl_atr,
        dist_to_tp_atr: position.dist_to_tp_atr,
        tp_stage: position.tp_stage,
        action_taken,
    }
}

fn command_to_record(command: &ManagementCommand) -> (String, serde_json::Value) {
    match command {
        ManagementCommand::ReplaceSL { cancel_order_id, desired_price, .. } => (
            "replace_sl".into(),
            json!({
                "cancel_order_id": cancel_order_id.to_string(),
                "desired_price": desired_price.to_string(),
            }),
        ),
        ManagementCommand::PartialClose { qty, tp_stage, .. } => (
            "partial_close".into(),
            json!({
                "qty": qty.to_string(),
                "tp_stage": tp_stage,
            }),
        ),
        ManagementCommand::EarlyExit { reason, .. } => (
            "early_exit".into(),
            json!({ "reason": reason }),
        ),
        ManagementCommand::EmergencyClose { reason, .. } => (
            "emergency_close".into(),
            json!({ "reason": reason }),
        ),
    }
}
