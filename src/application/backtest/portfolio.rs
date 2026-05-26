use crate::domain::order::{Fill, Order, OrderSide};
use crate::domain::position::Position;
use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::Zero;

const BPS_DIVISOR: Decimal = Decimal::from_parts(10_000, 0, 0, false, 0);
const FEE_ASSET: &str = "USDT";

/// 백테스트 포트폴리오 처리 중 발생할 수 있는 오류입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::application::backtest::portfolio::PortfolioError;
///
/// let err = PortfolioError::InvalidPrice;
/// assert!(err.to_string().contains("price"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum PortfolioError {
    #[error("initial equity must be non-negative")]
    InvalidInitialEquity,
    #[error("fee bps must be non-negative")]
    InvalidFeeBps,
    #[error("price must be positive")]
    InvalidPrice,
    #[error("quantity must be positive")]
    InvalidQuantity,
}

/// 백테스트에서 완전히 닫힌 거래의 손익 기록입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::application::backtest::portfolio::ClosedTrade;
/// use trading_bot::domain::order::OrderSide;
///
/// let trade = ClosedTrade {
///     entry_ts_ns: 1,
///     exit_ts_ns: 2,
///     side: OrderSide::Buy,
///     qty: dec!(1),
///     entry_price: dec!(100),
///     exit_price: dec!(110),
///     fees: dec!(0.08),
///     gross_pnl: dec!(10),
///     net_pnl: dec!(9.92),
/// };
/// assert_eq!(trade.side, OrderSide::Buy);
/// ```
#[derive(Debug, Clone)]
pub struct ClosedTrade {
    pub entry_ts_ns: i64,
    pub exit_ts_ns: i64,
    pub side: OrderSide,
    pub qty: Decimal,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    pub fees: Decimal,
    pub gross_pnl: Decimal,
    pub net_pnl: Decimal,
}

/// 백테스트 전용 현금/포지션/거래 내역 상태입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::application::backtest::portfolio::Portfolio;
///
/// let portfolio = Portfolio::new(dec!(10_000), dec!(4)).unwrap();
/// assert_eq!(portfolio.initial_equity, dec!(10_000));
/// ```
pub struct Portfolio {
    pub initial_equity: Decimal,
    pub equity: Decimal,
    pub(crate) position: Position,
    pub(crate) unrealized_pnl: Decimal,
    pub(crate) fee_bps: Decimal,
    pub closed_trades: Vec<ClosedTrade>,
    pub(crate) open_entry_ts: Option<i64>,
    pub(crate) open_entry_fee: Option<Decimal>,
}

impl Portfolio {
    /// 새 백테스트 포트폴리오를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::application::backtest::portfolio::Portfolio;
    ///
    /// let portfolio = Portfolio::new(dec!(1000), dec!(2)).unwrap();
    /// assert_eq!(portfolio.equity, dec!(1000));
    /// ```
    pub fn new(initial_equity: Decimal, fee_bps: Decimal) -> Result<Self, PortfolioError> {
        validate_initial_equity(initial_equity)?;
        validate_fee_bps(fee_bps)?;

        Ok(Self {
            initial_equity,
            equity: initial_equity,
            position: Position::default(),
            unrealized_pnl: Decimal::zero(),
            fee_bps,
            closed_trades: Vec::new(),
            open_entry_ts: None,
            open_entry_fee: None,
        })
    }

    /// 백테스트 가격으로 주문을 즉시 체결하고 생성된 체결 목록을 반환합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::application::backtest::portfolio::Portfolio;
    /// use trading_bot::domain::order::{Order, OrderRequest, OrderSide, OrderType};
    ///
    /// let mut portfolio = Portfolio::new(dec!(1000), dec!(0)).unwrap();
    /// let order = Order::new_from_request(OrderRequest {
    ///     symbol: "BTCUSDT".into(),
    ///     order_type: OrderType::Market,
    ///     side: OrderSide::Buy,
    ///     qty: dec!(1),
    ///     ..Default::default()
    /// }).unwrap();
    ///
    /// let fills = portfolio.execute_simulated(&order, dec!(100), 1).unwrap();
    /// assert_eq!(fills.len(), 1);
    /// ```
    pub fn execute_simulated(
        &mut self,
        order: &Order,
        price: Decimal,
        ts_ns: i64,
    ) -> Result<Vec<Fill>, PortfolioError> {
        validate_execution_inputs(order.qty, price)?;

        let mut fills = Vec::new();
        match execution_action(self.position.side, order.side) {
            ExecutionAction::IgnoreSameSide => return Ok(fills),
            ExecutionAction::CloseAndOpen => {
                if let Some(fill) = self.close_at(order, price, ts_ns) {
                    fills.push(fill);
                }
            }
            ExecutionAction::Open => {}
        }

        fills.push(self.open_at(order, price, ts_ns));
        Ok(fills)
    }

