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
use ta::indicators::RelativeStrengthIndex;

#[derive(Deserialize)]
struct Params {
    rsi_period: usize,
    oversold: u32,
    overbought: u32,
    qty: String,
}

pub struct RsiBounce {
    rsi: RelativeStrengthIndex,
    oversold: f64,
    overbought: f64,
    qty: Decimal,
    prev_rsi: Option<f64>,
}

impl RsiBounce {
    pub fn from_params(v: &Value) -> Result<Self> {
        let p: Params = serde_json::from_value(v.clone())?;
        let qty = Decimal::from_str(&p.qty).map_err(|e| anyhow::anyhow!("qty 파싱 실패: {e}"))?;
        anyhow::ensure!(qty > Decimal::ZERO, "qty must be positive");
        let rsi = RelativeStrengthIndex::new(p.rsi_period)
            .map_err(|e| anyhow::anyhow!("RSI 초기화 실패: {e}"))?;
        Ok(Self {
            rsi,
            oversold: p.oversold as f64,
            overbought: p.overbought as f64,
            qty,
            prev_rsi: None,
        })
    }
}

impl Strategy for RsiBounce {
    fn on_bar(&mut self, bar: &Bar, ctx: &Context) -> Vec<StrategyCommand> {
        let rsi_val = self.rsi.next(bar.close.to_f64().unwrap_or(0.0));
        let cmds = match self.prev_rsi {
            // RSI 상향 돌파 -> 롱
            Some(prev) if prev <= self.oversold && rsi_val > self.oversold => {
                Self::close_and_open(&ctx.position, &bar.symbol, OrderSide::Buy, self.qty)
            }
            // RSI 하향 돌파 ->  숏
            Some(prev) if prev >= self.overbought && rsi_val < self.overbought => {
                Self::close_and_open(&ctx.position, &bar.symbol, OrderSide::Sell, self.qty)
            }
            _ => vec![],
        };

        self.prev_rsi = Some(rsi_val);
        cmds
    }
}
