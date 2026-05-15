use crate::backtest::types::{BacktestOrder, Fill, Position, Side};
use rust_decimal::Decimal;
use rust_decimal::prelude::Zero;

const BPS_DIVISOR: Decimal = Decimal::from_parts(10_000, 0, 0, false, 0);

#[derive(Debug, Clone)]
pub struct ClosedTrade {
    pub entry_ts_ns: i64,
    pub exit_ts_ns: i64,
    pub side: Side,
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

    pub fn execute(&mut self, order: &BacktestOrder, price: Decimal, ts_ns: i64) -> Vec<Fill> {
        match order {
            BacktestOrder::Close => self.close_at(price, ts_ns).into_iter().collect(),
            BacktestOrder::Market { side, qty } => {
                let mut fills = Vec::new();
                if let Some(cur) = self.position.side {
                    if cur != *side {
                        if let Some(f) = self.close_at(price, ts_ns) {
                            fills.push(f);
                        }
                    } else {
                        // v1: 같은 방향 추가매수 무시
                        return fills;
                    }
                }
                fills.push(self.open_at(*side, *qty, price, ts_ns));
                fills
            }
        }
    }

    fn open_at(&mut self, side: Side, qty: Decimal, price: Decimal, ts_ns: i64) -> Fill {
        let fee = self.fee_of(price, qty);
        self.position = Position {
            side: Some(side),
            qty,
            entry_price: price,
            entry_ts: ts_ns,
        };
        Fill {
            ts_ns,
            side,
            qty,
            price,
            fee,
        }
    }

    fn close_at(&mut self, price: Decimal, ts_ns: i64) -> Option<Fill> {
        let pos_side = self.position.side?;
        let qty = self.position.qty;
        let entry_price = self.position.entry_price;
        let entry_ts = self.position.entry_ts;

        let entry_fee = self.fee_of(entry_price, qty);
        let exit_fee = self.fee_of(price, qty);
        let fees = entry_fee + exit_fee;
        let gross = match pos_side {
            Side::Long => (price - entry_price) * qty,
            Side::Short => (entry_price - price) * qty,
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

        Some(Fill {
            ts_ns,
            side: pos_side,
            qty,
            price,
            fee: exit_fee,
        })
    }

    pub fn mark(&mut self, mark_price: Decimal) {
        self.unrealized_pnl = match self.position.side {
            Some(Side::Long) => (mark_price - self.position.entry_price) * self.position.qty,
            Some(Side::Short) => (self.position.entry_price - mark_price) * self.position.qty,
            None => Decimal::zero(),
        };
        self.equity = self.initial_equity + self.realized_pnl + self.unrealized_pnl;
    }

    fn fee_of(&self, price: Decimal, qty: Decimal) -> Decimal {
        price * qty * self.fee_bps / BPS_DIVISOR
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn pf() -> Portfolio {
        Portfolio::new(dec!(10000), dec!(4))
    }

    #[test]
    fn long_open_close_net_pnl() {
        let mut p = pf();
        p.execute(
            &BacktestOrder::Market {
                side: Side::Long,
                qty: dec!(1),
            },
            dec!(100),
            1,
        );
        p.execute(&BacktestOrder::Close, dec!(110), 2);

        let t = p.closed_trades.last().unwrap();
        assert_eq!(t.gross_pnl, dec!(10));
        assert_eq!(t.fees, dec!(0.0840));
        assert_eq!(t.net_pnl, dec!(9.9160));
        assert_eq!(p.realized_pnl, dec!(9.9160));
    }

    #[test]
    fn short_open_close_net_pnl() {
        let mut p = pf();
        p.execute(
            &BacktestOrder::Market {
                side: Side::Short,
                qty: dec!(1),
            },
            dec!(100),
            1,
        );
        p.execute(&BacktestOrder::Close, dec!(90), 2);

        let t = p.closed_trades.last().unwrap();
        assert_eq!(t.gross_pnl, dec!(10));
        assert_eq!(t.fees, dec!(0.0760));
        assert_eq!(t.net_pnl, dec!(9.9240));
    }

    #[test]
    fn close_when_flat_is_noop() {
        let mut p = pf();
        let fills = p.execute(&BacktestOrder::Close, dec!(100), 1);
        assert!(fills.is_empty());
        assert!(p.closed_trades.is_empty());
        assert!(p.position.side.is_none());
    }

    #[test]
    fn opposite_market_closes_and_reopens() {
        let mut p = pf();
        p.execute(
            &BacktestOrder::Market {
                side: Side::Long,
                qty: dec!(1),
            },
            dec!(100),
            1,
        );
        let fills = p.execute(
            &BacktestOrder::Market {
                side: Side::Short,
                qty: dec!(1),
            },
            dec!(110),
            2,
        );
        assert_eq!(fills.len(), 2);
        assert_eq!(p.position.side, Some(Side::Short));
        assert_eq!(p.position.entry_price, dec!(110));
        assert_eq!(p.closed_trades.len(), 1);
    }

    #[test]
    fn mark_updates_unrealized_and_equity() {
        let mut p = pf();
        p.execute(
            &BacktestOrder::Market {
                side: Side::Long,
                qty: dec!(2),
            },
            dec!(100),
            1,
        );
        p.mark(dec!(105));
        assert_eq!(p.unrealized_pnl, dec!(10));
        assert_eq!(p.equity, dec!(10010));
    }
}
