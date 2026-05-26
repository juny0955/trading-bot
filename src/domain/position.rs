use crate::domain::order::{Fill, OrderSide};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

/// 체결 적용 결과로 포지션이 어떻게 변했는지를 나타냅니다.
#[derive(Debug, Clone)]
pub enum PositionAction {
    /// 플랫 상태에서 신규 포지션을 열었습니다.
    Opened,
    /// 기존 포지션과 같은 방향으로 수량을 추가했습니다.
    Increased { new_avg_entry_price: Decimal },
    /// 포지션 일부를 청산했습니다.
    PartialClose {
        closed_qty: Decimal,
        this_pnl: Decimal,
        pnl_pct: Decimal,
    },
    /// 포지션 전량을 청산했습니다.
    FullClose { this_pnl: Decimal, pnl_pct: Decimal },
    /// 기존 포지션을 전량 청산하고 반대 방향으로 진입했습니다.
    Flipped {
        closed_qty: Decimal,
        this_pnl: Decimal,
        pnl_pct: Decimal,
    },
}

/// 특정 전략과 심볼의 현재 포지션입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::order::OrderSide;
/// use trading_bot::domain::position::Position;
///
/// let position = Position {
///     side: Some(OrderSide::Buy),
///     qty: dec!(2),
///     avg_entry_price: dec!(100),
///     realized_pnl: dec!(0),
/// };
///
/// assert_eq!(position.unrealized(dec!(105)), dec!(10));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Position {
    pub side: Option<OrderSide>, // qty == 0 일 때 None
    pub qty: Decimal,
    pub avg_entry_price: Decimal,
    pub realized_pnl: Decimal,
}

/// 포지션 계산 중 발생할 수 있는 오류입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::domain::position::PositionError;
///
/// let err = PositionError::InvalidFill("qty must be positive".into());
/// assert!(err.to_string().contains("invalid fill"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum PositionError {
    #[error("invalid fill: {0}")]
    InvalidFill(String),
    #[error("invalid position: {0}")]
    InvalidPosition(String),
}

impl Position {
    /// 체결을 반영한 새 포지션을 계산합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use chrono::Utc;
    /// use rust_decimal_macros::dec;
    /// use trading_bot::domain::order::{Fill, OrderSide};
    /// use trading_bot::domain::position::Position;
    /// use uuid::Uuid;
    ///
    /// let fill = Fill {
    ///     order_id: Uuid::nil(),
    ///     strategy_instance_id: 1,
    ///     symbol: "BTCUSDT".into(),
    ///     side: OrderSide::Buy,
    ///     qty: dec!(1),
    ///     price: dec!(100),
    ///     fee: dec!(0),
    ///     fee_asset: "USDT".into(),
    ///     filled_at: Utc::now(),
    ///     exchange_trade_id: 1,
    /// };
    ///
    /// let (position, _) = Position::default().apply(&fill).unwrap();
    /// assert_eq!(position.side, Some(OrderSide::Buy));
    /// ```
    pub fn apply(&self, fill: &Fill) -> Result<(Self, PositionAction), PositionError> {
        validate_fill(fill)?;
        validate_position(self)?;

        Ok(apply_valid_fill(*self, fill))
    }

    /// 현재 mark 가격 기준 미실현 손익을 계산합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::domain::order::OrderSide;
    /// use trading_bot::domain::position::Position;
    ///
    /// let position = Position {
    ///     side: Some(OrderSide::Sell),
    ///     qty: dec!(1),
    ///     avg_entry_price: dec!(100),
    ///     realized_pnl: dec!(0),
    /// };
    ///
    /// assert_eq!(position.unrealized(dec!(90)), dec!(10));
    /// ```
    pub fn unrealized(&self, mark: Decimal) -> Decimal {
        match self.side {
            Some(OrderSide::Buy) => (mark - self.avg_entry_price) * self.qty,
            Some(OrderSide::Sell) => (self.avg_entry_price - mark) * self.qty,
            None => Decimal::ZERO,
        }
    }
}

fn validate_fill(fill: &Fill) -> Result<(), PositionError> {
    validate_fill_positive("qty", fill.qty)?;
    validate_fill_positive("price", fill.price)?;

    if fill.fee < Decimal::ZERO {
        return Err(PositionError::InvalidFill(
            "fee must not be negative".into(),
        ));
    }

    Ok(())
}

