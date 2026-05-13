use rust_decimal::Decimal;
use serde::Deserialize;

// ================ 바이낸스 데이터 ======================
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
    #[serde(rename = "U")]
    pub first_update_id: u64,
    #[serde(rename = "u")]
    pub last_update_id: u64,
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
    pub bid_price: Decimal,
    #[serde(rename = "B")]
    pub bid_quantity: String,
    #[serde(rename = "a")]
    pub ask_price: Decimal,
    #[serde(rename = "A")]
    pub ask_quantity: String,
}
// ================ 바이낸스 데이터 ======================

// ================ Alternative 데이터 ======================
#[derive(Debug, Deserialize)]
pub struct FngResponse {
    pub name: String,
    pub data: Vec<FngData>,
}

#[derive(Debug, Deserialize)]
pub struct FngData {
    pub value: String,
    #[serde(rename = "value_classification")]
    pub status: FngStatus,
    pub timestamp: String,
    pub time_until_update: String,
}

#[derive(Debug, Deserialize)]
pub enum FngStatus {
    #[serde(rename = "Extreme Fear")]
    ExtremeFear,
    #[serde(rename = "Fear")]
    Fear,
    #[serde(rename = "Neutral")]
    Neutral,
    #[serde(rename = "Greed")]
    Greed,
    #[serde(rename = "Extreme Greed")]
    ExtremeGreed,
}
