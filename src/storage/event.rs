use crate::binance::StreamData;
use crate::binance::dto::{BookTickerData, DepthData, KlineData, MarkPriceData, TradeData};
use crate::market_data::alternative::dto::FngData;

pub enum MarketDataEvent {
    Trade(TradeData),
    Depth(DepthData),
    BookTicker(BookTickerData),
    Kline(KlineData),
    MarkPrice(MarkPriceData),
    Fng(FngData),
}

impl From<StreamData> for MarketDataEvent {
    fn from(value: StreamData) -> Self {
        match value {
            StreamData::Trade(d) => MarketDataEvent::Trade(d),
            StreamData::Depth(d) => MarketDataEvent::Depth(d),
            StreamData::BookTicker(d) => MarketDataEvent::BookTicker(d),
            StreamData::Kline(d) => MarketDataEvent::Kline(d),
            StreamData::MarkPrice(d) => MarketDataEvent::MarkPrice(d),
        }
    }
}

impl From<FngData> for MarketDataEvent {
    fn from(value: FngData) -> Self {
        MarketDataEvent::Fng(value)
    }
}
