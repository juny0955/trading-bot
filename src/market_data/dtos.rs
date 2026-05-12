use rust_decimal::Decimal;
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
}

#[derive(Debug, Deserialize)]
pub struct TradeData {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "p")]
    pub price: Decimal,
    #[serde(rename = "q")]
    pub quantity: Decimal,
    #[serde(rename = "T")]
    pub time: u64,
    #[serde(rename = "m")]
    pub is_buyer: bool,
}

#[derive(Debug, Deserialize)]
pub struct PriceLevel(pub Decimal, pub Decimal);

#[derive(Debug, Deserialize)]
pub struct DepthData {
    #[serde(rename = "b")]
    pub bids: Vec<PriceLevel>,
    #[serde(rename = "a")]
    pub asks: Vec<PriceLevel>,
}

#[derive(Debug, Deserialize)]
pub struct BookTickerData {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "b")]
    pub bid_price: String,
    #[serde(rename = "B")]
    pub bid_quantity: String,
    #[serde(rename = "a")]
    pub ask_price: String,
    #[serde(rename = "A")]
    pub ask_quantity: String,
}
