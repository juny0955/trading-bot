use crate::domain::backtest::{Bar, DepthSnapshot};
use crate::domain::order::OrderSide;
use crate::domain::strategies::{Context, Strategy, StrategyCommand};
use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::str::FromStr;

// ──────────────────────────────────────────────
// Indicator calculators (Serialize/Deserialize for future state persistence)
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EmaCalc {
    k: Decimal,
    value: Option<Decimal>,
    bars: usize,
    period: usize,
}

impl EmaCalc {
    fn new(period: usize) -> Self {
        Self {
            k: dec!(2) / Decimal::from(period + 1),
            value: None,
            bars: 0,
            period,
        }
    }

    fn next(&mut self, price: Decimal) {
        self.value = Some(match self.value {
            Some(ema) => ema + self.k * (price - ema),
            None => price,
        });
        self.bars += 1;
    }

    fn value(&self) -> Option<Decimal> {
        if self.bars >= self.period {
            self.value
        } else {
            None
        }
    }
}

/// ATR with Wilder's smoothing (factor = 1/n, not 2/(n+1))
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AtrCalc {
    period: usize,
    prev_close: Option<Decimal>,
    smooth_tr: Option<Decimal>,
    bars: usize,
    init_sum: Decimal,
}

impl AtrCalc {
    fn new(period: usize) -> Self {
        Self {
            period,
            prev_close: None,
            smooth_tr: None,
            bars: 0,
            init_sum: Decimal::ZERO,
        }
    }

    fn next(&mut self, bar: &Bar) {
        let tr = true_range(bar.high, bar.low, self.prev_close);
        self.prev_close = Some(bar.close);
        self.bars += 1;
        let n = Decimal::from(self.period);

        if self.bars < self.period {
            self.init_sum += tr;
        } else if self.bars == self.period {
            self.init_sum += tr;
            self.smooth_tr = Some(self.init_sum / n);
        } else {
            // Wilder: S_n = S_{n-1} - S_{n-1}/n + tr
            let prev = self.smooth_tr.unwrap_or_default();
            self.smooth_tr = Some(prev - prev / n + tr);
        }
    }

    fn value(&self) -> Option<Decimal> {
        if self.bars >= self.period {
            self.smooth_tr
        } else {
            None
        }
    }
}

/// ADX using Wilder's 1978 formula with Wilder smoothing
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AdxCalc {
    period: usize,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
    prev_close: Option<Decimal>,
    smooth_tr: Option<Decimal>,
    smooth_plus_dm: Option<Decimal>,
    smooth_minus_dm: Option<Decimal>,
    adx: Option<Decimal>,
    bars: usize,
    init_tr: Decimal,
    init_plus_dm: Decimal,
    init_minus_dm: Decimal,
    dx_ema: Option<Decimal>,
    dx_bars: usize,
}

impl AdxCalc {
    fn new(period: usize) -> Self {
        Self {
            period,
            prev_high: None,
            prev_low: None,
            prev_close: None,
            smooth_tr: None,
            smooth_plus_dm: None,
            smooth_minus_dm: None,
            adx: None,
            bars: 0,
            init_tr: Decimal::ZERO,
            init_plus_dm: Decimal::ZERO,
            init_minus_dm: Decimal::ZERO,
            dx_ema: None,
            dx_bars: 0,
        }
    }