fn validate_position(position: &Position) -> Result<(), PositionError> {
    match (position.side, position.qty.is_zero()) {
        (None, true) => Ok(()),
        (None, false) => Err(PositionError::InvalidPosition(
            "flat position must have zero qty".into(),
        )),
        (Some(_), true) => Err(PositionError::InvalidPosition(
            "open position must have positive qty".into(),
        )),
        (Some(_), false) => validate_open_position(position),
    }
}

fn validate_open_position(position: &Position) -> Result<(), PositionError> {
    validate_position_positive("qty", position.qty)?;
    validate_position_positive("avg_entry_price", position.avg_entry_price)
}

fn validate_fill_positive(name: &str, value: Decimal) -> Result<(), PositionError> {
    if value <= Decimal::ZERO {
        return Err(PositionError::InvalidFill(format!(
            "{name} must be positive"
        )));
    }
    Ok(())
}

fn validate_position_positive(name: &str, value: Decimal) -> Result<(), PositionError> {
    if value <= Decimal::ZERO {
        return Err(PositionError::InvalidPosition(format!(
            "{name} must be positive"
        )));
    }
    Ok(())
}

fn apply_valid_fill(position: Position, fill: &Fill) -> (Position, PositionAction) {
    // PnL 델타와 수익률 계산을 위해 수수료 차감 전 원본 값을 먼저 캡처한다.
    let orig_pnl = position.realized_pnl;
    let orig_avg = position.avg_entry_price;
    let orig_qty = position.qty;

    let mut pos = position;
    // 수수료는 거래 방향과 무관하게 체결 시점에 즉시 실현 손익에서 차감한다.
    pos.realized_pnl -= fill.fee;

    match pos.side {
        None => (
            open_position(pos.realized_pnl, fill),
            PositionAction::Opened,
        ),
        Some(side) if side == fill.side => {
            let new_pos = increase_position(pos, fill);
            let action = PositionAction::Increased {
                new_avg_entry_price: new_pos.avg_entry_price,
            };
            (new_pos, action)
        }
        Some(side) => {
            let new_pos = reduce_or_flip_position(pos, side, fill);
            let this_pnl = new_pos.realized_pnl - orig_pnl;
            let close_qty = fill.qty.min(orig_qty);
            let cost_basis = orig_avg * close_qty;
            let pnl_pct = if cost_basis.is_zero() {
                Decimal::ZERO
            } else {
                (this_pnl / cost_basis) * Decimal::from(100u32)
            };
            let action = if fill.qty > orig_qty {
                PositionAction::Flipped {
                    closed_qty: orig_qty,
                    this_pnl,
                    pnl_pct,
                }
            } else if fill.qty == orig_qty {
                PositionAction::FullClose { this_pnl, pnl_pct }
            } else {
                PositionAction::PartialClose {
                    closed_qty: fill.qty,
                    this_pnl,
                    pnl_pct,
                }
            };
            (new_pos, action)
        }
    }
}

fn open_position(realized_pnl: Decimal, fill: &Fill) -> Position {
    Position {
        side: Some(fill.side),
        qty: fill.qty,
        avg_entry_price: fill.price,
        realized_pnl,
    }
}

fn increase_position(position: Position, fill: &Fill) -> Position {
    let new_qty = position.qty + fill.qty;

    Position {
        qty: new_qty,
        avg_entry_price: weighted_average_price(position, fill, new_qty),
        ..position
    }
}

fn reduce_or_flip_position(position: Position, current_side: OrderSide, fill: &Fill) -> Position {
    if fill.qty <= position.qty {
        reduce_position(position, current_side, fill)
    } else {
        flip_position(position, current_side, fill)
    }
}

