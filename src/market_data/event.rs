use crate::market_data::types::{BookTicker, Depth, FearGreed, Kline, MarkPrice, Trade};

pub enum MarketDataEvent {
    Trade(Trade),
    Depth(Depth),
    BookTicker(BookTicker),
    Kline(Kline),
    MarkPrice(MarkPrice),
    FearGreed(FearGreed),
}
