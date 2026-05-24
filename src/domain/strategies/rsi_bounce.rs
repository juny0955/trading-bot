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
use ta::indicators::RelativeStrengthIndex;

#[derive(Deserialize)]
struct Params {
    rsi_period: usize,
    oversold: u32,
    overbought: u32,
    qty: String,
}

/// RSI 임계값 재돌파를 매매 신호로 사용하는 전략입니다.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use trading_bot::domain::strategies::rsi_bounce::RsiBounce;
///
/// let params = json!({
///     "rsi_period": 14,
///     "oversold": 30,
///     "overbought": 70,
///     "qty": "0.001"
/// });
/// assert!(RsiBounce::from_params(&params).is_ok());
/// ```
pub struct RsiBounce {
    rsi: RelativeStrengthIndex,
    oversold: f64,
    overbought: f64,
    qty: Decimal,
    prev_rsi: Option<f64>,
}

impl RsiBounce {
    /// JSON 파라미터에서 전략을 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use serde_json::json;
    /// use trading_bot::domain::strategies::rsi_bounce::RsiBounce;
    ///
    /// let params = json!({
    ///     "rsi_period": 14,
    ///     "oversold": 30,
    ///     "overbought": 70,
    ///     "qty": "1"
    /// });
    /// assert!(RsiBounce::from_params(&params).is_ok());
    /// ```
    pub fn from_params(v: &Value) -> Result<Self> {
        let p: Params = serde_json::from_value(v.clone())?;
        validate_params(&p)?;

        let qty = parse_positive_qty(&p.qty)?;
        let rsi = RelativeStrengthIndex::new(p.rsi_period)
            .map_err(|e| anyhow!("RSI 초기화 실패: {e}"))?;

        Ok(Self {
            rsi,
            oversold: p.oversold as f64,
            overbought: p.overbought as f64,
            qty,
            prev_rsi: None,
        })
    }

    fn next_rsi(&mut self, bar: &Bar) -> Result<f64> {
        let close = decimal_to_f64(bar.close)?;
        Ok(self.rsi.next(close))
    }

    fn commands_for_rsi(&self, bar: &Bar, ctx: &Context, rsi: f64) -> Vec<StrategyCommand> {
        match rsi_signal(self.prev_rsi, rsi, self.oversold, self.overbought) {
            Some(side) => Self::close_and_open(&ctx.position, &bar.symbol, side, self.qty),
            None => vec![],
        }
    }
}

impl Strategy for RsiBounce {
    fn on_bar(&mut self, bar: &Bar, ctx: &Context) -> Result<Vec<StrategyCommand>> {
        let rsi = self.next_rsi(bar)?;
        let cmds = self.commands_for_rsi(bar, ctx, rsi);

        self.prev_rsi = Some(rsi);
        Ok(cmds)
    }
}

fn validate_params(params: &Params) -> Result<()> {
    anyhow::ensure!(params.rsi_period > 0, "rsi_period must be positive");
    anyhow::ensure!(
        params.oversold < params.overbought,
        "oversold must be below overbought"
    );
    anyhow::ensure!(params.overbought <= 100, "overbought must be <= 100");
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

fn rsi_signal(
    prev: Option<f64>,
    current: f64,
    oversold: f64,
    overbought: f64,
) -> Option<OrderSide> {
    match prev {
        Some(prev) if prev <= oversold && current > oversold => Some(OrderSide::Buy),
        Some(prev) if prev >= overbought && current < overbought => Some(OrderSide::Sell),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use serde_json::json;

    #[test]
    fn rsi_signal_detects_threshold_reclaim() -> Result<()> {
        assert_eq!(
            rsi_signal(Some(25.0), 31.0, 30.0, 70.0),
            Some(OrderSide::Buy)
        );
        assert_eq!(
            rsi_signal(Some(75.0), 69.0, 30.0, 70.0),
            Some(OrderSide::Sell)
        );
        assert!(rsi_signal(Some(40.0), 45.0, 30.0, 70.0).is_none());
        Ok(())
    }

    #[test]
    fn from_params_rejects_inverted_thresholds() -> Result<()> {
        let params = json!({
            "rsi_period": 14,
            "oversold": 80,
            "overbought": 70,
            "qty": "1"
        });

        assert!(RsiBounce::from_params(&params).is_err());
        Ok(())
    }
}
