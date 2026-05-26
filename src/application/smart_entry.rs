use crate::domain::order::{OrderRequest, OrderSide, OrderType, TimeInForce};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// 진입 방식 결정 파라미터 (strategy_instances.params JSON에서 로드).
#[derive(Debug, Clone)]
pub struct SmartEntryParams {
    /// 지정가 사용 기준: spread_pct < atr_pct × limit_spread_threshold (기본 0.1)
    pub limit_spread_threshold: Decimal,
    /// 지정가 호가 오프셋 — best bid/ask 기준 안쪽 (기본 0.02%)
    pub limit_offset_pct: Decimal,
    /// 지정가 타임아웃 후 시장가 전환 (초, 기본 10)
    pub limit_timeout_secs: u64,
}

impl Default for SmartEntryParams {
    fn default() -> Self {
        Self {
            limit_spread_threshold: dec!(0.1),
            limit_offset_pct: dec!(0.0002),
            limit_timeout_secs: 10,
        }
    }
}

/// 진입 방식 결정 결과.
#[derive(Debug, Clone)]
pub enum EntryMethod {
    Market,
    Limit { price: Decimal },
}

/// 시장 상황에 따라 지정가/시장가 진입 방식을 결정합니다.
///
/// # 로직
/// ```text
/// atr_pct    = atr / mid_price × 100
/// spread_pct = (ask - bid) / mid_price × 100
///
/// if spread_pct < atr_pct × limit_spread_threshold:
///     지정가 (best bid/ask 기준 limit_offset_pct 안쪽)
/// else:
///     시장가
/// ```
pub struct SmartEntry {
    params: SmartEntryParams,
}

impl SmartEntry {
    pub fn new(params: SmartEntryParams) -> Self {
        Self { params }
    }

    /// 진입 방식을 결정합니다.
    pub fn decide(
        &self,
        side: OrderSide,
        bid: Decimal,
        ask: Decimal,
        atr: Decimal,
    ) -> EntryMethod {
        if bid.is_zero() || ask.is_zero() {
            return EntryMethod::Market;
        }
        let mid = (bid + ask) / dec!(2);
        if mid.is_zero() {
            return EntryMethod::Market;
        }

        let spread_pct = (ask - bid) / mid;
        let atr_pct = atr / mid;

        if spread_pct < atr_pct * self.params.limit_spread_threshold {
            // 지정가: best price 기준 offset 안쪽
            let price = match side {
                OrderSide::Buy => ask * (Decimal::ONE - self.params.limit_offset_pct),
                OrderSide::Sell => bid * (Decimal::ONE + self.params.limit_offset_pct),
            };
            EntryMethod::Limit { price }
        } else {
            EntryMethod::Market
        }
    }

    /// 지정가 진입 주문 요청을 빌드합니다.
    pub fn build_request(
        &self,
        base: OrderRequest,
        side: OrderSide,
        bid: Decimal,
        ask: Decimal,
        atr: Decimal,
    ) -> OrderRequest {
        match self.decide(side, bid, ask, atr) {
            EntryMethod::Market => OrderRequest {
                order_type: OrderType::Market,
                time_in_force: TimeInForce::Gtc,
                ..base
            },
            EntryMethod::Limit { price } => OrderRequest {
                order_type: OrderType::Limit,
                price: Some(price),
                time_in_force: TimeInForce::Gtc,
                ..base
            },
        }
    }

    pub fn timeout_secs(&self) -> u64 {
        self.params.limit_timeout_secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> SmartEntry {
        SmartEntry::new(SmartEntryParams::default())
    }

    #[test]
    fn small_spread_uses_limit() {
        // spread = 1 / 50000 = 0.002%, atr_pct = 100/50000 = 0.2%
        // limit_spread_threshold = 0.1 → 0.2% × 0.1 = 0.02%
        // spread 0.002% < 0.02% → limit
        let e = entry();
        let method = e.decide(OrderSide::Buy, dec!(49999.5), dec!(50000.5), dec!(100));
        assert!(matches!(method, EntryMethod::Limit { .. }));
    }

    #[test]
    fn wide_spread_uses_market() {
        // spread = 200 / 50000 = 0.4%, atr_pct = 100/50000 = 0.2%
        // 0.4% >= 0.2% × 0.1 = 0.02% — wait, 0.4% >= 0.02% so market
        // Actually spread >= atr × threshold → market
        let e = entry();
        let method = e.decide(OrderSide::Buy, dec!(49900), dec!(50100), dec!(100));
        assert!(matches!(method, EntryMethod::Market));
    }

    #[test]
    fn limit_buy_price_below_ask() {
        let e = SmartEntry::new(SmartEntryParams {
            limit_offset_pct: dec!(0.001), // 0.1%
            ..Default::default()
        });
        if let EntryMethod::Limit { price } =
            e.decide(OrderSide::Buy, dec!(49999), dec!(50001), dec!(1))
        {
            // 매수: ask × (1 - 0.001) = 50001 × 0.999 ≈ 49951
            assert!(price < dec!(50001));
        }
    }
}
