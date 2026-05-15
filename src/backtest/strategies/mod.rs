mod ema_cross;

use crate::backtest::strategies::ema_cross::EmaCross;
use crate::backtest::strategy::Strategy;
use anyhow::bail;

pub fn create_strategy(
    name: &str,
    params: &serde_json::Value,
) -> anyhow::Result<Box<dyn Strategy + Send>> {
    match name {
        "ema_cross" => Ok(Box::new(EmaCross::from_params(params)?)),
        _ => bail!("알수없는 전략: {name}"),
    }
}
