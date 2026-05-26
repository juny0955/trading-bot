use crate::domain::backtest::Bar;
use crate::domain::order::OrderSide;
use crate::domain::strategies::{Context, Strategy, StrategyCommand};
use anyhow::{Result, anyhow};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;
use serde_json::Value;
use std::str::FromStr;

#[derive(Deserialize)]
struct Params {
    fast: usize,
    slow: usize,
    qty: String,
}

/// 빠른 EMA와 느린 EMA의 교차로 진입 방향을 결정하는 전략입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::strategies::ema_cross::EmaCross;
///
/// let strategy = EmaCross::new(9, 21, dec!(0.001)).unwrap();
/// assert_eq!(strategy.slow_period(), 21);
/// ```
pub struct EmaCross {
    slow_n: usize,
    qty: Decimal,
    fast_k: Decimal,
    slow_k: Decimal,
    fast_ema: Option<Decimal>,
    slow_ema: Option<Decimal>,
    prev_diff: Option<Decimal>,
    bars_seen: usize,
}

impl EmaCross {
    /// EMA 교차 전략을 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::domain::strategies::ema_cross::EmaCross;
    ///
    /// let strategy = EmaCross::new(12, 26, dec!(1)).unwrap();
    /// assert_eq!(strategy.slow_period(), 26);
    /// ```
    pub fn new(fast: usize, slow: usize, qty: Decimal) -> Result<Self> {
        validate_config(fast, slow, qty)?;

        Ok(Self {
            slow_n: slow,
            qty,
            fast_k: Self::smoothing(fast),
            slow_k: Self::smoothing(slow),
            fast_ema: None,
            slow_ema: None,
            prev_diff: None,
            bars_seen: 0,
        })
    }

    /// 느린 EMA 기간을 반환합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::domain::strategies::ema_cross::EmaCross;
    ///
    /// assert_eq!(EmaCross::new(5, 20, dec!(1)).unwrap().slow_period(), 20);
    /// ```
    pub fn slow_period(&self) -> usize {
        self.slow_n
    }

    /// JSON 파라미터에서 전략을 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use serde_json::json;
    /// use trading_bot::domain::strategies::ema_cross::EmaCross;
    ///
    /// let params = json!({"fast": 9, "slow": 21, "qty": "0.001"});
    /// assert!(EmaCross::from_params(&params).is_ok());
    /// ```
    pub fn from_params(v: &Value) -> Result<Self> {
        let p: Params = serde_json::from_value(v.clone())?;
        let qty = parse_positive_qty(&p.qty)?;
        Self::new(p.fast, p.slow, qty)
    }

    fn smoothing(n: usize) -> Decimal {
        dec!(2) / Decimal::from(n + 1)
    }

    fn update_emas(&mut self, price: Decimal) {
        self.fast_ema = Some(next_ema(self.fast_ema, self.fast_k, price));
        self.slow_ema = Some(next_ema(self.slow_ema, self.slow_k, price));
        self.bars_seen += 1;
    }

    fn is_warmed_up(&self) -> bool {
        self.bars_seen >= self.slow_n
    }

    fn current_diff(&self) -> Option<Decimal> {
        Some(self.fast_ema? - self.slow_ema?)
    }

    fn commands_for_diff(&self, bar: &Bar, ctx: &Context, diff: Decimal) -> Vec<StrategyCommand> {
        match cross_signal(self.prev_diff, diff) {
            Some(side) => Self::close_and_open(&ctx.position, &bar.symbol, side, self.qty),
            None => vec![],
        }
    }
}

impl Strategy for EmaCross {
    fn on_bar(&mut self, bar: &Bar, ctx: &Context) -> Result<Vec<StrategyCommand>> {
        self.update_emas(bar.close);

        if !self.is_warmed_up() {
            return Ok(vec![]);
        }

        let Some(diff) = self.current_diff() else {
            return Ok(vec![]);
        };

        let cmds = self.commands_for_diff(bar, ctx, diff);
        self.prev_diff = Some(diff);
        Ok(cmds)
    }
}

fn validate_config(fast: usize, slow: usize, qty: Decimal) -> Result<()> {
    anyhow::ensure!(fast > 0, "fast must be positive");
    anyhow::ensure!(slow > 0, "slow must be positive");
    anyhow::ensure!(fast < slow, "fast({}) must be < slow({})", fast, slow);
    anyhow::ensure!(qty > Decimal::ZERO, "qty must be positive");
    Ok(())
}

