use crate::domain::backtest::Bar;
use crate::domain::order::OrderSide;
use crate::domain::strategies::{Context, Strategy, StrategyCommand};
use anyhow::{Result, anyhow};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::Deserialize;
use serde_json::Value;
use std::str::FromStr;
use ta::Next;
use ta::indicators::MovingAverageConvergenceDivergence;

#[derive(Deserialize)]
struct Params {
    fast: usize,
    slow: usize,
    signal: usize,
    qty: String,
}

/// MACD histogram의 0선 교차를 매매 신호로 사용하는 전략입니다.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use trading_bot::domain::strategies::macd_cross::MacdCross;
///
/// let params = json!({"fast": 12, "slow": 26, "signal": 9, "qty": "0.001"});
/// assert!(MacdCross::from_params(&params).is_ok());
/// ```
pub struct MacdCross {
    macd: MovingAverageConvergenceDivergence,
    qty: Decimal,
    prev_histogram: Option<f64>,
}

impl MacdCross {
    /// JSON 파라미터에서 전략을 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use serde_json::json;
    /// use trading_bot::domain::strategies::macd_cross::MacdCross;
    ///
    /// let params = json!({"fast": 12, "slow": 26, "signal": 9, "qty": "1"});
    /// assert!(MacdCross::from_params(&params).is_ok());
    /// ```
    pub fn from_params(v: &Value) -> Result<Self> {
        let p: Params = serde_json::from_value(v.clone())?;
        validate_params(&p)?;

        let qty = parse_positive_qty(&p.qty)?;
        let macd = MovingAverageConvergenceDivergence::new(p.fast, p.slow, p.signal)
            .map_err(|e| anyhow!("MACD 초기화 실패: {e}"))?;

        Ok(Self {
            macd,
            qty,
            prev_histogram: None,
        })
    }

    fn next_histogram(&mut self, bar: &Bar) -> Result<f64> {
        let close = decimal_to_f64(bar.close)?;
        Ok(self.macd.next(close).histogram)
    }

    fn commands_for_histogram(
        &self,
        bar: &Bar,
        ctx: &Context,
        histogram: f64,
    ) -> Vec<StrategyCommand> {
        match histogram_signal(self.prev_histogram, histogram) {
            Some(side) => Self::close_and_open(&ctx.position, &bar.symbol, side, self.qty),
            None => vec![],
        }
    }
}

impl Strategy for MacdCross {
    fn on_bar(&mut self, bar: &Bar, ctx: &Context) -> Result<Vec<StrategyCommand>> {
        let histogram = self.next_histogram(bar)?;
        let cmds = self.commands_for_histogram(bar, ctx, histogram);

        self.prev_histogram = Some(histogram);
        Ok(cmds)
    }
}

fn validate_params(params: &Params) -> Result<()> {
    anyhow::ensure!(params.fast > 0, "fast must be positive");
    anyhow::ensure!(params.slow > 0, "slow must be positive");
    anyhow::ensure!(params.signal > 0, "signal must be positive");
    anyhow::ensure!(
        params.fast < params.slow,
        "fast({}) must be < slow({})",
        params.fast,
        params.slow
    );
    Ok(())
}

fn parse_positive_qty(raw: &str) -> Result<Decimal> {
    let qty = Decimal::from_str(raw).map_err(|e| anyhow!("qty 파싱 실패: {e}"))?;
    anyhow::ensure!(qty > Decimal::ZERO, "qty must be positive");
    Ok(qty)
}

fn decimal_to_f64(value: Decimal) -> Result<f64> {
    value
        .to_f64()
        .ok_or_else(|| anyhow!("Decimal 값을 f64로 변환할 수 없습니다: {value}"))
}

fn histogram_signal(prev: Option<f64>, current: f64) -> Option<OrderSide> {
    match prev {
        Some(prev) if prev <= 0.0 && current > 0.0 => Some(OrderSide::Buy),
        Some(prev) if prev >= 0.0 && current < 0.0 => Some(OrderSide::Sell),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use serde_json::json;

    #[test]
    fn histogram_signal_detects_zero_cross() -> Result<()> {
        assert_eq!(histogram_signal(Some(-0.1), 0.1), Some(OrderSide::Buy));
        assert_eq!(histogram_signal(Some(0.1), -0.1), Some(OrderSide::Sell));
        assert!(histogram_signal(Some(0.1), 0.2).is_none());
        Ok(())
    }

    #[test]
    fn from_params_rejects_invalid_periods() -> Result<()> {
        let params = json!({"fast": 26, "slow": 12, "signal": 9, "qty": "1"});

        assert!(MacdCross::from_params(&params).is_err());
        Ok(())
    }
}