    fn next(&mut self, bar: &Bar) {
        let tr = true_range(bar.high, bar.low, self.prev_close);
        let (plus_dm, minus_dm) =
            directional_movement(bar.high, bar.low, self.prev_high, self.prev_low);

        self.prev_high = Some(bar.high);
        self.prev_low = Some(bar.low);
        self.prev_close = Some(bar.close);
        self.bars += 1;

        let n = Decimal::from(self.period);

        if self.bars < self.period {
            self.init_tr += tr;
            self.init_plus_dm += plus_dm;
            self.init_minus_dm += minus_dm;
            return;
        }
        if self.bars == self.period {
            self.init_tr += tr;
            self.init_plus_dm += plus_dm;
            self.init_minus_dm += minus_dm;
            self.smooth_tr = Some(self.init_tr);
            self.smooth_plus_dm = Some(self.init_plus_dm);
            self.smooth_minus_dm = Some(self.init_minus_dm);
        } else {
            let s_tr = self.smooth_tr.unwrap_or_default();
            let s_p = self.smooth_plus_dm.unwrap_or_default();
            let s_m = self.smooth_minus_dm.unwrap_or_default();
            self.smooth_tr = Some(s_tr - s_tr / n + tr);
            self.smooth_plus_dm = Some(s_p - s_p / n + plus_dm);
            self.smooth_minus_dm = Some(s_m - s_m / n + minus_dm);
        }

        let s_tr = self.smooth_tr.unwrap_or_default();
        if s_tr.is_zero() {
            return;
        }
        let plus_di = dec!(100) * self.smooth_plus_dm.unwrap_or_default() / s_tr;
        let minus_di = dec!(100) * self.smooth_minus_dm.unwrap_or_default() / s_tr;
        let di_sum = plus_di + minus_di;
        if di_sum.is_zero() {
            return;
        }
        let dx = dec!(100) * (plus_di - minus_di).abs() / di_sum;

        self.dx_bars += 1;
        let adx = match self.dx_ema {
            None => dx,
            // Wilder smoothing on DX
            Some(prev) => prev - prev / n + dx,
        };
        self.dx_ema = Some(adx);
        self.adx = Some(adx);
    }

    fn value(&self) -> Option<Decimal> {
        // ADX needs 2×period bars to be meaningful (one period to init DM/TR, one to smooth DX)
        if self.dx_bars >= self.period {
            self.adx
        } else {
            None
        }
    }
}

fn true_range(high: Decimal, low: Decimal, prev_close: Option<Decimal>) -> Decimal {
    let hl = high - low;
    match prev_close {
        None => hl,
        Some(pc) => hl.max((high - pc).abs()).max((low - pc).abs()),
    }
}

fn directional_movement(
    high: Decimal,
    low: Decimal,
    prev_high: Option<Decimal>,
    prev_low: Option<Decimal>,
) -> (Decimal, Decimal) {
    let (ph, pl) = match (prev_high, prev_low) {
        (Some(ph), Some(pl)) => (ph, pl),
        _ => return (Decimal::ZERO, Decimal::ZERO),
    };
    let up = high - ph;
    let down = pl - low;
    let plus_dm = if up > down && up > Decimal::ZERO {
        up
    } else {
        Decimal::ZERO
    };
    let minus_dm = if down > up && down > Decimal::ZERO {
        down
    } else {
        Decimal::ZERO
    };
    (plus_dm, minus_dm)
}

// ──────────────────────────────────────────────
// FilterConfig
// ──────────────────────────────────────────────

/// MarketFilter 동작을 제어하는 파라미터입니다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterConfig {
    pub atr_period: usize,
    pub adx_period: usize,
    pub ema_trend_period: usize,
    pub adx_threshold: Decimal,
    pub sl_atr_mult: Decimal,
    pub tp_atr_mult: Decimal,
    pub base_risk_pct: Decimal,
    pub risk_scale_min: Decimal,
    pub risk_scale_max: Decimal,
    pub fear_greed_lo: u8,
    pub fear_greed_hi: u8,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            atr_period: 14,
            adx_period: 14,
            ema_trend_period: 200,
            adx_threshold: dec!(20),
            sl_atr_mult: dec!(1.5),
            tp_atr_mult: dec!(3.0),
            base_risk_pct: dec!(0.01),
            risk_scale_min: dec!(0.5),
            risk_scale_max: dec!(1.5),
            fear_greed_lo: 20,
            fear_greed_hi: 80,
        }
    }
}