    fn open_at(&mut self, order: &Order, price: Decimal, ts_ns: i64) -> Fill {
        let fee = fee_of(price, order.qty, self.fee_bps);
        self.position = Position {
            side: Some(order.side),
            qty: order.qty,
            avg_entry_price: price,
            realized_pnl: self.position.realized_pnl,
        };
        self.open_entry_ts = Some(ts_ns);
        self.open_entry_fee = Some(fee);
        self.make_fill(order, order.qty, price, fee, ts_ns)
    }

    fn close_at(&mut self, order: &Order, price: Decimal, ts_ns: i64) -> Option<Fill> {
        let pos_side = self.position.side?;
        let qty = self.position.qty;
        let entry_price = self.position.avg_entry_price;
        let entry_ts = self.open_entry_ts.unwrap_or(ts_ns);

        let entry_fee = self
            .open_entry_fee
            .unwrap_or_else(|| fee_of(entry_price, qty, self.fee_bps));
        let exit_fee = fee_of(price, qty, self.fee_bps);
        let closed = closed_trade(CloseTradeInput {
            side: pos_side,
            qty,
            entry_price,
            exit_price: price,
            entry_fee,
            exit_fee,
            entry_ts_ns: entry_ts,
            exit_ts_ns: ts_ns,
        });

        self.position.realized_pnl += closed.net_pnl;
        self.closed_trades.push(closed);

        self.position = Position {
            realized_pnl: self.position.realized_pnl,
            ..Position::default()
        };
        self.unrealized_pnl = Decimal::zero();
        self.open_entry_ts = None;
        self.open_entry_fee = None;
        self.equity = self.initial_equity + self.position.realized_pnl;

        Some(self.make_fill(order, qty, price, exit_fee, ts_ns))
    }

    /// 지정한 mark 가격으로 미실현 손익과 equity를 갱신합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::application::backtest::portfolio::Portfolio;
    ///
    /// let mut portfolio = Portfolio::new(dec!(1000), dec!(0)).unwrap();
    /// portfolio.mark(dec!(100)).unwrap();
    /// assert_eq!(portfolio.equity, dec!(1000));
    /// ```
    pub fn mark(&mut self, mark_price: Decimal) -> Result<(), PortfolioError> {
        validate_price(mark_price)?;
        self.unrealized_pnl = self.position.unrealized(mark_price);
        self.equity = self.initial_equity + self.position.realized_pnl + self.unrealized_pnl;
        Ok(())
    }

    fn make_fill(
        &self,
        order: &Order,
        qty: Decimal,
        price: Decimal,
        fee: Decimal,
        ts_ns: i64,
    ) -> Fill {
        Fill {
            order_id: order.id,
            strategy_instance_id: order.strategy_instance_id,
            symbol: order.symbol.clone(),
            side: order.side,
            qty,
            price,
            fee,
            fee_asset: FEE_ASSET.into(),
            filled_at: Utc.timestamp_nanos(ts_ns),
            exchange_trade_id: -1,
            position_lifecycle_id: None,
        }
    }
}

enum ExecutionAction {
    Open,
    CloseAndOpen,
    IgnoreSameSide,
}

fn execution_action(position_side: Option<OrderSide>, order_side: OrderSide) -> ExecutionAction {
    match position_side {
        Some(side) if side == order_side => ExecutionAction::IgnoreSameSide,
        Some(_) => ExecutionAction::CloseAndOpen,
        None => ExecutionAction::Open,
    }
}

fn validate_initial_equity(initial_equity: Decimal) -> Result<(), PortfolioError> {
    if initial_equity < Decimal::zero() {
        return Err(PortfolioError::InvalidInitialEquity);
    }
    Ok(())
}

fn validate_fee_bps(fee_bps: Decimal) -> Result<(), PortfolioError> {
    if fee_bps < Decimal::zero() {
        return Err(PortfolioError::InvalidFeeBps);
    }
    Ok(())
}

fn validate_price(price: Decimal) -> Result<(), PortfolioError> {
    if price <= Decimal::zero() {
        return Err(PortfolioError::InvalidPrice);
    }
    Ok(())
}

fn validate_execution_inputs(qty: Decimal, price: Decimal) -> Result<(), PortfolioError> {
    if qty <= Decimal::zero() {
        return Err(PortfolioError::InvalidQuantity);
    }
    validate_price(price)
}

fn fee_of(price: Decimal, qty: Decimal, fee_bps: Decimal) -> Decimal {
    price * qty * fee_bps / BPS_DIVISOR
}

