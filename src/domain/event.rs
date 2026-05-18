use crate::domain::market_data::{BookTicker, Depth, FearGreed, Kline, MarkPrice, Trade};

pub enum MarketDataEvent {
    Trade(Trade),
    Depth(Depth),
    BookTicker(BookTicker),
    Kline(Kline),
    MarkPrice(MarkPrice),
    FearGreed(FearGreed),
}