impl FilterConfig {
    pub fn from_params(v: &Value) -> Result<Self> {
        let d = Self::default();
        Ok(Self {
            atr_period: parse_usize_field(v, "atr_period", d.atr_period)?,
            adx_period: parse_usize_field(v, "adx_period", d.adx_period)?,
            ema_trend_period: parse_usize_field(v, "ema_trend_period", d.ema_trend_period)?,
            adx_threshold: parse_dec_field(v, "adx_threshold", d.adx_threshold)?,
            sl_atr_mult: parse_dec_field(v, "sl_atr_mult", d.sl_atr_mult)?,
            tp_atr_mult: parse_dec_field(v, "tp_atr_mult", d.tp_atr_mult)?,
            base_risk_pct: parse_dec_field(v, "base_risk_pct", d.base_risk_pct)?,
            risk_scale_min: parse_dec_field(v, "risk_scale_min", d.risk_scale_min)?,
            risk_scale_max: parse_dec_field(v, "risk_scale_max", d.risk_scale_max)?,
            fear_greed_lo: parse_u8_field(v, "fear_greed_lo", d.fear_greed_lo)?,
            fear_greed_hi: parse_u8_field(v, "fear_greed_hi", d.fear_greed_hi)?,
        })
    }
}

fn parse_dec_field(v: &Value, key: &str, default: Decimal) -> Result<Decimal> {
    match v.get(key) {
        None => Ok(default),
        Some(Value::String(s)) => Ok(Decimal::from_str(s)?),
        Some(Value::Number(n)) => Ok(Decimal::from_str(&n.to_string())?),
        Some(other) => anyhow::bail!("{key}: 숫자 또는 문자열이어야 합니다. 받은 값: {other}"),
    }
}

fn parse_usize_field(v: &Value, key: &str, default: usize) -> Result<usize> {
    match v.get(key) {
        None => Ok(default),
        Some(Value::Number(n)) => n
            .as_u64()
            .map(|x| x as usize)
            .ok_or_else(|| anyhow::anyhow!("{key}: 양수 정수여야 합니다")),
        Some(other) => anyhow::bail!("{key}: 정수여야 합니다. 받은 값: {other}"),
    }
}

fn parse_u8_field(v: &Value, key: &str, default: u8) -> Result<u8> {
    match v.get(key) {
        None => Ok(default),
        Some(Value::Number(n)) => n
            .as_u64()
            .map(|x| x as u8)
            .ok_or_else(|| anyhow::anyhow!("{key}: 0–255 범위의 정수여야 합니다")),
        Some(other) => anyhow::bail!("{key}: 정수여야 합니다. 받은 값: {other}"),
    }
}

// ──────────────────────────────────────────────
// MarketFilter
// ──────────────────────────────────────────────

/// 기존 전략을 감싸서 장세 필터링, 리스크 기반 포지션 사이징, ATR 기반 TP/SL을 주입하는 래퍼입니다.
///
/// 필터 계층 (순서대로):
/// 1. Fear & Greed 극단값 차단
/// 2. ADX 추세 강도 미달 차단
/// 3. EMA 방향과 반대 신호 차단
///
/// 진입 통과 시:
/// - qty를 리스크 기반으로 교체 (`equity × adjusted_risk% / sl_distance`)
/// - SetStopLoss, SetTakeProfit 명령 추가
pub struct MarketFilter {
    inner: Box<dyn Strategy>,
    config: FilterConfig,
    atr: AtrCalc,
    adx: AdxCalc,
    trend_ema: EmaCalc,
}

impl MarketFilter {
    pub fn new(inner: Box<dyn Strategy>, config: FilterConfig) -> Self {
        let atr = AtrCalc::new(config.atr_period);
        let adx = AdxCalc::new(config.adx_period);
        let trend_ema = EmaCalc::new(config.ema_trend_period);
        Self {
            inner,
            config,
            atr,
            adx,
            trend_ema,
        }
    }

    fn warmed_up(&self) -> bool {
        self.atr.value().is_some() && self.adx.value().is_some()
    }

    fn is_entry_allowed(&self, bar: &Bar, side: OrderSide, ctx: &Context) -> bool {
        // Layer 1: Macro — Fear & Greed extremes
        if let Some(fg) = ctx.fear_greed
            && (fg < self.config.fear_greed_lo || fg > self.config.fear_greed_hi)
        {
            return false;
        }

        // Layer 2: Trend strength — ADX threshold (0이면 필터 비활성)
        if self.config.adx_threshold > Decimal::ZERO {
            let Some(adx) = self.adx.value() else {
                return false;
            };
            if adx < self.config.adx_threshold {
                return false;
            }
        }

        // Layer 3: Direction — EMA trend filter (skip if EMA not yet warmed up)
        if let Some(ema) = self.trend_ema.value() {
            match side {
                OrderSide::Buy if bar.close < ema => return false,
                OrderSide::Sell if bar.close > ema => return false,
                _ => {}
            }
        }

        true
    }

