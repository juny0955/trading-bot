use crate::application::backtest::portfolio::{ClosedTrade, Portfolio, PortfolioError};
use crate::domain::order::{
    ExchangeOpenOrder, Fill, Order, OrderError, OrderRequest, OrderSide, OrderStatus, OrderType,
};
use crate::domain::position::Position;
use crate::ports::order_executor::OrderExecutor;
use async_trait::async_trait;
use chrono::Utc;
use rust_decimal::Decimal;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

/// 백테스트 주문 실행기에서 발생하는 애플리케이션 오류입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::application::backtest::backtest_executor::BacktestExecutorError;
///
/// let err = BacktestExecutorError::StatePoisoned;
/// assert!(err.to_string().contains("state"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum BacktestExecutorError {
    #[error("backtest state lock poisoned")]
    StatePoisoned,
    #[error(transparent)]
    Portfolio(#[from] PortfolioError),
}

impl From<BacktestExecutorError> for OrderError {
    fn from(value: BacktestExecutorError) -> Self {
        OrderError::Internal(anyhow::anyhow!(value))
    }
}

/// 백테스트용 인메모리 주문 실행기입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::application::backtest::backtest_executor::BacktestOrderExecutor;
///
/// let executor = BacktestOrderExecutor::new(dec!(10_000), dec!(4)).unwrap();
/// assert_eq!(executor.equity().unwrap(), dec!(10_000));
/// ```
pub struct BacktestOrderExecutor {
    state: Mutex<BacktestState>,
}

struct BacktestState {
    portfolio: Portfolio,
    orders: HashMap<Uuid, Order>,
    pending: VecDeque<Uuid>,
    /// 가격 트리거 대기 중인 StopMarket 주문 (order_id, stop_price)
    stop_orders: VecDeque<Uuid>,
    /// 가격 트리거 대기 중인 TakeProfit 주문 (order_id, stop_price)
    take_profit_orders: VecDeque<Uuid>,
    fills_log: Vec<Fill>,
    next_exchange_order_id: i64,
}

/// 포트폴리오 상태의 읽기 전용 스냅샷입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::application::backtest::backtest_executor::BacktestOrderExecutor;
///
/// let executor = BacktestOrderExecutor::new(dec!(10_000), dec!(0)).unwrap();
/// let snapshot = executor.snapshot().unwrap();
/// assert_eq!(snapshot.equity, dec!(10_000));
/// ```
#[derive(Clone)]
pub struct PortfolioSnapshot {
    pub equity: Decimal,
    pub initial_equity: Decimal,
    pub position: Position,
    pub closed_trades: Vec<ClosedTrade>,
}

impl BacktestOrderExecutor {
    /// 백테스트 주문 실행기를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::application::backtest::backtest_executor::BacktestOrderExecutor;
    ///
    /// assert!(BacktestOrderExecutor::new(dec!(1000), dec!(1)).is_ok());
    /// ```
    pub fn new(initial_equity: Decimal, fee_bps: Decimal) -> Result<Self, BacktestExecutorError> {
        Ok(Self {
            state: Mutex::new(BacktestState {
                portfolio: Portfolio::new(initial_equity, fee_bps)?,
                orders: HashMap::new(),
                pending: VecDeque::new(),
                stop_orders: VecDeque::new(),
                take_profit_orders: VecDeque::new(),
                fills_log: Vec::new(),
                next_exchange_order_id: 0,
            }),
        })
    }

