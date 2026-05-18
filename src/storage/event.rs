use crate::binance::dto::StreamData;
use crate::order::types::Fill;
use crate::types::{BookTickerData, DepthData, FngData, KlineData, MarkPriceData, TradeData};

pub enum StorageEvent {
    Trade(TradeData),
    Depth(DepthData),
    BookTicker(BookTickerData),
    Kline(KlineData),
    MarkPrice(MarkPriceData),
    Fng(FngData),
    Fill(Fill),
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
