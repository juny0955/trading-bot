use crate::backtest::types::{Bar, DepthSnapshot, Position};
use crate::order::backtest::BacktestOrderExecutor;
use async_trait::async_trait;
use rust_decimal::Decimal;
use std::sync::Arc;

pub struct Context {
    pub now_ns: i64,
    pub equity: Decimal,
    pub position: Position,
    pub executor: Arc<BacktestOrderExecutor>,
}

#[async_trait]
pub trait Strategy: Send {
    async fn on_start(&mut self, _ctx: &Context) {}
    async fn on_bar(&mut self, bar: &Bar, ctx: &Context);
    async fn on_depth(&mut self, _snap: &DepthSnapshot, _ctx: &Context) {}
    async fn on_finish(&mut self, _ctx: &Context) {}
}
