use crate::domain::market_data::{BookTicker, Depth, FearGreed, Kline, MarkPrice, Trade};

/// 실시간 시장 데이터 공급자가 내보내는 도메인 이벤트입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::event::MarketDataEvent;
/// use trading_bot::domain::market_data::Trade;
///
/// let event = MarketDataEvent::Trade(Trade {
///     symbol: "BTCUSDT".into(),
///     price: dec!(100),
///     quantity: dec!(1),
///     time_ns: 1,
///     buyer_is_market_maker: false,
/// });
/// assert!(matches!(event, MarketDataEvent::Trade(_)));
/// ```
pub enum MarketDataEvent {
    Trade(Trade),
    Depth(Depth),
    BookTicker(BookTicker),
    Kline(Kline),
    MarkPrice(MarkPrice),
    FearGreed(FearGreed),
}
