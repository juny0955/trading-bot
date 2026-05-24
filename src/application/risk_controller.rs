use crate::adapters::outbound::cache::book_ticker_cache::BookTickerCache;
use crate::adapters::outbound::cache::symbol_filters_cache::SymbolFiltersCache;
use crate::application::account_state::AccountState;
use crate::domain::exchange_info::{FilterViolation, SymbolFilters};
use crate::domain::market_data::BookTicker;
use crate::domain::order::{OrderRequest, OrderSide, OrderType};
use crate::ports::outbound::order_repository::OrderRepository;
use crate::ports::outbound::position_repository::PositionRepository;
use crate::ports::outbound::risk_config_source::RiskLimits;
use arc_swap::ArcSwap;
use rust_decimal::Decimal;
use std::sync::Arc;

/// 주문 사전 검증 시 발생할 수 있는 오류.
#[derive(Debug, thiserror::Error)]
pub enum RiskError {
    #[error("알 수 없는 심볼: {0}")]
    UnknownSymbol(String),
    #[error("필터 위반: {0}")]
    FilterViolation(#[from] FilterViolation),
    #[error("증거금 부족: available={available}, required={required}")]
    InsufficientMargin {
        available: Decimal,
        required: Decimal,
    },
    #[error("포지션 한도 초과: projected={projected}, max={max}")]
    PositionLimitExceeded { projected: Decimal, max: Decimal },
    #[error("미체결 주문 한도 초과: {0}개")]
    TooManyOpenOrders(u32),
    #[error("호가 정보 없음: {0}")]
    NoBookTicker(String),
    #[error("내부 오류: {0}")]
    Internal(String),
}

/// 거래소 전송 전 증거금·필터·포지션 한도·주문 수를 검사하는 가드레일.
pub struct RiskController {
    limits: Arc<ArcSwap<RiskLimits>>,
    filters_cache: Arc<SymbolFiltersCache>,
    account: Arc<AccountState>,
    order_repo: Arc<dyn OrderRepository>,
    position_repo: Arc<dyn PositionRepository>,
    book_ticker_cache: Arc<BookTickerCache>,
}

impl RiskController {
    /// 새 `RiskController`를 생성한다.
    pub fn new(
        limits: Arc<ArcSwap<RiskLimits>>,
        filters_cache: Arc<SymbolFiltersCache>,
        account: Arc<AccountState>,
        order_repo: Arc<dyn OrderRepository>,
        position_repo: Arc<dyn PositionRepository>,
        book_ticker_cache: Arc<BookTickerCache>,
    ) -> Self {
        Self {
            limits,
            filters_cache,
            account,
            order_repo,
            position_repo,
            book_ticker_cache,
        }
    }

    /// 주문 요청의 사전 검증을 순서대로 수행한다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// risk.check(&order_request).await?;
    /// ```
    pub async fn check(&self, req: &OrderRequest) -> Result<(), RiskError> {
        let limits = (*self.limits.load()).clone();
        let filters = self
            .filters_cache
            .get(&req.symbol)
            .ok_or_else(|| RiskError::UnknownSymbol(req.symbol.clone()))?;

        self.validate_filters(req, &filters)?;
        let est_notional = self.validate_margin(req, &limits, &filters)?;
        self.validate_position_limit(req, &limits, est_notional).await?;
        self.validate_open_order_count(req, &limits).await?;

        Ok(())
    }

    fn validate_filters(&self, req: &OrderRequest, filters: &SymbolFilters) -> Result<(), RiskError> {
        filters.validate_qty(req.qty)?;
        if let Some(price) = req.price {
            filters.validate_price(price)?;
            filters.validate_notional(req.qty, price)?;
        }
        Ok(())
    }

    fn validate_margin(
        &self,
        req: &OrderRequest,
        limits: &RiskLimits,
        filters: &SymbolFilters,
    ) -> Result<Decimal, RiskError> {
        let est_notional = self.estimate_notional(req, limits)?;
        let leverage = std::cmp::min(limits.max_leverage, filters.max_leverage);
        let required_margin = calculate_required_margin(est_notional, leverage);
        let available = self.account.available_balance();
        if available < required_margin + limits.min_available_balance {
            return Err(RiskError::InsufficientMargin {
                available,
                required: required_margin,
            });
        }
        Ok(est_notional)
    }