    /// 대기 중인 시장가 주문을 지정한 매수/매도 가격으로 체결합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::application::backtest::backtest_executor::BacktestOrderExecutor;
    ///
    /// let executor = BacktestOrderExecutor::new(dec!(10_000), dec!(0)).unwrap();
    /// let fills = executor.drain_pending(dec!(100), dec!(99), 1).unwrap();
    /// assert!(fills.is_empty());
    /// ```
    pub fn drain_pending(
        &self,
        buy_price: Decimal,
        sell_price: Decimal,
        ts_ns: i64,
    ) -> Result<Vec<Fill>, BacktestExecutorError> {
        let mut st_guard = self.state()?;

        let BacktestState {
            portfolio,
            orders,
            pending,
            fills_log,
            ..
        } = &mut *st_guard;

        if pending.is_empty() {
            return Ok(Vec::new());
        }
        let mut result = Vec::with_capacity(pending.len());

        while let Some(id) = pending.pop_front() {
            if let Some(order) = orders.get_mut(&id) {
                let price = match order.side {
                    OrderSide::Buy => buy_price,
                    OrderSide::Sell => sell_price,
                };

                let fills = portfolio.execute_simulated(order, price, ts_ns)?;

                for f in fills.iter().cloned() {
                    fills_log.push(f);
                }
                result.extend(fills);

                order.status = OrderStatus::Filled;
                order.filled_qty = order.qty;
                order.avg_fill_price = Some(price);
                order.updated_at = Utc::now();
            }
        }
        Ok(result)
    }

    /// 포트폴리오의 mark-to-market 값을 갱신합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::application::backtest::backtest_executor::BacktestOrderExecutor;
    ///
    /// let executor = BacktestOrderExecutor::new(dec!(10_000), dec!(0)).unwrap();
    /// executor.mark(dec!(100)).unwrap();
    /// ```
    pub fn mark(&self, mark_price: Decimal) -> Result<(), BacktestExecutorError> {
        let mut st = self.state()?;
        Ok(st.portfolio.mark(mark_price)?)
    }

    /// 현재 포트폴리오 스냅샷을 반환합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::application::backtest::backtest_executor::BacktestOrderExecutor;
    ///
    /// let executor = BacktestOrderExecutor::new(dec!(10_000), dec!(0)).unwrap();
    /// assert_eq!(executor.snapshot().unwrap().initial_equity, dec!(10_000));
    /// ```
    pub fn snapshot(&self) -> Result<PortfolioSnapshot, BacktestExecutorError> {
        let st = self.state()?;
        Ok(PortfolioSnapshot {
            equity: st.portfolio.equity,
            initial_equity: st.portfolio.initial_equity,
            position: st.portfolio.position,
            closed_trades: st.portfolio.closed_trades.clone(),
        })
    }

    /// 캔들 고가/저가를 기준으로 StopMarket 주문이 트리거됐는지 확인하고 체결합니다.
    ///
    /// - Sell StopMarket: low <= stop_price 일 때 체결 (손절/숏 TP)
    /// - Buy StopMarket: high >= stop_price 일 때 체결 (숏 손절/롱 TP)
    ///
    /// # Examples
    ///
    /// ```
    /// # use tokio::runtime::Runtime;
    /// use rust_decimal_macros::dec;
    /// use trading_bot::application::backtest::backtest_executor::BacktestOrderExecutor;
    /// use trading_bot::domain::order::{OrderRequest, OrderSide, OrderType};
    /// use trading_bot::ports::outbound::order_executor::OrderExecutor;
    ///
    /// Runtime::new().unwrap().block_on(async {
    ///     let ex = BacktestOrderExecutor::new(dec!(10_000), dec!(0)).unwrap();
    ///     // 먼저 롱 포지션 진입
    ///     ex.submit(OrderRequest { order_type: OrderType::Market, side: OrderSide::Buy, qty: dec!(1), symbol: "X".into(), ..Default::default() }).await.unwrap();
    ///     ex.drain_pending(dec!(100), dec!(100), 1).unwrap();
    ///     // SL 설정
    ///     ex.submit(OrderRequest { order_type: OrderType::StopMarket, side: OrderSide::Sell, qty: dec!(1), stop_price: Some(dec!(95)), reduce_only: true, symbol: "X".into(), ..Default::default() }).await.unwrap();
    ///     // 저가가 SL 미달
    ///     let fills = ex.drain_stops(dec!(98), dec!(102), 2).unwrap();
    ///     assert!(fills.is_empty());
    ///     // 저가가 SL 도달 → 체결
    ///     let fills = ex.drain_stops(dec!(90), dec!(99), 3).unwrap();
    ///     assert_eq!(fills.len(), 1);
    /// });
    /// ```
    pub fn drain_stops(
        &self,
        low: Decimal,
        high: Decimal,
        ts_ns: i64,
    ) -> Result<Vec<Fill>, BacktestExecutorError> {
        let mut st_guard = self.state()?;
        let BacktestState {
            portfolio,
            orders,
            stop_orders,
            take_profit_orders,
            fills_log,
            ..
        } = &mut *st_guard;

        if stop_orders.is_empty() && take_profit_orders.is_empty() {
            return Ok(Vec::new());
        }

        let mut result = Vec::new();
        result.extend(drain_trigger_queue(
            portfolio,
            orders,
            stop_orders,
            fills_log,
            low,
            high,
            ts_ns,
            stop_triggered,
        )?);
        result.extend(drain_trigger_queue(
            portfolio,
            orders,
            take_profit_orders,
            fills_log,
            low,
            high,
            ts_ns,
            take_profit_triggered,
        )?);
        Ok(result)
    }

