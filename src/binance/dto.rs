use crate::types::{BookTickerData, DepthData, KlineData, MarkPriceData, TradeData};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct StreamEnvelope {
    pub stream: String,
    pub data: StreamData,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "e")]
pub enum StreamData {
    #[serde(rename = "trade")]
    Trade(TradeData),
    #[serde(rename = "depthUpdate")]
    Depth(DepthData),
    #[serde(rename = "bookTicker")]
    BookTicker(BookTickerData),
    #[serde(rename = "kline")]
    Kline(KlineData),
    #[serde(rename = "markPriceUpdate")]
    MarkPrice(MarkPriceData),
}
