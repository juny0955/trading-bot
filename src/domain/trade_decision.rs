use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeDecisionStatus {
    Pending,
    Acked,
    Rejected,
    Timeout,
}

impl ExchangeDecisionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Acked => "ACKED",
            Self::Rejected => "REJECTED",
            Self::Timeout => "TIMEOUT",
        }
    }
}

/// trade_decisions 테이블에 insert할 입력 데이터.
#[derive(Debug, Clone)]
pub struct TradeDecisionInsert {
    pub position_lifecycle_id: Uuid,
    pub symbol: String,
    pub decision_type: String,
    // 시장 특징
    pub atr: Option<Decimal>,
    pub atr_pct: Option<Decimal>,
    pub adx: Option<Decimal>,
    pub spread_pct: Option<Decimal>,
    pub fng_value: Option<i16>,
    pub ema_alignment: Option<i16>,
    pub ob_imbalance: Option<Decimal>,
    // 포지션 상태
    pub unrealized_pnl_pct: Option<Decimal>,
    pub time_in_trade_secs: Option<i32>,
    pub dist_to_sl_atr: Option<Decimal>,
    pub dist_to_tp_atr: Option<Decimal>,
    pub tp_stage: Option<i16>,
    // 결정 내용 (command-specific metadata)
    pub action_taken: Value,
}

/// trade_decisions 테이블의 전체 레코드 (OutcomeBackfiller 조회용).
#[derive(Debug, Clone)]
pub struct TradeDecision {
    pub id: Uuid,
    pub position_lifecycle_id: Uuid,
    pub symbol: String,
    pub decision_type: String,
    pub exchange_status: ExchangeDecisionStatus,
    pub pnl_after_5m: Option<Decimal>,
    pub pnl_after_10m: Option<Decimal>,
    pub pnl_after_30m: Option<Decimal>,
    pub max_drawdown_until_close: Option<Decimal>,
    pub decided_at: DateTime<Utc>,
}
