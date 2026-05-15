use crate::backtest::types::{BacktestOrder, Bar, DepthSnapshot, Position, Side};
use rust_decimal::Decimal;

pub struct Context<'a> {
    pub now_ns: i64,
    pub position: &'a Position,
    pub equity: Decimal,
    pub(crate) pending: &'a mut Vec<BacktestOrder>,
}

impl Context<'_> {
    pub fn submit_market(&mut self, side: Side, qty: Decimal) {
        self.pending.push(BacktestOrder::Market { side, qty });
    }
    pub fn close_position(&mut self) {
        self.pending.push(BacktestOrder::Close);
    }
}

pub trait Strategy {
    fn on_start(&mut self, _ctx: &mut Context) {}
    fn on_bar(&mut self, bar: &Bar, ctx: &mut Context);
    fn on_depth(&mut self, _snap: &DepthSnapshot, _ctx: &mut Context) {}
    fn on_finish(&mut self, _ctx: &mut Context) {}
}