    fn calc_qty(&self, equity: Decimal, atr: Decimal, adx: Decimal) -> Decimal {
        if atr.is_zero() || equity.is_zero() {
            return Decimal::ZERO;
        }
        let scale = if self.config.adx_threshold.is_zero() {
            self.config.risk_scale_max
        } else {
            (adx / self.config.adx_threshold)
                .max(self.config.risk_scale_min)
                .min(self.config.risk_scale_max)
        };
        let risk_pct = self.config.base_risk_pct * scale;
        let sl_distance = atr * self.config.sl_atr_mult;
        (equity * risk_pct) / sl_distance
    }

    fn tp_sl_commands(
        &self,
        bar: &Bar,
        side: OrderSide,
        qty: Decimal,
        atr: Decimal,
    ) -> Vec<StrategyCommand> {
        let sl_dist = atr * self.config.sl_atr_mult;
        let tp_dist = atr * self.config.tp_atr_mult;
        let (sl_price, tp_price) = match side {
            OrderSide::Buy => (bar.close - sl_dist, bar.close + tp_dist),
            OrderSide::Sell => (bar.close + sl_dist, bar.close - tp_dist),
        };
        let exit_side = match side {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
        };
        vec![
            StrategyCommand::SetStopLoss {
                side: exit_side,
                qty,
                stop_price: sl_price,
            },
            StrategyCommand::SetTakeProfit {
                side: exit_side,
                qty,
                stop_price: tp_price,
            },
        ]
    }
}

impl Strategy for MarketFilter {
    fn on_start(&mut self, ctx: &Context) -> Result<Vec<StrategyCommand>> {
        self.inner.on_start(ctx)
    }

    fn on_bar(&mut self, bar: &Bar, ctx: &Context) -> Result<Vec<StrategyCommand>> {
        self.atr.next(bar);
        self.adx.next(bar);
        self.trend_ema.next(bar.close);

        let inner_cmds = self.inner.on_bar(bar, ctx)?;

        if !self.warmed_up() {
            // reduce_only(청산) 주문만 통과, 진입 신호는 차단
            return Ok(inner_cmds
                .into_iter()
                .filter(|c| matches!(c, StrategyCommand::SubmitOrder(r) if r.reduce_only))
                .collect());
        }

        let atr = self.atr.value().unwrap_or_default();
        let adx = self.adx.value().unwrap_or_default();

        // inner가 이미 SetSL/SetTP를 발행했으면 중복 추가 안 함
        let has_sl = inner_cmds
            .iter()
            .any(|c| matches!(c, StrategyCommand::SetStopLoss { .. }));
        let has_tp = inner_cmds
            .iter()
            .any(|c| matches!(c, StrategyCommand::SetTakeProfit { .. }));

        let mut result = vec![];
        for cmd in inner_cmds {
            match cmd {
                StrategyCommand::SubmitOrder(mut req) if !req.reduce_only => {
                    if !self.is_entry_allowed(bar, req.side, ctx) {
                        continue;
                    }
                    let qty = self.calc_qty(ctx.equity, atr, adx);
                    if qty <= Decimal::ZERO {
                        continue;
                    }
                    let side = req.side;
                    req.qty = qty;
                    result.push(StrategyCommand::SubmitOrder(req));
                    if !has_sl && !has_tp {
                        result.extend(self.tp_sl_commands(bar, side, qty, atr));
                    }
                }
                other => result.push(other),
            }
        }
        Ok(result)
    }

    fn on_depth(&mut self, snap: &DepthSnapshot, ctx: &Context) -> Result<Vec<StrategyCommand>> {
        self.inner.on_depth(snap, ctx)
    }

