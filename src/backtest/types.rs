use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub struct Bar {
    pub symbol: String,
    pub ts_ns: i64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
}

#[derive(Debug, Clone)]
pub struct DepthSnapshot {
    pub symbol: String,
    pub ts_ns: i64,
    pub bid1_price: Decimal,
    pub bid1_qty: Decimal,
    pub ask1_price: Decimal,
    pub ask1_qty: Decimal,
    // L2~L10은 v2. v1은 L1만 사용.
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Long,
    Short,
}

#[derive(Debug, Clone)]
pub enum BacktestOrder {
    Market { side: Side, qty: Decimal },
    Close,
}

#[derive(Debug, Clone)]
pub struct Fill {
    pub ts_ns: i64,
    pub side: Side,
    pub qty: Decimal,
    pub price: Decimal,
    pub fee: Decimal,
}

#[derive(Debug, Clone, Default)]
pub struct Position {
    pub side: Option<Side>,
    pub qty: Decimal,
    pub entry_price: Decimal,
    pub entry_ts: i64,
}

#[derive(Debug, Clone)]
pub enum Event {
    Bar(Bar),
    Depth(DepthSnapshot),
}

impl Event {
    pub fn ts_ns(&self) -> i64 {
        match self {
            Event::Bar(b) => b.ts_ns,
            Event::Depth(d) => d.ts_ns,
        }
    }
}
