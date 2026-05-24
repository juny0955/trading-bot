use crate::domain::order::{Fill, OrderSide};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

#[derive(Debug, Clone, Default)]
pub struct Position {
    pub side: Option<OrderSide>, // qty == 0 일 때 None
    pub qty: Decimal,
    pub avg_entry_price: Decimal,
    pub realized_pnl: Decimal,
}

impl Position {
    pub fn apply(&self, fill: &Fill) -> Self {
        let mut next = self.clone();
        next.realized_pnl -= fill.fee;

        match next.side {
            // 무포지션 -> 신규 진입
            None => {
                next.side = Some(fill.side);
                next.qty = fill.qty;
                next.avg_entry_price = fill.price;
            }
            // 같은 방향 -> 평단가 조정 및 수량 증가
            Some(cur) if cur == fill.side => {
                let new_qty = next.qty + fill.qty;
                next.avg_entry_price =
                    (next.avg_entry_price * next.qty + fill.price * fill.qty) / new_qty;
                next.qty = new_qty;
            }
            // 반대 방향
            Some(cur) => {
                if fill.qty <= next.qty {
                    // 부분/전체 청산
                    let close_qty = fill.qty;
                    let gross = match cur {
                        OrderSide::Buy => (fill.price - next.avg_entry_price) * close_qty,
                        OrderSide::Sell => (next.avg_entry_price - fill.price) * close_qty,
                    };

                    next.realized_pnl += gross;
                    next.qty -= close_qty;
                    if next.qty.is_zero() {
                        next.side = None;
                        next.avg_entry_price = Decimal::ZERO;
                    }
                } else {
                    // 청산 + 반전
                    let close_qty = next.qty;
                    let open_qty = fill.qty - close_qty;
                    let gross = match cur {
                        OrderSide::Buy => (fill.price - next.avg_entry_price) * close_qty,
                        OrderSide::Sell => (next.avg_entry_price - fill.price) * close_qty,
                    };
                    next.realized_pnl += gross;
                    next.side = Some(fill.side);
                    next.qty = open_qty;
                    next.avg_entry_price = fill.price;
                }
            }
        }
        next
    }

    pub fn unrealized(&self, mark: Decimal) -> Decimal {
        match self.side {
            Some(OrderSide::Buy) => (mark - self.avg_entry_price) * self.qty,
            Some(OrderSide::Sell) => (self.avg_entry_price - mark) * self.qty,
            None => Decimal::ZERO,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoredPosition {
    pub strategy_instance_id: i64,
    pub symbol: String,
    pub position: Position,
    pub last_fill_id: Option<i64>,
    pub updated_at: DateTime<Utc>,
}

impl StoredPosition {
    pub fn flat(strategy_instance_id: i64, symbol: String) -> Self {
        Self {
            strategy_instance_id,
            symbol,
            position: Position::default(),
            last_fill_id: None,
            updated_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn fill(side: OrderSide, qty: Decimal, price: Decimal, fee: Decimal) -> Fill {
        Fill {
            order_id: Uuid::nil(),
            strategy_instance_id: 1,
            symbol: "BTCUSDT".into(),
            side,
            qty,
            price,
            fee,
            fee_asset: "USDT".into(),
            filled_at: Utc::now(),
        }
    }

    #[test]
    fn open_from_flat() {
        let p =
            Position::default().apply(&fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0.05)));
        assert_eq!(p.side, Some(OrderSide::Buy));
        assert_eq!(p.qty, dec!(1));
        assert_eq!(p.avg_entry_price, dec!(100));
        assert_eq!(p.realized_pnl, dec!(-0.05));
    }

    #[test]
    fn pyramid_same_side() {
        let p = Position::default()
            .apply(&fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0)))
            .apply(&fill(OrderSide::Buy, dec!(1), dec!(120), dec!(0)));
        assert_eq!(p.qty, dec!(2));
        assert_eq!(p.avg_entry_price, dec!(110));
    }

    #[test]
    fn partial_close() {
        let p = Position::default()
            .apply(&fill(OrderSide::Buy, dec!(2), dec!(100), dec!(0)))
            .apply(&fill(OrderSide::Sell, dec!(1), dec!(110), dec!(0)));
        assert_eq!(p.side, Some(OrderSide::Buy));
        assert_eq!(p.qty, dec!(1));
        assert_eq!(p.realized_pnl, dec!(10));
    }

    #[test]
    fn full_close_to_flat() {
        let p = Position::default()
            .apply(&fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0)))
            .apply(&fill(OrderSide::Sell, dec!(1), dec!(110), dec!(0)));
        assert_eq!(p.side, None);
        assert_eq!(p.qty, dec!(0));
        assert_eq!(p.avg_entry_price, dec!(0));
        assert_eq!(p.realized_pnl, dec!(10));
    }

    #[test]
    fn flip_long_to_short() {
        let p = Position::default()
            .apply(&fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0)))
            .apply(&fill(OrderSide::Sell, dec!(3), dec!(110), dec!(0)));
        assert_eq!(p.side, Some(OrderSide::Sell));
        assert_eq!(p.qty, dec!(2));
        assert_eq!(p.avg_entry_price, dec!(110));
        assert_eq!(p.realized_pnl, dec!(10));
    }

    #[test]
    fn fee_accumulates_into_realized() {
        let p = Position::default()
            .apply(&fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0.1)))
            .apply(&fill(OrderSide::Sell, dec!(1), dec!(100), dec!(0.1)));
        assert_eq!(p.realized_pnl, dec!(-0.2));
    }

    #[test]
    fn unrealized_mark() {
        let p =
            Position::default().apply(&fill(OrderSide::Buy, dec!(2), dec!(100), dec!(0)));
        assert_eq!(p.unrealized(dec!(105)), dec!(10));
    }
}
