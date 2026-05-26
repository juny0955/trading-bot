use crate::domain::lifecycle::{ManagementCommand, PositionRuntimeState, PositionSnapshot};
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::RwLock;
use uuid::Uuid;

pub type PositionKey = (i64, String); // (strategy_instance_id, symbol)

/// TradeManager 파라미터 (strategy_instances.params JSON에서 로드).
#[derive(Debug, Clone)]
pub struct TradeManagerParams {
    /// 트레일링 스탑 활성화 기준 (entry_atr 배수, 기본 1.0)
    pub trail_activation_atr: Decimal,
    /// 트레일링 스탑 거리 (entry_atr 배수, 기본 1.5)
    pub trail_distance_atr: Decimal,
    /// 단계별 부분 익절: (최소 unrealized_pnl / entry_atr, close_pct)
    pub partial_tp_thresholds: Vec<(Decimal, Decimal)>,
    /// 조기 손절 ATR 팽창 기준 (기본 1.3)
    pub hold_cut_atr_expansion: Decimal,
    /// 최소 평가 간격 초 (기본 1.0)
    pub throttle_secs: f64,
    /// 가격 변화 임계값 배수 — atr_pct × trigger_mult (기본 0.2)
    pub trigger_mult: Decimal,
    /// cancel ACK 후 새 SL 미체결 허용 최대 시간 초 (기본 30)
    pub unprotected_limit_secs: i64,
    /// 지정가 진입 타임아웃 후 시장가 전환 (초, 기본 10)
    pub limit_timeout_secs: u64,
}

impl Default for TradeManagerParams {
    fn default() -> Self {
        Self {
            trail_activation_atr: dec!(1.0),
            trail_distance_atr: dec!(1.5),
            partial_tp_thresholds: vec![(dec!(2.0), dec!(0.3)), (dec!(4.0), dec!(0.3))],
            hold_cut_atr_expansion: dec!(1.3),
            throttle_secs: 1.0,
            trigger_mult: dec!(0.2),
            unprotected_limit_secs: 30,
            limit_timeout_secs: 10,
        }
    }
}

struct TriggerState {
    last_price: Decimal,
    last_eval: Instant,
}

/// 포지션 보유 중 관리 명령을 생성합니다.
///
/// # 실행 흐름
/// LiveTrader가 BookTicker 수신 시 `tick()`을 호출 → 트리거 조건 확인 → 평가 → 명령 반환.
/// 반환된 명령은 LiveTrader가 실행하고, 실행 결과로 상태를 업데이트한다.
pub struct TradeManager {
    /// 포지션별 가변 상태 (tp_stage, sl_order_id 등)
    states: RwLock<HashMap<PositionKey, PositionRuntimeState>>,
    /// 포지션별 트리거 상태 (last_price, last_eval_time)
    triggers: RwLock<HashMap<PositionKey, TriggerState>>,
    /// 포지션별 entry ATR 캐시 (lifecycle open 시 저장, snapshot 빌드에 사용)
    entry_atrs: RwLock<HashMap<PositionKey, Decimal>>,
}

impl TradeManager {
    pub fn new() -> Self {
        Self {
            states: RwLock::new(HashMap::new()),
            triggers: RwLock::new(HashMap::new()),
            entry_atrs: RwLock::new(HashMap::new()),
        }
    }

    // ── 상태 관리 ─────────────────────────────────────────────────────────────

    /// 새 포지션 오픈 시 runtime state를 초기화합니다.
    pub async fn init_position(
        &self,
        key: PositionKey,
        state: PositionRuntimeState,
        entry_atr: Option<Decimal>,
    ) {
        self.states.write().await.insert(key.clone(), state);
        if let Some(atr) = entry_atr {
            self.entry_atrs.write().await.insert(key, atr);
        }
    }

    /// 포지션 종료 시 runtime state를 제거합니다.
    pub async fn remove_position(&self, key: &PositionKey) {
        self.states.write().await.remove(key);
        self.triggers.write().await.remove(key);
        self.entry_atrs.write().await.remove(key);
    }

    /// lifecycle open 후 entry_atr을 설정합니다 (전략 ATR이 확정되면 호출).
    pub async fn set_entry_atr(&self, key: &PositionKey, atr: Decimal) {
        self.entry_atrs.write().await.insert(key.clone(), atr);
    }

    pub async fn get_entry_atr(&self, key: &PositionKey) -> Option<Decimal> {
        self.entry_atrs.read().await.get(key).copied()
    }

