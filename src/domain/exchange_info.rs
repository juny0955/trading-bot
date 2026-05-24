use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolFilters {
    pub symbol: String,
    pub min_notional: Decimal,
    pub min_qty: Decimal,
    pub step_size: Decimal,
    pub tick_size: Decimal,
    pub max_leverage: u32,
}

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

    pub fn validate_price(&self, price: Decimal) -> Result<(), FilterViolation> {
        if !is_aligned(price, self.tick_size) {
            return Err(FilterViolation::PriceTick {
                price,
                tick_size: self.tick_size,
            });
        }
        Ok(())
    }

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