    /// 현재 equity를 반환합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::application::backtest::backtest_executor::BacktestOrderExecutor;
    ///
    /// let executor = BacktestOrderExecutor::new(dec!(10_000), dec!(0)).unwrap();
    /// assert_eq!(executor.equity().unwrap(), dec!(10_000));
    /// ```
    pub fn equity(&self) -> Result<Decimal, BacktestExecutorError> {
        Ok(self.state()?.portfolio.equity)
    }

    /// 현재 포지션을 반환합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use rust_decimal_macros::dec;
    /// use trading_bot::application::backtest::backtest_executor::BacktestOrderExecutor;
    ///
    /// let executor = BacktestOrderExecutor::new(dec!(10_000), dec!(0)).unwrap();
    /// assert!(executor.position().unwrap().side.is_none());
    /// ```
    pub fn position(&self) -> Result<Position, BacktestExecutorError> {
        Ok(self.state()?.portfolio.position)
    }

    /// 현재 equity와 포지션을 단 한 번의 락으로 경량 조회합니다.
    pub fn equity_and_position(&self) -> Result<(Decimal, Position), BacktestExecutorError> {
        let st = self.state()?;
        Ok((st.portfolio.equity, st.portfolio.position))
    }

    /// 주문 ID만으로 주문을 취소합니다 (백테스트 내부 SL/TP 교체 전용).
    /// 이미 체결됐거나 없는 주문은 조용히 무시합니다.
    pub fn cancel_order_id(&self, id: Uuid) -> Result<(), BacktestExecutorError> {
        let mut st = self.state()?;
        if let Some(o) = st.orders.get_mut(&id)
            && o.status == OrderStatus::New
        {
            o.status = OrderStatus::Cancelled;
            o.updated_at = Utc::now();
        }
        st.pending.retain(|oid| *oid != id);
        st.stop_orders.retain(|oid| *oid != id);
        st.take_profit_orders.retain(|oid| *oid != id);
        Ok(())
    }

    pub async fn submit_take_profit(&self, request: OrderRequest) -> Result<Order, OrderError> {
        if request.order_type != OrderType::StopMarket {
            return Err(OrderError::InvalidRequest(format!(
                "TakeProfit 백테스트 주문은 StopMarket 기반이어야 함: {:?}",
                request.order_type
            )));
        }
        if request.stop_price.is_none() {
            return Err(OrderError::InvalidRequest(
                "TakeProfit 주문에 stop_price 필수".into(),
            ));
        }
        let mut order = Order::new_from_request(request)?;
        let mut st = self.state().map_err(OrderError::from)?;
        st.next_exchange_order_id += 1;
        order.exchange_order_id = Some(st.next_exchange_order_id);
        st.orders.insert(order.id, order.clone());
        st.take_profit_orders.push_back(order.id);
        Ok(order)
    }

    fn state(&self) -> Result<MutexGuard<'_, BacktestState>, BacktestExecutorError> {
        self.state
            .lock()
            .map_err(|_| BacktestExecutorError::StatePoisoned)
    }
}