fn reduce_position(position: Position, current_side: OrderSide, fill: &Fill) -> Position {
    let remaining_qty = position.qty - fill.qty;
    // 청산된 수량(fill.qty)에 해당하는 gross PnL을 누적한다.
    let realized_pnl = position.realized_pnl
        + gross_pnl(current_side, position.avg_entry_price, fill.price, fill.qty);

    // remaining_qty == 0이면 포지션이 전량 청산됐으므로 flat(side=None) 상태로 전환한다.
    if remaining_qty.is_zero() {
        return Position {
            side: None,
            qty: Decimal::ZERO,
            avg_entry_price: Decimal::ZERO,
            realized_pnl,
        };
    }

    Position {
        qty: remaining_qty,
        realized_pnl,
        ..position
    }
}

fn flip_position(position: Position, current_side: OrderSide, fill: &Fill) -> Position {
    // fill.qty > position.qty: 기존 포지션 전량을 청산(close_qty)하고 초과분(open_qty)으로 반대 포지션을 연다.
    let close_qty = position.qty;
    let open_qty = fill.qty - close_qty;
    // 청산 부분(close_qty)에 대한 PnL만 누적한다. 신규 포지션의 PnL은 이후 체결로 계산된다.
    let realized_pnl = position.realized_pnl
        + gross_pnl(
            current_side,
            position.avg_entry_price,
            fill.price,
            close_qty,
        );

    Position {
        side: Some(fill.side),
        qty: open_qty,
        avg_entry_price: fill.price,
        realized_pnl,
    }
}

fn weighted_average_price(position: Position, fill: &Fill, new_qty: Decimal) -> Decimal {
    // (기존가 × 기존수량 + 신규가 × 신규수량) / 총수량
    (position.avg_entry_price * position.qty + fill.price * fill.qty) / new_qty
}

fn gross_pnl(
    current_side: OrderSide,
    entry_price: Decimal,
    exit_price: Decimal,
    qty: Decimal,
) -> Decimal {
    // LONG: 가격 상승이 이익(exit - entry), SHORT: 가격 하락이 이익(entry - exit)
    match current_side {
        OrderSide::Buy => (exit_price - entry_price) * qty,
        OrderSide::Sell => (entry_price - exit_price) * qty,
    }
}

/// 저장소에 보관되는 포지션과 메타데이터입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::domain::position::StoredPosition;
///
/// let stored = StoredPosition::flat(1, "BTCUSDT".into());
/// assert!(stored.position.side.is_none());
/// ```
#[derive(Debug, Clone)]
pub struct StoredPosition {
    pub strategy_instance_id: i64,
    pub symbol: String,
    pub position: Position,
    pub last_fill_id: Option<i64>,
    pub updated_at: DateTime<Utc>,
}

