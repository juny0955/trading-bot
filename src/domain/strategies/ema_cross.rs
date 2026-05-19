use crate::domain::backtest::{Bar, Position};
use crate::domain::order::{OrderRequest, OrderSide, OrderType};
use crate::domain::strategies::{Context, Strategy, StrategyCommand};
use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::Deserialize;

#[derive(Deserialize)]
struct Params {
    fast: usize,
    slow: usize,
    qty: f64,
}

pub struct EmaCross {
    fast_n: usize,
    slow_n: usize,
    qty: Decimal,
    fast_ema: Option<f64>,
    slow_ema: Option<f64>,
    prev_diff: Option<f64>,
    bars_seen: usize,
}

impl EmaCross {
    pub fn new(fast: usize, slow: usize, qty: Decimal) -> Self {
        Self {
            fast_n: fast,
            slow_n: slow,
            qty,
            fast_ema: None,
            slow_ema: None,
            prev_diff: None,
            bars_seen: 0,
        }
    }

    pub fn from_params(v: &serde_json::Value) -> Result<Self> {
        let p: Params = serde_json::from_value(v.clone())?;
        anyhow::ensure!(
            p.fast < p.slow,
            "fast({}) must be < slow({})",
            p.fast,
            p.slow
        );
        anyhow::ensure!(p.qty > 0.0, "qty must be positive");
        Ok(Self {
            fast_n: p.fast,
            slow_n: p.slow,
            qty: Decimal::from_f64_retain(p.qty).unwrap_or_default(),
            fast_ema: None,
            slow_ema: None,
            prev_diff: None,
            bars_seen: 0,
        })
    }

    fn close_and_open(
        position: &Position,
        symbol: &str,
        new_side: OrderSide,
        qty: Decimal,
    ) -> Vec<StrategyCommand> {
        let mut cmds = Vec::new();
        if let Some(cur) = position.side {
            let close_side = if cur == OrderSide::Buy {
                OrderSide::Sell
            } else {
                OrderSide::Buy
            };
            cmds.push(StrategyCommand::SubmitOrder(OrderRequest {
                symbol: symbol.into(),
                order_type: OrderType::Market,
                side: close_side,
                qty: position.qty,
                reduce_only: true,
                ..Default::default()
            }));
        }

        cmds.push(StrategyCommand::SubmitOrder(OrderRequest {
            symbol: symbol.into(),
            order_type: OrderType::Market,
            side: new_side,
            qty,
            ..Default::default()
        }));

        cmds
    }
}

impl Strategy for EmaCross {
    fn on_bar(&mut self, bar: &Bar, ctx: &Context) -> Vec<StrategyCommand> {
        let price = bar.close.to_f64().unwrap_or(0.0);
        let kf = 2.0 / (self.fast_n as f64 + 1.0);
        let ks = 2.0 / (self.slow_n as f64 + 1.0);
        self.fast_ema = Some(self.fast_ema.map_or(price, |e| e + kf * (price - e)));
        self.slow_ema = Some(self.slow_ema.map_or(price, |e| e + ks * (price - e)));
        self.bars_seen += 1;
        if self.bars_seen < self.slow_n {
            return vec![];
        }

        let diff = self.fast_ema.unwrap() - self.slow_ema.unwrap();
        let cmds = if let Some(prev) = self.prev_diff {
            if prev <= 0.0 && diff > 0.0 {
                Self::close_and_open(&ctx.position, &bar.symbol, OrderSide::Buy, self.qty)
            } else if prev >= 0.0 && diff < 0.0 {
                Self::close_and_open(&ctx.position, &bar.symbol, OrderSide::Sell, self.qty)
            } else {
                vec![]
            }
        } else {
            vec![]
        };
        self.prev_diff = Some(diff);
        cmds
    }
}
