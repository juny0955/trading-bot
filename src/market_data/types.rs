use rust_decimal::Decimal;

pub struct Trade {
    pub symbol: String,
    pub price: Decimal,
    pub quantity: Decimal,
    pub time_ns: i64,
    pub buyer_is_market_maker: bool,
}

pub struct PriceLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

pub struct Depth {
    pub symbol: String,
    pub first_update_id: i64,
    pub last_update_id: i64,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    pub event_time_ns: i64,
}

pub struct BookTicker {
    pub symbol: String,
    pub bid_price: Decimal,
    pub bid_quantity: Decimal,
    pub ask_price: Decimal,
    pub ask_quantity: Decimal,
    pub event_time_ns: i64,
}

pub struct Kline {
    pub symbol: String,
    pub event_time_ns: i64,
    pub open_time_ms: i64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub quote_volume: Decimal,
    pub num_trades: i64,
    pub is_closed: bool,
}

pub struct MarkPrice {
    pub symbol: String,
    pub event_time_ns: i64,
    pub mark_price: Decimal,
    pub index_price: Decimal,
    pub funding_rate: Decimal,
    pub next_funding_time_ms: i64,
}

pub struct FearGreed {
    pub value: String,
    pub status: String,
    pub timestamp_sec: i64,
}