    fn on_finish(&mut self, ctx: &Context) -> Result<Vec<StrategyCommand>> {
        self.inner.on_finish(ctx)
    }
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::backtest::Bar;
    use crate::domain::order::{OrderRequest, OrderSide, OrderType};
    use crate::domain::position::Position;
    use crate::domain::strategies::{Context, Strategy, StrategyCommand};
    use anyhow::Result;
    use rust_decimal_macros::dec;

    struct AlwaysBuy {
        qty: Decimal,
    }
    impl Strategy for AlwaysBuy {
        fn on_bar(&mut self, bar: &Bar, _ctx: &Context) -> Result<Vec<StrategyCommand>> {
            Ok(vec![StrategyCommand::SubmitOrder(OrderRequest {
                symbol: bar.symbol.clone(),
                order_type: OrderType::Market,
                side: OrderSide::Buy,
                qty: self.qty,
                reduce_only: false,
                ..Default::default()
            })])
        }
    }

    struct AlwaysClose;
    impl Strategy for AlwaysClose {
        fn on_bar(&mut self, bar: &Bar, _ctx: &Context) -> Result<Vec<StrategyCommand>> {
            Ok(vec![StrategyCommand::SubmitOrder(OrderRequest {
                symbol: bar.symbol.clone(),
                order_type: OrderType::Market,
                side: OrderSide::Sell,
                qty: dec!(1),
                reduce_only: true,
                ..Default::default()
            })])
        }
    }

    fn make_bar(close: Decimal) -> Bar {
        Bar {
            symbol: "BTCUSDT".into(),
            ts_ns: 1,
            open: close,
            high: close + dec!(50),
            low: close - dec!(50),
            close,
            volume: dec!(100),
        }
    }

    fn make_ctx(equity: Decimal) -> Context {
        Context {
            now_ns: 1,
            equity,
            position: Position::default(),
            fear_greed: None,
        }
    }

    #[test]
    fn blocks_entry_before_warmup() {
        let mut filter = MarketFilter::new(
            Box::new(AlwaysBuy { qty: dec!(1) }),
            FilterConfig::default(),
        );
        let ctx = make_ctx(dec!(10000));
        let cmds = filter.on_bar(&make_bar(dec!(50000)), &ctx).unwrap();
        assert!(cmds.is_empty(), "워밍업 전 진입 신호는 차단되어야 함");
    }

    #[test]
    fn passes_reduce_only_before_warmup() {
        let mut filter = MarketFilter::new(Box::new(AlwaysClose), FilterConfig::default());
        let ctx = make_ctx(dec!(10000));
        let cmds = filter.on_bar(&make_bar(dec!(50000)), &ctx).unwrap();
        assert_eq!(
            cmds.len(),
            1,
            "reduce_only 주문은 워밍업 전에도 통과해야 함"
        );
    }

    #[test]
    fn qty_formula_is_correct() {
        // equity=10000, risk=1%, sl_mult=2.0, adx=adx_threshold → scale=1.0
        // sl_distance = 100 * 2.0 = 200
        // qty = 10000 * 0.01 / 200 = 0.5
        let config = FilterConfig {
            base_risk_pct: dec!(0.01),
            sl_atr_mult: dec!(2.0),
            adx_threshold: dec!(25),
            risk_scale_min: dec!(0.5),
            risk_scale_max: dec!(1.5),
            ..FilterConfig::default()
        };
        let filter = MarketFilter::new(Box::new(AlwaysBuy { qty: dec!(999) }), config);
        let qty = filter.calc_qty(dec!(10000), dec!(100), dec!(25));
        assert_eq!(qty, dec!(0.5));
    }

    #[test]
    fn qty_scales_with_adx() {
        // adx=50, threshold=25 → scale = 50/25 = 2.0 → clamped to risk_scale_max=1.5
        let config = FilterConfig {
            base_risk_pct: dec!(0.01),
            sl_atr_mult: dec!(1.0),
            adx_threshold: dec!(25),
            risk_scale_min: dec!(0.5),
            risk_scale_max: dec!(1.5),
            ..FilterConfig::default()
        };
        let filter = MarketFilter::new(Box::new(AlwaysBuy { qty: dec!(999) }), config);
        // scale = clamp(50/25, 0.5, 1.5) = 1.5
        // qty = 10000 * 0.01 * 1.5 / (100 * 1.0) = 1.5
        let qty = filter.calc_qty(dec!(10000), dec!(100), dec!(50));
        assert_eq!(qty, dec!(1.5));
    }

