use crate::domain::backtest::{Bar, DepthSnapshot};
use crate::domain::order::{OrderRequest, OrderSide, OrderType};
use crate::domain::position::Position;
use crate::domain::strategies::ema_cross::EmaCross;
use crate::domain::strategies::macd_cross::MacdCross;
use crate::domain::strategies::rsi_bounce::RsiBounce;
use rust_decimal::Decimal;
use serde_json::Value;

pub mod ema_cross;
pub mod macd_cross;
pub mod rsi_bounce;

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
    fn on_bar(&mut self, _bar: &Bar, _ctx: &Context) -> Vec<StrategyCommand> {
        vec![]
    }
    fn on_depth(&mut self, _snap: &DepthSnapshot, _ctx: &Context) -> Vec<StrategyCommand> {
        vec![]
    }
    fn on_finish(&mut self, _ctx: &Context) -> Vec<StrategyCommand> {
        vec![]
    }
    fn close_and_open(
        position: &Position,
        symbol: &str,
        new_side: OrderSide,
        qty: Decimal,
    ) -> Vec<StrategyCommand>
    where
        Self: Sized,
    {
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

#[derive(Debug, Clone)]
pub struct StrategyInstance {
    pub id: i64,
    pub strategy_type: String,
    pub name: String,
    pub params: Value,
    pub symbols: Vec<String>,
}

pub fn create_strategy(name: &str, params: &Value) -> anyhow::Result<Box<dyn Strategy>> {
    match name {
        "ema_cross" => Ok(Box::new(EmaCross::from_params(params)?)),
        "rsi_bounce" => Ok(Box::new(RsiBounce::from_params(params)?)),
        "macd_cross" => Ok(Box::new(MacdCross::from_params(params)?)),
        other => anyhow::bail!("알 수 없는 전략: {other}"),
    }
}
