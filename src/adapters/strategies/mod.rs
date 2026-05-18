use crate::application::backtest::strategy::Strategy;
use anyhow::{Result, bail};

pub mod ema_cross;

pub fn create_strategy(name: &str, params: &serde_json::Value) -> Result<Box<dyn Strategy>> {
    match name {
        "ema_cross" => Ok(Box::new(ema_cross::EmaCross::from_params(params)?)),
        other => bail!("알 수 없는 전략: {other}"),
    }
}
