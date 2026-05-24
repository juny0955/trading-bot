use rust_decimal::Decimal;

/// 백테스트에서 사용하는 OHLCV 캔들입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::backtest::Bar;
///
/// let bar = Bar {
///     symbol: "BTCUSDT".into(),
///     ts_ns: 1,
///     open: dec!(100),
///     high: dec!(110),
///     low: dec!(90),
///     close: dec!(105),
///     volume: dec!(10),
/// };
/// assert_eq!(bar.close, dec!(105));
/// ```
#[derive(Debug, Clone)]
pub struct Bar {
    pub symbol: String,
    pub ts_ns: i64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
}

/// 백테스트에서 사용하는 L1 호가 스냅샷입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::backtest::DepthSnapshot;
///
/// let snap = DepthSnapshot {
///     symbol: "BTCUSDT".into(),
///     ts_ns: 1,
///     bid1_price: dec!(99),
///     bid1_qty: dec!(1),
///     ask1_price: dec!(100),
///     ask1_qty: dec!(1),
/// };
/// assert_eq!(snap.ask1_price, dec!(100));
/// ```
#[derive(Debug, Clone)]
pub struct DepthSnapshot {
    pub symbol: String,
    pub ts_ns: i64,
    pub bid1_price: Decimal,
    pub bid1_qty: Decimal,
    pub ask1_price: Decimal,
    pub ask1_qty: Decimal,
    // L2~L10은 v2. v1은 L1만 사용.
}

/// 백테스트 엔진에 시간순으로 공급되는 이벤트입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::backtest::{Bar, Event};
///
/// let event = Event::Bar(Bar {
///     symbol: "BTCUSDT".into(),
///     ts_ns: 10,
///     open: dec!(1),
///     high: dec!(1),
///     low: dec!(1),
///     close: dec!(1),
///     volume: dec!(1),
/// });
/// assert_eq!(event.ts_ns(), 10);
/// ```
#[derive(Debug, Clone)]
pub enum Event {
    Bar(Bar),
    Depth(DepthSnapshot),
}

impl Event {
    /// 이벤트의 nanosecond timestamp를 반환합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::domain::backtest::{DepthSnapshot, Event};
    ///
    /// let event = Event::Depth(DepthSnapshot {
    ///     symbol: "BTCUSDT".into(),
    ///     ts_ns: 20,
    ///     bid1_price: dec!(99),
    ///     bid1_qty: dec!(1),
    ///     ask1_price: dec!(100),
    ///     ask1_qty: dec!(1),
    /// });
    /// assert_eq!(event.ts_ns(), 20);
    /// ```
    pub fn ts_ns(&self) -> i64 {
        match self {
            Event::Bar(b) => b.ts_ns,
            Event::Depth(d) => d.ts_ns,
        }
    }
}
