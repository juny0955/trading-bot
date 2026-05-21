use crate::domain::backtest::Bar;
use crate::domain::order::OrderSide;
use crate::domain::strategies::{Context, Strategy, StrategyCommand};
use anyhow::Result;
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

pub struct MacdCross {
    macd: MovingAverageConvergenceDivergence,
    qty: Decimal,
    prev_histogram: Option<f64>,
}

impl MacdCross {
    pub fn from_params(v: &Value) -> Result<Self> {
        let p: Params = serde_json::from_value(v.clone())?;
        let qty = Decimal::from_str(&p.qty).map_err(|e| anyhow::anyhow!("qty 파싱 실패: {e}"))?;
        anyhow::ensure!(qty > Decimal::ZERO, "qty must be positive");
        let macd = MovingAverageConvergenceDivergence::new(p.fast, p.slow, p.signal)
            .map_err(|e| anyhow::anyhow!("MACD 초기화 실패: {e}"))?;
        Ok(Self {
            macd,
            qty,
            prev_histogram: None,
        })
    }
}

impl Strategy for MacdCross {
    fn on_bar(&mut self, bar: &Bar, ctx: &Context) -> Vec<StrategyCommand> {
        let out = self.macd.next(bar.close.to_f64().unwrap_or(0.0));
        let cmds = match self.prev_histogram {
            // Histogram 0 골든 크로스 -> 롱
            Some(prev) if prev <= 0.0 && out.histogram > 0.0 => {
                Self::close_and_open(&ctx.position, &bar.symbol, OrderSide::Buy, self.qty)
            }
            // Histogram 0 데스 크로스 -> 숏
            Some(prev) if prev >= 0.0 && out.histogram < 0.0 => {
                Self::close_and_open(&ctx.position, &bar.symbol, OrderSide::Sell, self.qty)
            }
            _ => vec![],
        };

        self.prev_histogram = Some(out.histogram);
        cmds
    }
}