fn drain_trigger_queue(
    portfolio: &mut Portfolio,
    orders: &mut HashMap<Uuid, Order>,
    queue: &mut VecDeque<Uuid>,
    fills_log: &mut Vec<Fill>,
    low: Decimal,
    high: Decimal,
    ts_ns: i64,
    triggered: fn(OrderSide, Decimal, Decimal, Decimal) -> bool,
) -> Result<Vec<Fill>, BacktestExecutorError> {
    let mut result = Vec::new();
    let mut remaining = VecDeque::new();

    while let Some(id) = queue.pop_front() {
        let Some(order) = orders.get_mut(&id) else {
            continue;
        };
        let Some(stop_price) = order.stop_price else {
            remaining.push_back(id);
            continue;
        };

        if !triggered(order.side, stop_price, low, high) {
            remaining.push_back(id);
            continue;
        }

        let fills = portfolio.execute_simulated(order, stop_price, ts_ns)?;
        for f in fills.iter().cloned() {
            fills_log.push(f);
        }
        result.extend(fills);
        order.status = OrderStatus::Filled;
        order.filled_qty = order.qty;
        order.avg_fill_price = Some(stop_price);
        order.updated_at = Utc::now();
    }

    *queue = remaining;
    Ok(result)
}

fn stop_triggered(side: OrderSide, stop_price: Decimal, low: Decimal, high: Decimal) -> bool {
    match side {
        OrderSide::Sell => low <= stop_price,
        OrderSide::Buy => high >= stop_price,
    }
}

fn take_profit_triggered(
    side: OrderSide,
    stop_price: Decimal,
    low: Decimal,
    high: Decimal,
) -> bool {
    match side {
        OrderSide::Sell => high >= stop_price,
        OrderSide::Buy => low <= stop_price,
    }
}

#[async_trait]
impl OrderExecutor for BacktestOrderExecutor {
    async fn submit(&self, request: OrderRequest) -> Result<Order, OrderError> {
        let is_stop = request.order_type == OrderType::StopMarket;
        if !matches!(
            request.order_type,
            OrderType::Market | OrderType::StopMarket
        ) {
            return Err(OrderError::InvalidRequest(format!(
                "백테스트는 Market/StopMarket만 지원: {:?}",
                request.order_type
            )));
        }
        if is_stop && request.stop_price.is_none() {
            return Err(OrderError::InvalidRequest(
                "StopMarket 주문에 stop_price 필수".into(),
            ));
        }
        let mut order = Order::new_from_request(request)?;
        let mut st = self.state().map_err(OrderError::from)?;
        st.next_exchange_order_id += 1;
        order.exchange_order_id = Some(st.next_exchange_order_id);
        st.orders.insert(order.id, order.clone());
        if is_stop {
            st.stop_orders.push_back(order.id);
        } else {
            st.pending.push_back(order.id);
        }
        Ok(order)
    }

    async fn cancel(&self, order: &Order) -> Result<Order, OrderError> {
        let mut st = self.state().map_err(OrderError::from)?;
        let Some(o) = st.orders.get_mut(&order.id) else {
            return Err(OrderError::NotFound(order.id.to_string()));
        };

        if o.status != OrderStatus::New {
            return Err(OrderError::InvalidRequest(format!(
                "취소할 수 없는 주문 상태 {:?}",
                o.status
            )));
        };

        o.status = OrderStatus::Cancelled;
        o.updated_at = Utc::now();
        let cancelled = o.clone();
        st.pending.retain(|id| *id != order.id);
        st.stop_orders.retain(|id| *id != order.id);
        st.take_profit_orders.retain(|id| *id != order.id);
        Ok(cancelled)
    }

    async fn query(&self, order: &Order) -> Result<Order, OrderError> {
        let st = self.state().map_err(OrderError::from)?;
        st.orders
            .get(&order.id)
            .cloned()
            .ok_or(OrderError::NotFound(order.id.to_string()))
    }

    async fn open_orders(&self, symbol: &str) -> Result<Vec<ExchangeOpenOrder>, OrderError> {
        let st = self.state().map_err(OrderError::from)?;
        st.orders
            .values()
            .filter(|o| o.symbol == symbol && o.status == OrderStatus::New)
            .map(ExchangeOpenOrder::try_from)
            .collect::<Result<Vec<_>, _>>()
    }