    /// PartialClose 주문 제출 후 pending_reduce_order_id를 설정합니다.
    pub async fn set_pending_reduce(&self, key: &PositionKey, order_id: Uuid) {
        if let Some(s) = self.states.write().await.get_mut(key) {
            s.pending_reduce_order_id = Some(order_id);
        }
    }

    /// PartialClose 주문 체결 후 tp_stage를 승격합니다.
    pub async fn advance_tp_stage(&self, key: &PositionKey) {
        if let Some(s) = self.states.write().await.get_mut(key) {
            s.tp_stage += 1;
            s.pending_reduce_order_id = None;
        }
    }

    /// SL 주문 체결 / 갱신 완료 후 호출합니다.
    pub async fn set_sl_order(&self, key: &PositionKey, order_id: Option<Uuid>) {
        if let Some(s) = self.states.write().await.get_mut(key) {
            s.sl_order_id = order_id;
            if order_id.is_some() {
                s.sl_replace_in_flight = false;
                s.unprotected_since = None;
            }
        }
    }

    /// SL cancel ACK 후 무보호 시작 시점을 기록합니다.
    /// 기존 SL이 제거된 시점부터 무보호 타이머가 시작됩니다.
    pub async fn on_sl_cancelled(&self, key: &PositionKey) {
        if let Some(s) = self.states.write().await.get_mut(key) {
            s.sl_order_id = None;
            s.unprotected_since = Some(Utc::now());
        }
    }

    /// ReplaceSL 발행 시 desired_sl_price와 in_flight 상태를 설정합니다.
    pub async fn on_replace_sl_issued(&self, key: &PositionKey, desired_price: Decimal) {
        if let Some(s) = self.states.write().await.get_mut(key) {
            s.sl_replace_in_flight = true;
            s.desired_sl_price = Some(desired_price);
        }
    }

    /// ReplaceSL cancel 실패 시 in_flight를 해제하고 다음 주기에 재시도합니다.
    pub async fn on_replace_sl_cancel_failed(&self, key: &PositionKey) {
        if let Some(s) = self.states.write().await.get_mut(key) {
            s.sl_replace_in_flight = false;
            // desired_sl_price 유지 — 다음 주기에 재시도
        }
    }

    /// SL 교체 재시도 횟수를 증가시킵니다 (로깅 전용).
    pub async fn increment_sl_retry(&self, key: &PositionKey) {
        if let Some(s) = self.states.write().await.get_mut(key) {
            s.sl_retry_count = s.sl_retry_count.saturating_add(1);
        }
    }

    /// high_water_mark를 메모리에서 갱신합니다 (DB flush는 별도 주기).
    pub async fn update_hwm(&self, key: &PositionKey, price: Decimal) {
        if let Some(s) = self.states.write().await.get_mut(key) {
            s.high_water_mark = Some(match s.high_water_mark {
                Some(hwm) => hwm.max(price),
                None => price,
            });
        }
    }

    pub async fn get_state(&self, key: &PositionKey) -> Option<PositionRuntimeState> {
        self.states.read().await.get(key).cloned()
    }

    // ── 평가 ──────────────────────────────────────────────────────────────────

    /// BookTicker 수신마다 호출합니다.
    ///
    /// - 트리거 조건 미충족 → `None`
    /// - 충족 → `Some(commands)` (빈 Vec이면 현재 조건에서 발행할 명령 없음)
    pub async fn tick(
        &self,
        key: PositionKey,
        snapshot: &PositionSnapshot,
        params: &TradeManagerParams,
    ) -> Option<Vec<ManagementCommand>> {
        if !self.should_evaluate(&key, snapshot, params).await {
            return None;
        }
        Some(self.evaluate(&key, snapshot, params).await)
    }

    async fn should_evaluate(
        &self,
        key: &PositionKey,
        snapshot: &PositionSnapshot,
        params: &TradeManagerParams,
    ) -> bool {
        let mut triggers = self.triggers.write().await;
        let now = Instant::now();
        let mid = snapshot.current_price;

        let atr_pct = if !mid.is_zero() {
            snapshot.entry_atr / mid
        } else {
            Decimal::ZERO
        };

        // 트리거 임계값: max(5bp, atr_pct × trigger_mult)
        let threshold = (atr_pct * params.trigger_mult).max(dec!(0.0005));

        match triggers.get_mut(key) {
            Some(ts) => {
                let elapsed = now.duration_since(ts.last_eval).as_secs_f64();
                let price_change = if !ts.last_price.is_zero() {
                    ((mid - ts.last_price) / ts.last_price).abs()
                } else {
                    Decimal::ONE
                };
                if price_change >= threshold || elapsed >= params.throttle_secs {
                    ts.last_price = mid;
                    ts.last_eval = now;
                    true
                } else {
                    false
                }
            }
            None => {
                triggers.insert(
                    key.clone(),
                    TriggerState {
                        last_price: mid,
                        last_eval: now,
                    },
                );
                true
            }
        }
    }

