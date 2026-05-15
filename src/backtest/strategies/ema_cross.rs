use crate::backtest::strategy::{Context, Strategy};
use crate::backtest::types::{Bar, Side};
use anyhow::Result;
use rust_decimal::Decimal;
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
    pub fn from_params(v: &serde_json::Value) -> Result<Self> {
        let p: Params = serde_json::from_value(v.clone())?;
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
}

impl Strategy for EmaCross {
    fn on_bar(&mut self, bar: &Bar, ctx: &mut Context) {
        let price = bar.close.to_string().parse::<f64>().unwrap_or(0.0);
        let kf = 2.0 / (self.fast_n as f64 + 1.0);
        let ks = 2.0 / (self.slow_n as f64 + 1.0);
        self.fast_ema = Some(self.fast_ema.map_or(price, |e| e + kf * (price - e)));
        self.slow_ema = Some(self.slow_ema.map_or(price, |e| e + ks * (price - e)));
        self.bars_seen += 1;
        if self.bars_seen < self.slow_n {
            return;
        }

        let diff = self.fast_ema.unwrap() - self.slow_ema.unwrap();
        if let Some(prev) = self.prev_diff {
            // golden cross → long, dead cross → short
            if prev <= 0.0 && diff > 0.0 {
                ctx.close_position();
                ctx.submit_market(Side::Long, self.qty);
            } else if prev >= 0.0 && diff < 0.0 {
                ctx.close_position();
                ctx.submit_market(Side::Short, self.qty);
            }
        }
        self.prev_diff = Some(diff);
    }
}
