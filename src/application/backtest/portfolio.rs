use crate::domain::backtest::Position;
use crate::domain::order::{Fill, Order, OrderSide};
use chrono::{TimeZone, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::Zero;

const BPS_DIVISOR: Decimal = Decimal::from_parts(10_000, 0, 0, false, 0);
const FEE_ASSET: &str = "USDT";

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

pub struct Portfolio {
    pub initial_equity: Decimal,
    pub equity: Decimal,
    pub(crate) position: Position,
    pub(crate) realized_pnl: Decimal,
    pub(crate) unrealized_pnl: Decimal,
    pub(crate) fee_bps: Decimal,
    pub closed_trades: Vec<ClosedTrade>,
}

impl Portfolio {
    pub fn new(initial_equity: Decimal, fee_bps: Decimal) -> Self {
        Self {
            initial_equity,
            equity: initial_equity,
            position: Position::default(),
            realized_pnl: Decimal::zero(),
            unrealized_pnl: Decimal::zero(),
            fee_bps,
            closed_trades: Vec::new(),
        }
    }

    pub fn execute_simulated(&mut self, order: &Order, price: Decimal, ts_ns: i64) -> Vec<Fill> {
        let mut fills = Vec::new();
        if let Some(cur) = self.position.side {
            if cur != order.side {
                if let Some(f) = self.close_at(order, price, ts_ns) {
                    fills.push(f);
                }
            } else {
                return fills;
            }
        }

        fills.push(self.open_at(order, price, ts_ns));
        fills
    }

    fn open_at(&mut self, order: &Order, price: Decimal, ts_ns: i64) -> Fill {
        let fee = self.fee_of(price, order.qty);
        self.position = Position {
            side: Some(order.side),
            qty: order.qty,
            entry_price: price,
            entry_ts: ts_ns,
        };
        self.make_fill(order, order.qty, price, fee, ts_ns)
    }

    fn close_at(&mut self, order: &Order, price: Decimal, ts_ns: i64) -> Option<Fill> {
        let pos_side = self.position.side?;
        let qty = self.position.qty;
        let entry_price = self.position.entry_price;
        let entry_ts = self.position.entry_ts;

        let entry_fee = self.fee_of(entry_price, qty);
        let exit_fee = self.fee_of(price, qty);
        let fees = entry_fee + exit_fee;
        let gross = match pos_side {
            OrderSide::Buy => (price - entry_price) * qty,
            OrderSide::Sell => (entry_price - price) * qty,
        };
        let net = gross - fees;

        self.realized_pnl += net;
        self.closed_trades.push(ClosedTrade {
            entry_ts_ns: entry_ts,
            exit_ts_ns: ts_ns,
            side: pos_side,
            qty,
            entry_price,
            exit_price: price,
            fees,
            gross_pnl: gross,
            net_pnl: net,
        });

        self.position = Position::default();
        self.unrealized_pnl = Decimal::zero();
        self.equity = self.initial_equity + self.realized_pnl;

        Some(self.make_fill(order, qty, price, exit_fee, ts_ns))
    }

    pub fn mark(&mut self, mark_price: Decimal) {
        self.unrealized_pnl = match self.position.side {
            Some(OrderSide::Buy) => (mark_price - self.position.entry_price) * self.position.qty,
            Some(OrderSide::Sell) => (self.position.entry_price - mark_price) * self.position.qty,
            None => Decimal::zero(),
        };
        self.equity = self.initial_equity + self.realized_pnl + self.unrealized_pnl;
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
            symbol: order.symbol.clone(),
            side: order.side,
            qty,
            price,
            fee,
            fee_asset: FEE_ASSET.into(),
            filled_at: Utc.timestamp_nanos(ts_ns),
        }
    }

    fn fee_of(&self, price: Decimal, qty: Decimal) -> Decimal {
        price * qty * self.fee_bps / BPS_DIVISOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::order::{Order, OrderRequest, OrderSide, OrderType};
    use rust_decimal_macros::dec;

    fn pf() -> Portfolio {
        Portfolio::new(dec!(10000), dec!(4))
    }

    fn mkt(side: OrderSide, qty: Decimal) -> Order {
        Order::new_from_request(OrderRequest {
            symbol: "BTCUSDT".into(),
            order_type: OrderType::Market,
            side,
            qty,
            ..Default::default()
        })
    }

    #[test]
    fn buy_open_close_net_pnl() {
        let mut p = pf();
        let buy = mkt(OrderSide::Buy, dec!(1));
        p.execute_simulated(&buy, dec!(100), 1);

        let close = mkt(OrderSide::Sell, dec!(1));
        p.execute_simulated(&close, dec!(110), 2);

        let t = p.closed_trades.last().unwrap();
        assert_eq!(t.gross_pnl, dec!(10));
        assert_eq!(t.fees, dec!(0.0840)); // (100+110)*1*4/10000
        assert_eq!(t.net_pnl, dec!(9.9160));
        assert_eq!(p.realized_pnl, dec!(9.9160));
    }

    #[test]
    fn sell_open_close_net_pnl() {
        let mut p = pf();
        let sell = mkt(OrderSide::Sell, dec!(1));
        p.execute_simulated(&sell, dec!(100), 1);

        let close = mkt(OrderSide::Buy, dec!(1));
        p.execute_simulated(&close, dec!(90), 2);

        let t = p.closed_trades.last().unwrap();
        assert_eq!(t.gross_pnl, dec!(10));
        assert_eq!(t.fees, dec!(0.0760)); // (100+90)*1*4/10000
        assert_eq!(t.net_pnl, dec!(9.9240));
    }

    #[test]
    fn close_when_flat_is_noop() {
        let mut p = pf();
        p.execute_simulated(&mkt(OrderSide::Buy, dec!(1)), dec!(100), 1);
        let fills = p.execute_simulated(&mkt(OrderSide::Buy, dec!(1)), dec!(110), 2);
        assert!(fills.is_empty()); // 같은 방향 무시
        assert_eq!(p.position.qty, dec!(1)); // 기존 포지션 유지
    }

    #[test]
    fn opposite_side_closes_and_reopens() {
        let mut p = pf();
        p.execute_simulated(&mkt(OrderSide::Buy, dec!(1)), dec!(100), 1);
        let fills = p.execute_simulated(&mkt(OrderSide::Sell, dec!(1)), dec!(110), 2);
        assert_eq!(fills.len(), 2); // close fill + open fill
        assert_eq!(p.position.side, Some(OrderSide::Sell));
        assert_eq!(p.position.entry_price, dec!(110));
        assert_eq!(p.closed_trades.len(), 1);
    }

    #[test]
    fn mark_updates_unrealized_and_equity() {
        let mut p = pf();
        p.execute_simulated(&mkt(OrderSide::Buy, dec!(2)), dec!(100), 1);
        p.mark(dec!(105));
        assert_eq!(p.unrealized_pnl, dec!(10));
        assert_eq!(p.equity, dec!(10010));
    }
}
