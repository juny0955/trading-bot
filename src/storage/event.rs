use crate::market_data::alternative::dto::FngData;
use crate::market_data::binance::dto::{
    BookTickerData, DepthData, KlineData, MarkPriceData, StreamData, TradeData,
};

pub enum StorageEvent {
    Trade(TradeData),
    Depth(DepthData),
    BookTicker(BookTickerData),
    Kline(KlineData),
    MarkPrice(MarkPriceData),
    Fng(FngData),
}

impl From<StreamData> for StorageEvent {
    fn from(value: StreamData) -> Self {
        match value {
            StreamData::Trade(d) => StorageEvent::Trade(d),
            StreamData::Depth(d) => StorageEvent::Depth(d),
            StreamData::BookTicker(d) => StorageEvent::BookTicker(d),
            StreamData::Kline(d) => StorageEvent::Kline(d),
            StreamData::MarkPrice(d) => StorageEvent::MarkPrice(d),
        }
    }
}

impl From<FngData> for StorageEvent {
    fn from(value: FngData) -> Self {
        StorageEvent::Fng(value)
    }
}
