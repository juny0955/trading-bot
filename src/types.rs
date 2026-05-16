use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TradeData {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "p")]
    pub price: Decimal,
    #[serde(rename = "q")]
    pub quantity: Decimal,
    #[serde(rename = "T")]
    pub time: i64,
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
    pub first_update_id: i64,
    #[serde(rename = "u")]
    pub last_update_id: i64,
    #[serde(rename = "b")]
    pub bids: Vec<PriceLevel>,
    #[serde(rename = "a")]
    pub asks: Vec<PriceLevel>,
    #[serde(rename = "E")]
    pub event_time: i64,
}

#[derive(Debug, Deserialize)]
pub struct BookTickerData {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "b")]
    pub bid_price: Decimal,
    #[serde(rename = "B")]
    pub bid_quantity: Decimal,
    #[serde(rename = "a")]
    pub ask_price: Decimal,
    #[serde(rename = "A")]
    pub ask_quantity: Decimal,
    #[serde(rename = "E")]
    pub event_time: i64,
}

#[derive(Debug, Deserialize)]
pub struct KlineInner {
    #[serde(rename = "t")]
    pub open_time: i64,
    #[serde(rename = "o")]
    pub open: Decimal,
    #[serde(rename = "h")]
    pub high: Decimal,
    #[serde(rename = "l")]
    pub low: Decimal,
    #[serde(rename = "c")]
    pub close: Decimal,
    #[serde(rename = "v")]
    pub volume: Decimal,
    #[serde(rename = "n")]
    pub num_trades: i64,
    #[serde(rename = "x")]
    pub is_closed: bool,
    #[serde(rename = "q")]
    pub quote_volume: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct KlineData {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "k")]
    pub kline: KlineInner,
}

#[derive(Debug, Deserialize)]
pub struct MarkPriceData {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "p")]
    pub mark_price: Decimal,
    #[serde(rename = "i")]
    pub index_price: Decimal,
    #[serde(rename = "r")]
    pub funding_rate: Decimal,
    #[serde(rename = "T")]
    pub next_funding_time: i64,
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

impl FngStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            FngStatus::ExtremeFear => "Extreme Fear",
            FngStatus::Fear => "Fear",
            FngStatus::Neutral => "Neutral",
            FngStatus::Greed => "Greed",
            FngStatus::ExtremeGreed => "Extreme Greed",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct FngData {
    pub value: String,
    #[serde(rename = "value_classification")]
    pub status: FngStatus,
    pub timestamp: String,
    pub time_until_update: String,
}