struct CloseTradeInput {
    side: OrderSide,
    qty: Decimal,
    entry_price: Decimal,
    exit_price: Decimal,
    entry_fee: Decimal,
    exit_fee: Decimal,
    entry_ts_ns: i64,
    exit_ts_ns: i64,
}

fn closed_trade(input: CloseTradeInput) -> ClosedTrade {
    let gross_pnl = gross_pnl(input.side, input.qty, input.entry_price, input.exit_price);
    let fees = input.entry_fee + input.exit_fee;

    ClosedTrade {
        entry_ts_ns: input.entry_ts_ns,
        exit_ts_ns: input.exit_ts_ns,
        side: input.side,
        qty: input.qty,
        entry_price: input.entry_price,
        exit_price: input.exit_price,
        fees,
        gross_pnl,
        net_pnl: gross_pnl - fees,
    }
}

fn gross_pnl(side: OrderSide, qty: Decimal, entry_price: Decimal, exit_price: Decimal) -> Decimal {
    match side {
        OrderSide::Buy => (exit_price - entry_price) * qty,
        OrderSide::Sell => (entry_price - exit_price) * qty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::order::{Order, OrderRequest, OrderSide, OrderType};
    use anyhow::Context;
    use rust_decimal_macros::dec;

    fn pf() -> anyhow::Result<Portfolio> {
        Ok(Portfolio::new(dec!(10000), dec!(4))?)
    }

    fn mkt(side: OrderSide, qty: Decimal) -> anyhow::Result<Order> {
        Ok(Order::new_from_request(OrderRequest {
            symbol: "BTCUSDT".into(),
            order_type: OrderType::Market,
            side,
            qty,
            ..Default::default()
        })?)
    }

    #[test]
    fn buy_open_close_net_pnl() -> anyhow::Result<()> {
        let mut p = pf()?;
        let buy = mkt(OrderSide::Buy, dec!(1))?;
        p.execute_simulated(&buy, dec!(100), 1)?;

        let close = mkt(OrderSide::Sell, dec!(1))?;
        p.execute_simulated(&close, dec!(110), 2)?;

        let t = p.closed_trades.last().context("trade should close")?;
        assert_eq!(t.gross_pnl, dec!(10));
        assert_eq!(t.net_pnl, dec!(9.9160));
        assert_eq!(p.position.realized_pnl, dec!(9.9160));
        Ok(())
    }

    #[test]
    fn sell_open_close_net_pnl() -> anyhow::Result<()> {
        let mut p = pf()?;
        let sell = mkt(OrderSide::Sell, dec!(1))?;
        p.execute_simulated(&sell, dec!(100), 1)?;

        let close = mkt(OrderSide::Buy, dec!(1))?;
        p.execute_simulated(&close, dec!(90), 2)?;

        let t = p.closed_trades.last().context("trade should close")?;
        assert_eq!(t.gross_pnl, dec!(10));
        assert_eq!(t.net_pnl, dec!(9.9240));
        Ok(())
    }

    #[test]
    fn same_side_order_is_ignored() -> anyhow::Result<()> {
        let mut p = pf()?;
        p.execute_simulated(&mkt(OrderSide::Buy, dec!(1))?, dec!(100), 1)?;
        let fills = p.execute_simulated(&mkt(OrderSide::Buy, dec!(1))?, dec!(110), 2)?;
        assert!(fills.is_empty());
        assert_eq!(p.position.qty, dec!(1));
        Ok(())
    }

    #[test]
    fn opposite_side_closes_and_reopens() -> anyhow::Result<()> {
        let mut p = pf()?;
        p.execute_simulated(&mkt(OrderSide::Buy, dec!(1))?, dec!(100), 1)?;
        let fills = p.execute_simulated(&mkt(OrderSide::Sell, dec!(1))?, dec!(110), 2)?;
        assert_eq!(fills.len(), 2);
        assert_eq!(p.position.side, Some(OrderSide::Sell));
        assert_eq!(p.position.avg_entry_price, dec!(110));
        assert_eq!(p.closed_trades.len(), 1);
        Ok(())
    }

    #[test]
    fn mark_updates_unrealized_and_equity() -> anyhow::Result<()> {
        let mut p = pf()?;
        p.execute_simulated(&mkt(OrderSide::Buy, dec!(2))?, dec!(100), 1)?;
        p.mark(dec!(105))?;
        assert_eq!(p.unrealized_pnl, dec!(10));
        assert_eq!(p.equity, dec!(10010));
        Ok(())
    }

    #[test]
    fn invalid_price_returns_error() -> anyhow::Result<()> {
        let mut p = pf()?;
        let result = p.execute_simulated(&mkt(OrderSide::Buy, dec!(1))?, dec!(0), 1);
        assert!(matches!(result, Err(PortfolioError::InvalidPrice)));
        Ok(())
    }
}
