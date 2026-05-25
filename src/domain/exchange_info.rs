use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 거래소에서 허용하는 주문 수량·가격·명목가 제약 조건입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::exchange_info::SymbolFilters;
///
/// let f = SymbolFilters {
///     symbol: "BTCUSDT".into(),
///     min_notional: dec!(5),
///     min_qty: dec!(0.001),
///     step_size: dec!(0.001),
///     tick_size: dec!(0.1),
///     max_leverage: 20,
/// };
/// assert!(f.validate_qty(dec!(0.001)).is_ok());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolFilters {
    pub symbol: String,
    pub min_notional: Decimal,
    pub min_qty: Decimal,
    pub step_size: Decimal,
    pub tick_size: Decimal,
    pub max_leverage: u32,
}

/// `SymbolFilters` 검증 실패 시 반환되는 오류 유형입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::exchange_info::FilterViolation;
///
/// let err = FilterViolation::QtyBelowMin { qty: dec!(0.0001), min_qty: dec!(0.001) };
/// assert!(err.to_string().contains("below minQty"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum FilterViolation {
    #[error("qty {qty} below minQty {min_qty}")]
    QtyBelowMin { qty: Decimal, min_qty: Decimal },
    #[error("qty {qty} not aligned to stepSize {step_size}")]
    QtyStep { qty: Decimal, step_size: Decimal },
    #[error("price {price} not aligned to tickSize {tick_size}")]
    PriceTick { price: Decimal, tick_size: Decimal },
    #[error("notional {notional} below minNotional {min_notional}")]
    NotionalBelowMin {
        notional: Decimal,
        min_notional: Decimal,
    },
}

impl SymbolFilters {
    /// 수량이 최솟값 이상이며 stepSize에 정렬됐는지 검사한다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::domain::exchange_info::SymbolFilters;
    ///
    /// let f = SymbolFilters {
    ///     symbol: "X".into(), min_notional: dec!(0), min_qty: dec!(0.001),
    ///     step_size: dec!(0.001), tick_size: dec!(0.1), max_leverage: 10,
    /// };
    /// assert!(f.validate_qty(dec!(0.001)).is_ok());
    /// assert!(f.validate_qty(dec!(0.0005)).is_err());
    /// ```
    pub fn validate_qty(&self, qty: Decimal) -> Result<(), FilterViolation> {
        if qty < self.min_qty {
            return Err(FilterViolation::QtyBelowMin {
                qty,
                min_qty: self.min_qty,
            });
        }
        if !is_aligned(qty, self.step_size) {
            return Err(FilterViolation::QtyStep {
                qty,
                step_size: self.step_size,
            });
        }
        Ok(())
    }

    /// 가격이 tickSize에 정렬됐는지 검사한다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::domain::exchange_info::SymbolFilters;
    ///
    /// let f = SymbolFilters {
    ///     symbol: "X".into(), min_notional: dec!(0), min_qty: dec!(0.001),
    ///     step_size: dec!(0.001), tick_size: dec!(0.1), max_leverage: 10,
    /// };
    /// assert!(f.validate_price(dec!(100.1)).is_ok());
    /// assert!(f.validate_price(dec!(100.05)).is_err());
    /// ```
    pub fn validate_price(&self, price: Decimal) -> Result<(), FilterViolation> {
        if !is_aligned(price, self.tick_size) {
            return Err(FilterViolation::PriceTick {
                price,
                tick_size: self.tick_size,
            });
        }
        Ok(())
    }

    /// 명목가(qty × price)가 minNotional 이상인지 검사한다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::domain::exchange_info::SymbolFilters;
    ///
    /// let f = SymbolFilters {
    ///     symbol: "X".into(), min_notional: dec!(5), min_qty: dec!(0.001),
    ///     step_size: dec!(0.001), tick_size: dec!(0.1), max_leverage: 10,
    /// };
    /// assert!(f.validate_notional(dec!(0.01), dec!(1000)).is_ok());
    /// assert!(f.validate_notional(dec!(0.001), dec!(1)).is_err());
    /// ```
    pub fn validate_notional(&self, qty: Decimal, price: Decimal) -> Result<(), FilterViolation> {
        let notional = qty * price;
        if notional < self.min_notional {
            return Err(FilterViolation::NotionalBelowMin {
                notional,
                min_notional: self.min_notional,
            });
        }
        Ok(())
    }
}

// step이 0이면 정렬 조건 없음 (거래소 스펙). value % step == 0 이면 정렬된 것으로 판단한다.
fn is_aligned(value: Decimal, step: Decimal) -> bool {
    if step.is_zero() {
        return true;
    }
    (value % step).is_zero()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn filters() -> SymbolFilters {
        SymbolFilters {
            symbol: "BTCUSDT".into(),
            min_notional: dec!(5),
            min_qty: dec!(0.001),
            step_size: dec!(0.001),
            tick_size: dec!(0.1),
            max_leverage: 20,
        }
    }

    #[test]
    fn validate_qty_rejects_misaligned_or_below_min() {
        let f = filters();
        assert!(f.validate_qty(dec!(0.0005)).is_err());
        assert!(f.validate_qty(dec!(0.0015)).is_err());
        assert!(f.validate_qty(dec!(0.001)).is_ok());
    }

    #[test]
    fn validate_notional_rejects_below_min() {
        let f = filters();
        assert!(f.validate_notional(dec!(0.001), dec!(1000)).is_err());
        assert!(f.validate_notional(dec!(0.01), dec!(1000)).is_ok());
    }

    #[test]
    fn validate_price_rejects_misaligned_tick() {
        let f = filters();
        assert!(f.validate_price(dec!(100.05)).is_err());
        assert!(f.validate_price(dec!(100.1)).is_ok());
    }
}
