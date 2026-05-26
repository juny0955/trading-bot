use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleSide {
    Long,
    Short,
}

/// 포지션 생애주기 (진입~청산 하나의 거래 단위).
#[derive(Debug, Clone)]
pub struct PositionLifecycle {
    pub id: Uuid,
    pub strategy_instance_id: i64,
    pub symbol: String,
    pub side: LifecycleSide,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub entry_avg_price: Option<Decimal>,
    pub exit_avg_price: Option<Decimal>,
    pub total_qty: Option<Decimal>,
    pub realized_pnl: Option<Decimal>,
    pub fee_total: Option<Decimal>,
    pub net_pnl: Option<Decimal>,
    pub mfe_pct: Option<Decimal>,
    pub mae_pct: Option<Decimal>,
    pub entry_atr: Option<Decimal>,
    pub high_water_mark: Option<Decimal>,
    pub low_water_mark: Option<Decimal>,
}

/// 포지션 관리 결정 명령.
///
/// StrategyCommand(진입 전용)와 분리 — 포지션 보유 중 TradeManager가 발행한다.
#[derive(Debug, Clone)]
pub enum ManagementCommand {
    ReplaceSL {
        symbol: String,
        cancel_order_id: Uuid,
        desired_price: Decimal,
    },
    PartialClose {
        symbol: String,
        qty: Decimal,
        tp_stage: u8,
    },
    EarlyExit {
        symbol: String,
        reason: String,
    },
    /// SL 교체 실패로 무보호 시간이 임계값 초과 시 발행.
    EmergencyClose {
        symbol: String,
        reason: String,
    },
}

/// TradeManager 계산용 불변 스냅샷 (tick마다 생성, &로 전달).
#[derive(Debug, Clone)]
pub struct PositionSnapshot {
    pub current_price: Decimal,
    /// (current_price - entry_price) × qty × side_sign — 이미 방향 반영됨
    pub unrealized_pnl: Decimal,
    pub atr: Decimal,
    pub ema200: Decimal,
    pub adx: Decimal,
    pub entry_price: Decimal,
    pub entry_atr: Decimal,
    pub qty: Decimal,
    /// Long / Short — HoldCutDecider EMA 방향 판단에 사용
    pub is_long: bool,
}

/// 거래소/결정 가변 상태 (Arc<RwLock<PositionRuntimeState>>로 관리).
#[derive(Debug, Clone, Default)]
pub struct PositionRuntimeState {
    pub tp_stage: u8,
    pub pending_reduce_order_id: Option<Uuid>,
    pub sl_order_id: Option<Uuid>,
    pub sl_replace_in_flight: bool,
    /// 로깅 전용 — fallback 기준은 unprotected_since
    pub sl_retry_count: u8,
    pub desired_sl_price: Option<Decimal>,
    /// 트레일링 스탑 고점 (메모리 즉시 갱신, DB는 주기적 flush)
    pub high_water_mark: Option<Decimal>,
    /// cancel ACK로 기존 SL 제거된 시점. None이면 SL 보호 중.
    pub unprotected_since: Option<DateTime<Utc>>,
}

/// 결정 시점의 시장 특징 (trade_decisions ML features).
#[derive(Debug, Clone, Default)]
pub struct DecisionMarketContext {
    pub atr: Option<Decimal>,
    /// atr / mid_price × 100
    pub atr_pct: Option<Decimal>,
    pub adx: Option<Decimal>,
    pub spread_pct: Option<Decimal>,
    pub fng_value: Option<i16>,
    /// 1(정배열) / -1(역배열) / 0(중립)
    pub ema_alignment: Option<i16>,
    pub ob_imbalance: Option<Decimal>,
}

/// 결정 시점의 포지션 상태 (trade_decisions ML features).
#[derive(Debug, Clone, Default)]
pub struct DecisionPositionContext {
    pub unrealized_pnl_pct: Option<Decimal>,
    pub time_in_trade_secs: Option<i32>,
    pub dist_to_sl_atr: Option<Decimal>,
    pub dist_to_tp_atr: Option<Decimal>,
    pub tp_stage: Option<i16>,
}

/// lifecycle 매핑 실패 체결 격리 레코드 (unresolved_fills 테이블 대응).
#[derive(Debug, Clone)]
pub struct UnresolvedFill {
    pub id: Uuid,
    pub exchange_trade_id: Option<i64>,
    pub raw_payload: Value,
    pub status: UnresolvedStatus,
    pub reason: Option<String>,
    pub last_error: Option<String>,
    pub retry_count: i16,
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedStatus {
    Pending,
    Resolved,
    Dead,
}
