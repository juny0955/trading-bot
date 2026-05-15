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
    pub buyer_is_market_maker: bool,
}

#[derive(Debug, Deserialize)]
pub struct PriceLevel(pub Decimal, pub Decimal);

#[derive(Debug, Deserialize)]
pub struct DepthData {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "U")]
    pub first_update_id: u64,
    #[serde(rename = "u")]
    pub last_update_id: u64,
    #[serde(rename = "b")]
    pub bids: Vec<PriceLevel>,
    #[serde(rename = "a")]
    pub asks: Vec<PriceLevel>,
    #[serde(rename = "E")]
    pub event_time: u64,
}

#[derive(Debug, Deserialize)]
pub struct BookTickerData {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "b")]
    pub bid_price: Decimal,
    #[serde(rename = "B")]
    pub bid_quantity: String,
    #[serde(rename = "a")]
    pub ask_price: Decimal,
    #[serde(rename = "A")]
    pub ask_quantity: String,
    #[serde(rename = "E")]
    pub event_time: u64,
}
