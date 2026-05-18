use crate::domain;
use crate::domain::event::MarketDataEvent;
use crate::domain::market_data::{BookTicker, Depth, Kline, MarkPrice, Trade};
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
    #[serde(rename = "kline")]
    Kline(KlineData),
    #[serde(rename = "markPriceUpdate")]
    MarkPrice(MarkPriceData),
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

impl From<TradeData> for Trade {
    fn from(value: TradeData) -> Self {
        Trade {
            symbol: value.symbol,
            price: value.price,
            quantity: value.quantity,
            time_ns: value.time * 1_000_000,
            buyer_is_market_maker: value.buyer_is_market_maker,
        }
    }
}
impl From<DepthData> for Depth {
    fn from(value: DepthData) -> Self {
        Depth {
            symbol: value.symbol,
            first_update_id: value.first_update_id,
            last_update_id: value.last_update_id,
            bids: value
                .bids
                .iter()
                .map(|p| domain::market_data::PriceLevel {
                    price: p.0,
                    quantity: p.1,
                })
                .collect(),
            asks: value
                .asks
                .iter()
                .map(|p| domain::market_data::PriceLevel {
                    price: p.0,
                    quantity: p.1,
                })
                .collect(),
            event_time_ns: value.event_time * 1_000_000,
        }
    }
}
impl From<BookTickerData> for BookTicker {
    fn from(value: BookTickerData) -> Self {
        BookTicker {
            symbol: value.symbol,
            bid_price: value.bid_price,
            bid_quantity: value.bid_quantity,
            ask_price: value.ask_price,
            ask_quantity: value.ask_quantity,
            event_time_ns: value.event_time * 1_000_000,
        }
    }
}
impl From<KlineData> for Kline {
    fn from(value: KlineData) -> Self {
        Kline {
            symbol: value.symbol,
            event_time_ns: value.event_time * 1_000_000,
            open_time_ms: value.kline.open_time,
            open: value.kline.open,
            high: value.kline.high,
            low: value.kline.low,
            close: value.kline.close,
            volume: value.kline.volume,
            quote_volume: value.kline.quote_volume,
            num_trades: value.kline.num_trades,
            is_closed: value.kline.is_closed,
        }
    }
}
impl From<MarkPriceData> for MarkPrice {
    fn from(value: MarkPriceData) -> Self {
        MarkPrice {
            symbol: value.symbol,
            event_time_ns: value.event_time * 1_000_000,
            mark_price: value.mark_price,
            index_price: value.index_price,
            funding_rate: value.funding_rate,
            next_funding_time_ms: value.next_funding_time,
        }
    }
}

impl From<StreamData> for MarketDataEvent {
    fn from(value: StreamData) -> Self {
        match value {
            StreamData::Trade(d) => MarketDataEvent::Trade(d.into()),
            StreamData::Depth(d) => MarketDataEvent::Depth(d.into()),
            StreamData::BookTicker(d) => MarketDataEvent::BookTicker(d.into()),
            StreamData::Kline(d) => MarketDataEvent::Kline(d.into()),
            StreamData::MarkPrice(d) => MarketDataEvent::MarkPrice(d.into()),
        }
    }
}