    async fn validate_position_limit(
        &self,
        req: &OrderRequest,
        limits: &RiskLimits,
        est_notional: Decimal,
    ) -> Result<(), RiskError> {
        if req.reduce_only {
            return Ok(());
        }
        let current_notional = self.current_position_notional(req).await;
        let projected = current_notional + est_notional;
        if projected > limits.max_position_notional {
            return Err(RiskError::PositionLimitExceeded {
                projected,
                max: limits.max_position_notional,
            });
        }
        Ok(())
    }

    async fn validate_open_order_count(
        &self,
        req: &OrderRequest,
        limits: &RiskLimits,
    ) -> Result<(), RiskError> {
        let open_count = self
            .order_repo
            .find_open(&req.symbol)
            .await
            .map_err(|e| RiskError::Internal(e.to_string()))?
            .len() as u32;
        if open_count >= limits.max_open_orders_per_symbol {
            return Err(RiskError::TooManyOpenOrders(open_count));
        }
        Ok(())
    }

    fn estimate_notional(
        &self,
        req: &OrderRequest,
        limits: &RiskLimits,
    ) -> Result<Decimal, RiskError> {
        match req.order_type {
            OrderType::Market => {
                let bt = self
                    .book_ticker_cache
                    .get(&req.symbol)
                    .ok_or_else(|| RiskError::NoBookTicker(req.symbol.clone()))?;
                Ok(estimate_market_notional(req, &bt, limits.slippage_buffer_bps))
            }
            _ => {
                let price = req.price.unwrap_or(Decimal::ZERO);
                Ok(req.qty * price)
            }
        }
    }

    async fn current_position_notional(&self, req: &OrderRequest) -> Decimal {
        match self
            .position_repo
            .get(req.strategy_instance_id, &req.symbol)
            .await
        {
            Ok(Some(sp)) => {
                let pos = &sp.position;
                pos.qty.abs() * pos.avg_entry_price
            }
            _ => Decimal::ZERO,
        }
    }
}

/// 레버리지 기반 필요 증거금을 계산한다.
fn calculate_required_margin(notional: Decimal, leverage: u32) -> Decimal {
    if leverage > 0 {
        notional / Decimal::from(leverage)
    } else {
        notional
    }
}

/// 시장가 주문의 추정 notional에 슬리피지 버퍼를 적용한다.
fn estimate_market_notional(req: &OrderRequest, bt: &BookTicker, buffer_bps: u32) -> Decimal {
    let base = match req.side {
        OrderSide::Buy => bt.ask_price,
        OrderSide::Sell => bt.bid_price,
    };
    let bump = Decimal::from(10_000i64 + buffer_bps as i64) / Decimal::from(10_000i64);
    req.qty * base * bump
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn book_ticker(ask: Decimal, bid: Decimal) -> BookTicker {
        BookTicker {
            symbol: "BTCUSDT".into(),
            bid_price: bid,
            bid_quantity: dec!(10),
            ask_price: ask,
            ask_quantity: dec!(10),
            event_time_ns: 0,
        }
    }

    fn market_req(side: OrderSide, qty: Decimal) -> OrderRequest {
        OrderRequest {
            symbol: "BTCUSDT".into(),
            order_type: crate::domain::order::OrderType::Market,
            side,
            qty,
            ..Default::default()
        }
    }

    #[test]
    fn buy_market_notional_applies_100bps_slippage_buffer() {
        // ask=100, qty=10, 100bps → 10 * 100 * 1.01 = 1010
        let bt = book_ticker(dec!(100), dec!(99));
        let req = market_req(OrderSide::Buy, dec!(10));
        let notional = estimate_market_notional(&req, &bt, 100);
        assert_eq!(notional, dec!(1010));
    }

    #[test]
    fn sell_market_notional_uses_bid_with_buffer() {
        // bid=99, qty=10, 100bps → 10 * 99 * 1.01 = 999.9
        let bt = book_ticker(dec!(100), dec!(99));
        let req = market_req(OrderSide::Sell, dec!(10));
        let notional = estimate_market_notional(&req, &bt, 100);
        assert_eq!(notional, dec!(999.9));
    }

    #[test]
    fn zero_buffer_gives_raw_notional() {
        let bt = book_ticker(dec!(100), dec!(100));
        let req = market_req(OrderSide::Buy, dec!(5));
        let notional = estimate_market_notional(&req, &bt, 0);
        assert_eq!(notional, dec!(500));
    }
}