    async fn query_by_client_id(
        &self,
        symbol: &str,
        client_order_id: &str,
    ) -> Result<Order, OrderError> {
        let st = self.state().map_err(OrderError::from)?;
        st.orders
            .values()
            .find(|o| o.symbol == symbol && o.client_order_id == client_order_id)
            .cloned()
            .ok_or_else(|| OrderError::NotFound(client_order_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn req(side: OrderSide, qty: Decimal) -> OrderRequest {
        OrderRequest {
            symbol: "BTCUSDT".into(),
            order_type: OrderType::Market,
            side,
            qty,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn submit_then_drain_fills_at_next_tick() -> anyhow::Result<()> {
        let ex = BacktestOrderExecutor::new(dec!(10000), dec!(0))?;
        let o = ex.submit(req(OrderSide::Buy, dec!(1))).await?;
        assert_eq!(ex.query(&o).await?.status, OrderStatus::New);
        let fills = ex.drain_pending(dec!(100), dec!(99), 1_000_000)?;
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].price, dec!(100));
        assert_eq!(ex.query(&o).await?.status, OrderStatus::Filled);
        Ok(())
    }

    #[tokio::test]
    async fn cancel_removes_from_pending() -> anyhow::Result<()> {
        let ex = BacktestOrderExecutor::new(dec!(10000), dec!(0))?;
        let o = ex.submit(req(OrderSide::Buy, dec!(1))).await?;
        ex.cancel(&o).await?;
        let fills = ex.drain_pending(dec!(100), dec!(99), 1)?;
        assert!(fills.is_empty());
        assert_eq!(ex.query(&o).await?.status, OrderStatus::Cancelled);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_non_market_orders() -> anyhow::Result<()> {
        let ex = BacktestOrderExecutor::new(dec!(10000), dec!(0))?;
        let mut request = req(OrderSide::Buy, dec!(1));
        request.order_type = OrderType::Limit;

        let result = ex.submit(request).await;
        assert!(matches!(result, Err(OrderError::InvalidRequest(_))));
        Ok(())
    }

    fn stop_req(side: OrderSide, qty: Decimal, stop_price: Decimal) -> OrderRequest {
        OrderRequest {
            symbol: "BTCUSDT".into(),
            order_type: OrderType::StopMarket,
            side,
            qty,
            stop_price: Some(stop_price),
            reduce_only: true,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn sl_sell_triggers_when_low_crosses_stop() -> anyhow::Result<()> {
        let ex = BacktestOrderExecutor::new(dec!(10000), dec!(0))?;
        // 롱 진입
        ex.submit(req(OrderSide::Buy, dec!(1))).await?;
        ex.drain_pending(dec!(100), dec!(100), 1)?;
        // SL 설정 (95 아래로 떨어지면 청산)
        let sl = ex
            .submit(stop_req(OrderSide::Sell, dec!(1), dec!(95)))
            .await?;

        // 저가 98 → 미트리거
        let fills = ex.drain_stops(dec!(98), dec!(102), 2)?;
        assert!(fills.is_empty());

        // 저가 90 → 트리거 (95에 체결)
        let fills = ex.drain_stops(dec!(90), dec!(99), 3)?;
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].price, dec!(95));
        assert_eq!(ex.query(&sl).await?.status, OrderStatus::Filled);
        Ok(())
    }

    #[tokio::test]
    async fn cancel_removes_stop_order() -> anyhow::Result<()> {
        let ex = BacktestOrderExecutor::new(dec!(10000), dec!(0))?;
        ex.submit(req(OrderSide::Buy, dec!(1))).await?;
        ex.drain_pending(dec!(100), dec!(100), 1)?;
        let sl = ex
            .submit(stop_req(OrderSide::Sell, dec!(1), dec!(95)))
            .await?;

        ex.cancel_order_id(sl.id)?;

        // 저가가 SL 아래로 내려가도 체결 안 됨
        let fills = ex.drain_stops(dec!(80), dec!(99), 2)?;
        assert!(fills.is_empty());
        Ok(())
    }
}
