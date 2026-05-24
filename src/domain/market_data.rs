use rust_decimal::Decimal;

/// 거래소 체결 이벤트입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::market_data::Trade;
///
/// let trade = Trade {
///     symbol: "BTCUSDT".into(),
///     price: dec!(100),
///     quantity: dec!(1),
///     time_ns: 1,
///     buyer_is_market_maker: false,
/// };
/// assert_eq!(trade.symbol, "BTCUSDT");
/// ```
pub struct Trade {
    pub symbol: String,
    pub price: Decimal,
    pub quantity: Decimal,
    pub time_ns: i64,
    pub buyer_is_market_maker: bool,
}

/// 호가 한 단계의 가격과 수량입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::market_data::PriceLevel;
///
/// let level = PriceLevel { price: dec!(100), quantity: dec!(2) };
/// assert_eq!(level.quantity, dec!(2));
/// ```
pub struct PriceLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

/// 호가창 변경 이벤트입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::domain::market_data::Depth;
///
/// let depth = Depth {
///     symbol: "BTCUSDT".into(),
///     first_update_id: 1,
///     last_update_id: 2,
///     bids: vec![],
///     asks: vec![],
///     event_time_ns: 100,
/// };
/// assert_eq!(depth.last_update_id, 2);
/// ```
pub struct Depth {
    pub symbol: String,
    pub first_update_id: i64,
    pub last_update_id: i64,
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    pub event_time_ns: i64,
}

/// 최우선 매수/매도 호가 이벤트입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::market_data::BookTicker;
///
/// let ticker = BookTicker {
///     symbol: "BTCUSDT".into(),
///     bid_price: dec!(99),
///     bid_quantity: dec!(1),
///     ask_price: dec!(100),
///     ask_quantity: dec!(1),
///     event_time_ns: 1,
/// };
/// assert_eq!(ticker.ask_price, dec!(100));
/// ```
#[derive(Debug, Clone)]
pub struct BookTicker {
    pub symbol: String,
    pub bid_price: Decimal,
    pub bid_quantity: Decimal,
    pub ask_price: Decimal,
    pub ask_quantity: Decimal,
    pub event_time_ns: i64,
}

/// 캔들 데이터 이벤트입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::market_data::Kline;
///
/// let kline = Kline {
///     symbol: "BTCUSDT".into(),
///     event_time_ns: 1,
///     open_time_ms: 1,
///     open: dec!(100),
///     high: dec!(110),
///     low: dec!(90),
///     close: dec!(105),
///     volume: dec!(10),
///     quote_volume: dec!(1000),
///     num_trades: 3,
///     is_closed: true,
/// };
/// assert!(kline.is_closed);
/// ```
pub struct Kline {
    pub symbol: String,
    pub event_time_ns: i64,
    pub open_time_ms: i64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub quote_volume: Decimal,
    pub num_trades: i64,
    pub is_closed: bool,
}

/// 선물 mark price와 펀딩 정보입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::market_data::MarkPrice;
///
/// let mark = MarkPrice {
///     symbol: "BTCUSDT".into(),
///     event_time_ns: 1,
///     mark_price: dec!(100),
///     index_price: dec!(100),
///     funding_rate: dec!(0.0001),
///     next_funding_time_ms: 2,
/// };
/// assert_eq!(mark.mark_price, dec!(100));
/// ```
pub struct MarkPrice {
    pub symbol: String,
    pub event_time_ns: i64,
    pub mark_price: Decimal,
    pub index_price: Decimal,
    pub funding_rate: Decimal,
    pub next_funding_time_ms: i64,
}

/// 공포탐욕 지수 데이터입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::domain::market_data::FearGreed;
///
/// let fng = FearGreed {
///     value: "50".into(),
///     status: "Neutral".into(),
///     timestamp_sec: 1,
/// };
/// assert_eq!(fng.status, "Neutral");
/// ```
pub struct FearGreed {
    pub value: String,
    pub status: String,
    pub timestamp_sec: i64,
}
