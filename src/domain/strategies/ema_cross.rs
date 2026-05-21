use crate::domain::backtest::Bar;
use crate::domain::order::OrderSide;
use crate::domain::strategies::{Context, Strategy, StrategyCommand};
use anyhow::{Result, anyhow};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;
use std::str::FromStr;

#[derive(Deserialize)]
struct Params {
    fast: usize,
    slow: usize,
    qty: String,
}

pub struct EmaCross {
    fast_n: usize,
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
    pub fn new(fast: usize, slow: usize, qty: Decimal) -> Self {
        Self {
            fast_n: fast,
            slow_n: slow,
            qty,
            fast_k: Self::smoothing(fast),
            slow_k: Self::smoothing(slow),
            fast_ema: None,
            slow_ema: None,
            prev_diff: None,
            bars_seen: 0,
        }
    }

    fn smoothing(n: usize) -> Decimal {
        // k = 2 / (n + 1)
        dec!(2) / Decimal::from(n + 1)
    }

    pub fn from_params(v: &serde_json::Value) -> Result<Self> {
        let p: Params = serde_json::from_value(v.clone())?;
        anyhow::ensure!(
            p.fast < p.slow,
            "fast({}) must be < slow({})",
            p.fast,
            p.slow
        );

        let qty = Decimal::from_str(&p.qty).map_err(|e| anyhow!("qty 파싱 실패: {e}"))?;

        anyhow::ensure!(qty > Decimal::ZERO, "qty must be positive");
        Ok(Self::new(p.fast, p.slow, qty))
    }
}

impl Strategy for EmaCross {
    fn on_bar(&mut self, bar: &Bar, ctx: &Context) -> Vec<StrategyCommand> {
        let price = bar.close;
        self.fast_ema = Some(match self.fast_ema {
            Some(e) => e + self.fast_k * (price - e),
            None => price,
        });
        self.slow_ema = Some(match self.slow_ema {
            Some(e) => e + self.slow_k * (price - e),
            None => price,
        });
        self.bars_seen += 1;
        if self.bars_seen < self.slow_n {
            return vec![];
        }

        let (Some(fast), Some(slow)) = (self.fast_ema, self.slow_ema) else {
            return vec![];
        };
        let diff = fast - slow;

        let cmds = match self.prev_diff {
            Some(prev) if prev <= Decimal::ZERO && diff > Decimal::ZERO => {
                Self::close_and_open(&ctx.position, &bar.symbol, OrderSide::Buy, self.qty)
            }
            Some(prev) if prev >= Decimal::ZERO && diff < Decimal::ZERO => {
                Self::close_and_open(&ctx.position, &bar.symbol, OrderSide::Sell, self.qty)
            }
            _ => vec![],
        };
        self.prev_diff = Some(diff);
        cmds
    }
}