    #[test]
    fn tp_sl_prices_are_symmetric_for_buy() {
        let config = FilterConfig {
            sl_atr_mult: dec!(1.5),
            tp_atr_mult: dec!(3.0),
            ..FilterConfig::default()
        };
        let filter = MarketFilter::new(Box::new(AlwaysBuy { qty: dec!(1) }), config);
        let bar = make_bar(dec!(50000));
        let cmds = filter.tp_sl_commands(&bar, OrderSide::Buy, dec!(0.1), dec!(100));
        assert_eq!(cmds.len(), 2);
        match &cmds[0] {
            StrategyCommand::SetStopLoss {
                stop_price, side, ..
            } => {
                assert_eq!(*stop_price, dec!(49850)); // 50000 - 1.5*100
                assert_eq!(*side, OrderSide::Sell);
            }
            _ => panic!("첫 번째 명령은 SetStopLoss여야 함"),
        }
        match &cmds[1] {
            StrategyCommand::SetTakeProfit {
                stop_price, side, ..
            } => {
                assert_eq!(*stop_price, dec!(50300)); // 50000 + 3.0*100
                assert_eq!(*side, OrderSide::Sell);
            }
            _ => panic!("두 번째 명령은 SetTakeProfit이어야 함"),
        }
    }

    #[test]
    fn fear_greed_filter_blocks_extremes() {
        // ADX, EMA 필터를 비활성화하고 fear_greed만 테스트
        let config = FilterConfig {
            adx_threshold: dec!(0),    // ADX 필터 비활성
            ema_trend_period: 1,       // EMA 즉시 워밍업
            ..FilterConfig::default()  // fear_greed_lo=20, hi=80
        };
        let filter = MarketFilter::new(Box::new(AlwaysBuy { qty: dec!(1) }), config);
        let bar = make_bar(dec!(50000));

        let mut ctx_extreme = make_ctx(dec!(10000));
        ctx_extreme.fear_greed = Some(85); // > 80 → 차단
        assert!(!filter.is_entry_allowed(&bar, OrderSide::Buy, &ctx_extreme));

        let mut ctx_normal = make_ctx(dec!(10000));
        ctx_normal.fear_greed = Some(50); // 정상 범위
        assert!(filter.is_entry_allowed(&bar, OrderSide::Buy, &ctx_normal));
    }

    #[test]
    fn true_range_without_prev_close() {
        // TR = high - low when no prev_close
        assert_eq!(true_range(dec!(110), dec!(90), None), dec!(20));
    }

    #[test]
    fn true_range_with_gap() {
        // high-low=10, high-prev=5, low-prev=15 → TR=15
        assert_eq!(true_range(dec!(105), dec!(95), Some(dec!(100))), dec!(10));
        // Gap up: prev=90, high=110, low=100 → hc=20, lc=10, hl=10 → TR=20
        assert_eq!(true_range(dec!(110), dec!(100), Some(dec!(90))), dec!(20));
    }

    #[test]
    fn filter_config_defaults_on_empty_params() {
        let config = FilterConfig::from_params(&serde_json::json!({})).unwrap();
        assert_eq!(config.atr_period, 14);
        assert_eq!(config.adx_threshold, dec!(20));
        assert_eq!(config.sl_atr_mult, dec!(1.5));
        assert_eq!(config.fear_greed_lo, 20);
    }

    #[test]
    fn filter_config_parses_string_decimals() {
        let params = serde_json::json!({
            "sl_atr_mult": "2.5",
            "tp_atr_mult": "5.0",
            "base_risk_pct": "0.02",
        });
        let config = FilterConfig::from_params(&params).unwrap();
        assert_eq!(config.sl_atr_mult, dec!(2.5));
        assert_eq!(config.tp_atr_mult, dec!(5.0));
        assert_eq!(config.base_risk_pct, dec!(0.02));
    }
}