fn parse_positive_qty(raw: &str) -> Result<Decimal> {
    let qty = Decimal::from_str(raw).map_err(|e| anyhow!("qty 파싱 실패: {e}"))?;
    anyhow::ensure!(qty > Decimal::ZERO, "qty must be positive");
    Ok(qty)
}

fn next_ema(current: Option<Decimal>, k: Decimal, price: Decimal) -> Decimal {
    match current {
        Some(ema) => ema + k * (price - ema),
        None => price,
    }
}

fn cross_signal(prev_diff: Option<Decimal>, diff: Decimal) -> Option<OrderSide> {
    match prev_diff {
        Some(prev) if prev <= Decimal::ZERO && diff > Decimal::ZERO => Some(OrderSide::Buy),
        Some(prev) if prev >= Decimal::ZERO && diff < Decimal::ZERO => Some(OrderSide::Sell),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::position::Position;
    use anyhow::Result;
    use rust_decimal_macros::dec;

    fn make_bar(price: Decimal) -> Bar {
        Bar {
            symbol: "BTCUSDT".into(),
            ts_ns: 1,
            open: price,
            high: price,
            low: price,
            close: price,
            volume: dec!(1),
        }
    }

    fn make_ctx() -> Context {
        Context {
            now_ns: 1,
            equity: dec!(10000),
            position: Position::default(),
            fear_greed: None,
        }
    }

    #[test]
    fn cross_signal_detects_direction_changes() -> Result<()> {
        assert_eq!(cross_signal(Some(dec!(-1)), dec!(1)), Some(OrderSide::Buy));
        assert_eq!(cross_signal(Some(dec!(1)), dec!(-1)), Some(OrderSide::Sell));
        assert!(cross_signal(Some(dec!(1)), dec!(2)).is_none());
        Ok(())
    }

    #[test]
    fn new_rejects_invalid_config() -> Result<()> {
        assert!(EmaCross::new(21, 9, dec!(1)).is_err());
        assert!(EmaCross::new(9, 21, dec!(0)).is_err());
        Ok(())
    }

    #[test]
    fn on_bar_silent_during_warmup() -> Result<()> {
        // fast=2, slow=3: slow_n=3 봉이 쌓여야 워밍업 완료
        let mut strategy = EmaCross::new(2, 3, dec!(1))?;
        let ctx = make_ctx();
        for price in [dec!(100), dec!(90), dec!(80)] {
            let cmds = strategy.on_bar(&make_bar(price), &ctx)?;
            assert!(cmds.is_empty(), "워밍업 전 명령 없음");
        }
        Ok(())
    }

    #[test]
    fn on_bar_emits_buy_on_golden_cross() -> Result<()> {
        // fast=2(k=2/3), slow=3(k=1/2) 최소 주기로 빠르게 교차 유도
        // [100→90→80] 하락 후 [100] 급등 → fast EMA가 slow EMA를 상향 돌파
        let mut strategy = EmaCross::new(2, 3, dec!(1))?;
        let ctx = make_ctx();

        // 워밍업 + prev_diff 설정 (첫 warmed-up 봉에서 diff<0 기록)
        for price in [dec!(100), dec!(90), dec!(80), dec!(80)] {
            strategy.on_bar(&make_bar(price), &ctx)?;
        }

        // 급등으로 fast > slow 교차 → Buy 진입
        let cmds = strategy.on_bar(&make_bar(dec!(200)), &ctx)?;
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                StrategyCommand::SubmitOrder(r) if r.side == OrderSide::Buy && !r.reduce_only
            )),
            "골든 크로스 시 Buy 진입 주문 필요"
        );
        Ok(())
    }

    #[test]
    fn on_bar_emits_sell_on_death_cross() -> Result<()> {
        // [100→110→120] 상승 후 [50] 급락 → fast EMA가 slow EMA를 하향 돌파
        let mut strategy = EmaCross::new(2, 3, dec!(1))?;
        let ctx = make_ctx();

        for price in [dec!(100), dec!(110), dec!(120), dec!(120)] {
            strategy.on_bar(&make_bar(price), &ctx)?;
        }

        let cmds = strategy.on_bar(&make_bar(dec!(10)), &ctx)?;
        assert!(
            cmds.iter().any(|c| matches!(
                c,
                StrategyCommand::SubmitOrder(r) if r.side == OrderSide::Sell && !r.reduce_only
            )),
            "데스 크로스 시 Sell 진입 주문 필요"
        );
        Ok(())
    }
}