    async fn evaluate(
        &self,
        key: &PositionKey,
        snapshot: &PositionSnapshot,
        params: &TradeManagerParams,
    ) -> Vec<ManagementCommand> {
        let symbol = &key.1;
        let mut commands = Vec::new();
        let states = self.states.read().await;
        let Some(runtime) = states.get(key) else {
            return commands;
        };

        // 1. PartialTP
        if let Some(cmd) = eval_partial_tp(symbol, snapshot, runtime, params) {
            commands.push(cmd);
        }

        // 2. TrailingStop
        if let Some(cmd) = eval_trailing_stop(symbol, snapshot, runtime, params) {
            commands.push(cmd);
        }

        // 3. HoldCutDecider
        if let Some(cmd) = eval_hold_cut(symbol, snapshot, runtime, params) {
            commands.push(cmd);
        }

        // 4. SLUpdater (EmergencyClose 확인)
        if let Some(cmd) = eval_sl_emergency(symbol, runtime, params) {
            commands.push(cmd);
        }

        commands
    }
}

impl Default for TradeManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── PartialTP ─────────────────────────────────────────────────────────────────

fn eval_partial_tp(
    symbol: &str,
    snapshot: &PositionSnapshot,
    runtime: &PositionRuntimeState,
    params: &TradeManagerParams,
) -> Option<ManagementCommand> {
    // 미체결 reduce-only 주문 있으면 중복 방지
    if runtime.pending_reduce_order_id.is_some() {
        return None;
    }

    let entry_atr = snapshot.entry_atr;
    let unrealized = snapshot.unrealized_pnl;
    let qty = snapshot.qty;

    for (i, &(atr_mult, close_pct)) in params.partial_tp_thresholds.iter().enumerate() {
        let stage = i as u8;
        if runtime.tp_stage != stage {
            continue;
        }
        // threshold = atr_mult × entry_atr × qty
        // unrealized_pnl이 이미 qty가 반영된 절대값이므로 (price_diff × qty)
        let threshold = atr_mult * entry_atr * qty;
        if unrealized >= threshold {
            return Some(ManagementCommand::PartialClose {
                symbol: symbol.to_string(),
                qty: qty * close_pct,
                tp_stage: stage + 1,
            });
        }
    }
    None
}

// ── SLUpdater (EmergencyClose 트리거) ─────────────────────────────────────────

fn eval_sl_emergency(
    symbol: &str,
    runtime: &PositionRuntimeState,
    params: &TradeManagerParams,
) -> Option<ManagementCommand> {
    // sl_order_id == None AND unprotected_since 설정된 상태
    if runtime.sl_order_id.is_some() {
        return None;
    }
    let since = runtime.unprotected_since?;
    let elapsed_secs = (Utc::now() - since).num_seconds();
    if elapsed_secs > params.unprotected_limit_secs {
        Some(ManagementCommand::EmergencyClose {
            symbol: symbol.to_string(),
            reason: format!(
                "SL 무보호 {}초 초과 (limit={}s)",
                elapsed_secs, params.unprotected_limit_secs
            ),
        })
    } else {
        None
    }
}

// ── TrailingStop ──────────────────────────────────────────────────────────────