impl StoredPosition {
    /// 포지션이 없는 저장 상태를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use trading_bot::domain::position::StoredPosition;
    ///
    /// let stored = StoredPosition::flat(10, "ETHUSDT".into());
    /// assert_eq!(stored.strategy_instance_id, 10);
    /// ```
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
    use anyhow::Result;
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
            exchange_trade_id: -1,
            position_lifecycle_id: None,
        }
    }

    #[test]
    fn open_from_flat() -> Result<()> {
        let (p, action) =
            Position::default().apply(&fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0.05)))?;
        assert_eq!(p.side, Some(OrderSide::Buy));
        assert_eq!(p.qty, dec!(1));
        assert_eq!(p.avg_entry_price, dec!(100));
        assert_eq!(p.realized_pnl, dec!(-0.05));
        assert!(matches!(action, PositionAction::Opened));
        Ok(())
    }

    #[test]
    fn pyramid_same_side() -> Result<()> {
        let (p1, _) =
            Position::default().apply(&fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0)))?;
        let (p, action) = p1.apply(&fill(OrderSide::Buy, dec!(1), dec!(120), dec!(0)))?;
        assert_eq!(p.qty, dec!(2));
        assert_eq!(p.avg_entry_price, dec!(110));
        assert!(matches!(action, PositionAction::Increased { .. }));
        Ok(())
    }

    #[test]
    fn partial_close() -> Result<()> {
        let (p1, _) =
            Position::default().apply(&fill(OrderSide::Buy, dec!(2), dec!(100), dec!(0)))?;
        let (p, action) = p1.apply(&fill(OrderSide::Sell, dec!(1), dec!(110), dec!(0)))?;
        assert_eq!(p.side, Some(OrderSide::Buy));
        assert_eq!(p.qty, dec!(1));
        assert_eq!(p.realized_pnl, dec!(10));
        assert!(matches!(action, PositionAction::PartialClose { .. }));
        Ok(())
    }

    #[test]
    fn full_close_to_flat() -> Result<()> {
        let (p1, _) =
            Position::default().apply(&fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0)))?;
        let (p, action) = p1.apply(&fill(OrderSide::Sell, dec!(1), dec!(110), dec!(0)))?;
        assert_eq!(p.side, None);
        assert_eq!(p.qty, dec!(0));
        assert_eq!(p.avg_entry_price, dec!(0));
        assert_eq!(p.realized_pnl, dec!(10));
        assert!(matches!(action, PositionAction::FullClose { .. }));
        Ok(())
    }

    #[test]
    fn flip_long_to_short() -> Result<()> {
        let (p1, _) =
            Position::default().apply(&fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0)))?;
        let (p, action) = p1.apply(&fill(OrderSide::Sell, dec!(3), dec!(110), dec!(0)))?;
        assert_eq!(p.side, Some(OrderSide::Sell));
        assert_eq!(p.qty, dec!(2));
        assert_eq!(p.avg_entry_price, dec!(110));
        assert_eq!(p.realized_pnl, dec!(10));
        assert!(matches!(action, PositionAction::Flipped { .. }));
        Ok(())
    }

    #[test]
    fn fee_accumulates_into_realized() -> Result<()> {
        let (p1, _) =
            Position::default().apply(&fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0.1)))?;
        let (p, _) = p1.apply(&fill(OrderSide::Sell, dec!(1), dec!(100), dec!(0.1)))?;
        assert_eq!(p.realized_pnl, dec!(-0.2));
        Ok(())
    }

    #[test]
    fn unrealized_mark() -> Result<()> {
        let (p, _) =
            Position::default().apply(&fill(OrderSide::Buy, dec!(2), dec!(100), dec!(0)))?;
        assert_eq!(p.unrealized(dec!(105)), dec!(10));
        Ok(())
    }

    #[test]
    fn full_close_pnl_action() -> Result<()> {
        let (p1, _) =
            Position::default().apply(&fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0)))?;
        let (_, action) = p1.apply(&fill(OrderSide::Sell, dec!(1), dec!(110), dec!(0)))?;
        match action {
            PositionAction::FullClose { this_pnl, pnl_pct } => {
                assert_eq!(this_pnl, dec!(10));
                assert_eq!(pnl_pct, dec!(10));
            }
            _ => panic!("expected FullClose"),
        }
        Ok(())
    }

    #[test]
    fn partial_close_pnl_action() -> Result<()> {
        let (p1, _) =
            Position::default().apply(&fill(OrderSide::Buy, dec!(2), dec!(100), dec!(0)))?;
        let (_, action) = p1.apply(&fill(OrderSide::Sell, dec!(1), dec!(110), dec!(0)))?;
        match action {
            PositionAction::PartialClose {
                closed_qty,
                this_pnl,
                pnl_pct,
            } => {
                assert_eq!(closed_qty, dec!(1));
                assert_eq!(this_pnl, dec!(10));
                assert_eq!(pnl_pct, dec!(10));
            }
            _ => panic!("expected PartialClose"),
        }
        Ok(())
    }

    #[test]
    fn flipped_pnl_action() -> Result<()> {
        let (p1, _) =
            Position::default().apply(&fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0)))?;
        let (_, action) = p1.apply(&fill(OrderSide::Sell, dec!(3), dec!(110), dec!(0)))?;
        match action {
            PositionAction::Flipped {
                closed_qty,
                this_pnl,
                pnl_pct,
            } => {
                assert_eq!(closed_qty, dec!(1));
                assert_eq!(this_pnl, dec!(10));
                assert_eq!(pnl_pct, dec!(10));
            }
            _ => panic!("expected Flipped"),
        }
        Ok(())
    }

    #[test]
    fn invalid_fill_qty_returns_error() -> Result<()> {
        assert!(matches!(
            Position::default().apply(&fill(OrderSide::Buy, dec!(0), dec!(100), dec!(0))),
            Err(PositionError::InvalidFill(_))
        ));
        Ok(())
    }
}
