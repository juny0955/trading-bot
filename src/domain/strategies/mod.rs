use crate::domain::backtest::{Bar, DepthSnapshot, Position};
use crate::domain::order::OrderRequest;
use crate::domain::strategies::ema_cross::EmaCross;
use rust_decimal::Decimal;
use serde_json::Value;

pub mod ema_cross;

pub struct Context {
    pub now_ns: i64,
    pub equity: Decimal,
    pub position: Position,
}

pub enum StrategyCommand {
    SubmitOrder(OrderRequest),
}

pub trait Strategy: Send {
    fn on_start(&mut self, _ctx: &Context) -> Vec<StrategyCommand> {
        vec![]
    }
    fn on_bar(&mut self, bar: &Bar, ctx: &Context) -> Vec<StrategyCommand> {
        vec![]
    }
    fn on_depth(&mut self, _snap: &DepthSnapshot, _ctx: &Context) -> Vec<StrategyCommand> {
        vec![]
    }
    fn on_finish(&mut self, _ctx: &Context) -> Vec<StrategyCommand> {
        vec![]
    }
}

pub fn create_strategy(name: &str, params: &Value) -> anyhow::Result<Box<dyn Strategy>> {
    match name {
        "ema_cross" => Ok(Box::new(EmaCross::from_params(params)?)),
        other => anyhow::bail!("알 수 없는 전략: {other}"),
    }
}