fn eval_trailing_stop(
    symbol: &str,
    snapshot: &PositionSnapshot,
    runtime: &PositionRuntimeState,
    params: &TradeManagerParams,
) -> Option<ManagementCommand> {
    // SL 교체 진행 중이면 대기
    if runtime.sl_replace_in_flight {
        return None;
    }
    let sl_order_id = runtime.sl_order_id?;
    let hwm = runtime.high_water_mark?;

    // 트레일링 활성화 조건
    let activation_threshold = params.trail_activation_atr * snapshot.entry_atr * snapshot.qty;
    if snapshot.unrealized_pnl < activation_threshold {
        return None;
    }

    // 롱 기준: desired_sl = hwm - trail_distance_atr × entry_atr
    let desired_sl = hwm - params.trail_distance_atr * snapshot.entry_atr;

    // 현재 SL보다 유리하게 갱신할 때만 ReplaceSL 발행
    // desired_sl이 current_sl보다 높아야 함 (롱 기준)
    // current_sl은 런타임 상태에 없으므로 desired_sl_price 필드로 비교
    let current_sl = runtime.desired_sl_price.unwrap_or(Decimal::ZERO);
    if desired_sl <= current_sl {
        return None;
    }

    Some(ManagementCommand::ReplaceSL {
        symbol: symbol.to_string(),
        cancel_order_id: sl_order_id,
        desired_price: desired_sl,
    })
}

// ── HoldCutDecider ───────────────────────────────────────────────────────────

fn eval_hold_cut(
    symbol: &str,
    snapshot: &PositionSnapshot,
    runtime: &PositionRuntimeState,
    params: &TradeManagerParams,
) -> Option<ManagementCommand> {
    let _ = runtime;

    // 1차 조건: 현재 ATR이 진입 시 ATR보다 크게 팽창
    if snapshot.atr <= snapshot.entry_atr * params.hold_cut_atr_expansion {
        return None;
    }

    // 가격이 포지션 반대 방향으로 이동 중인지
    if snapshot.unrealized_pnl >= Decimal::ZERO {
        return None;
    }

    // 확인 필터: EMA200을 반대 방향으로 돌파
    let ema_broken = if snapshot.is_long {
        snapshot.current_price < snapshot.ema200 // 롱: EMA200 하향 돌파
    } else {
        snapshot.current_price > snapshot.ema200 // 숏: EMA200 상향 돌파
    };

    if !ema_broken {
        return None;
    }

    Some(ManagementCommand::EarlyExit {
        symbol: symbol.to_string(),
        reason: format!(
            "ATR 팽창({:.4} > {:.4}×{:.4}) + EMA200 돌파",
            snapshot.atr, params.hold_cut_atr_expansion, snapshot.entry_atr
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn snapshot(unrealized_pnl: Decimal, entry_atr: Decimal, qty: Decimal) -> PositionSnapshot {
        PositionSnapshot {
            current_price: dec!(50000),
            unrealized_pnl,
            atr: entry_atr,
            ema200: dec!(49000),
            adx: dec!(25),
            entry_price: dec!(48000),
            entry_atr,
            qty,
            is_long: true,
        }
    }

    #[test]
    fn partial_tp_stage0_triggers_at_2x_atr() {
        let snap = snapshot(dec!(200), dec!(50), dec!(1)); // 200 >= 2 × 50 × 1 = 100
        let runtime = PositionRuntimeState::default(); // tp_stage = 0
        let params = TradeManagerParams::default();
        let cmd = eval_partial_tp("BTCUSDT", &snap, &runtime, &params);
        assert!(matches!(
            cmd,
            Some(ManagementCommand::PartialClose { tp_stage: 1, .. })
        ));
    }

    #[test]
    fn partial_tp_stage0_no_trigger_below_threshold() {
        let snap = snapshot(dec!(80), dec!(50), dec!(1)); // 80 < 100
        let runtime = PositionRuntimeState::default();
        let params = TradeManagerParams::default();
        let cmd = eval_partial_tp("BTCUSDT", &snap, &runtime, &params);
        assert!(cmd.is_none());
    }

    #[test]
    fn partial_tp_skipped_when_pending_reduce() {
        let snap = snapshot(dec!(200), dec!(50), dec!(1));
        let runtime = PositionRuntimeState {
            pending_reduce_order_id: Some(Uuid::nil()),
            ..Default::default()
        };
        let params = TradeManagerParams::default();
        let cmd = eval_partial_tp("BTCUSDT", &snap, &runtime, &params);
        assert!(cmd.is_none());
    }

    #[test]
    fn partial_tp_stage1_triggers_at_4x_atr() {
        let snap = snapshot(dec!(400), dec!(50), dec!(1)); // 400 >= 4 × 50 × 1 = 200
        let runtime = PositionRuntimeState {
            tp_stage: 1,
            ..Default::default()
        };
        let params = TradeManagerParams::default();
        let cmd = eval_partial_tp("BTCUSDT", &snap, &runtime, &params);
        assert!(matches!(
            cmd,
            Some(ManagementCommand::PartialClose { tp_stage: 2, .. })
        ));
    }
}
