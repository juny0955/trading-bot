use crate::application::backtest::portfolio::{ClosedTrade, Portfolio};
use crate::domain::backtest::Position;
use crate::domain::order::{
    ExchangeOpenOrder, Fill, Order, OrderError, OrderRequest, OrderSide, OrderStatus, OrderType,
};
use crate::ports::order_executor::OrderExecutor;
use async_trait::async_trait;
use chrono::Utc;
use rust_decimal::Decimal;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use uuid::Uuid;

pub struct BacktestOrderExecutor {
    state: Mutex<BacktestState>,
}

struct BacktestState {
    portfolio: Portfolio,
    orders: HashMap<Uuid, Order>,
    pending: VecDeque<Uuid>,
    fills_log: Vec<Fill>,
    next_exchange_order_id: i64,
}

#[derive(Clone)]
pub struct PortfolioSnapshot {
    pub equity: Decimal,
    pub initial_equity: Decimal,
    pub position: Position,
    pub closed_trades: Vec<ClosedTrade>,
}

impl BacktestOrderExecutor {
    pub fn new(initial_equity: Decimal, fee_bps: Decimal) -> Self {
        Self {
            state: Mutex::new(BacktestState {
                portfolio: Portfolio::new(initial_equity, fee_bps),
                orders: HashMap::new(),
                pending: VecDeque::new(),
                fills_log: Vec::new(),
                next_exchange_order_id: 0,
            }),
        }
    }

    pub fn drain_pending(&self, buy_price: Decimal, sell_price: Decimal, ts_ns: i64) -> Vec<Fill> {
        let mut st_guard = self.state.lock().expect("Mutex poisoned");

        let BacktestState {
            portfolio,
            orders,
            pending,
            fills_log,
            next_exchange_order_id: _,
        } = &mut *st_guard;

        if pending.is_empty() {
            return Vec::new();
        }
        let mut result = Vec::with_capacity(pending.len());

        while let Some(id) = pending.pop_front() {
            if let Some(order) = orders.get_mut(&id) {
                let price = match order.side {
                    OrderSide::Buy => buy_price,
                    OrderSide::Sell => sell_price,
                };

                let fills = portfolio.execute_simulated(order, price, ts_ns);

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
        result
    }

    pub fn mark(&self, mark_price: Decimal) {
        let mut st = self.state.lock().unwrap();
        st.portfolio.mark(mark_price);
    }

    pub fn snapshot(&self) -> PortfolioSnapshot {
        let st = self.state.lock().unwrap();
        PortfolioSnapshot {
            equity: st.portfolio.equity,
            initial_equity: st.portfolio.initial_equity,
            position: st.portfolio.position.clone(),
            closed_trades: st.portfolio.closed_trades.clone(),
        }
    }

    pub fn equity(&self) -> Decimal {
        self.state.lock().unwrap().portfolio.equity
    }

    pub fn position(&self) -> Position {
        self.state.lock().unwrap().portfolio.position.clone()
    }
}

#[async_trait]
impl OrderExecutor for BacktestOrderExecutor {
    async fn submit(&self, request: OrderRequest) -> Result<Order, OrderError> {
        if request.order_type != OrderType::Market {
            return Err(OrderError::InvalidRequest(format!(
                "백테스트 v1 Market Only 입력: {:?}",
                request.order_type
            )));
        }
        let mut order = Order::new_from_request(request);
        let mut st = self.state.lock().unwrap();
        st.next_exchange_order_id += 1;
        order.exchange_order_id = Some(st.next_exchange_order_id);
        st.orders.insert(order.id, order.clone());
        st.pending.push_back(order.id);
        Ok(order)
    }

    async fn cancel(&self, order: &Order) -> Result<Order, OrderError> {
        let mut st = self.state.lock().unwrap();
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
        Ok(cancelled)
    }

    async fn query(&self, order: &Order) -> Result<Order, OrderError> {
        let st = self.state.lock().unwrap();
        st.orders
            .get(&order.id)
            .cloned()
            .ok_or(OrderError::NotFound(order.id.to_string()))
    }

    async fn open_orders(&self, symbol: &str) -> Result<Vec<ExchangeOpenOrder>, OrderError> {
        let st = self.state.lock().unwrap();
        st.orders
            .values()
            .filter(|o| o.symbol == symbol && o.status == OrderStatus::New)
            .map(ExchangeOpenOrder::try_from)
            .collect::<Result<Vec<_>, _>>()
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
    async fn submit_then_drain_fills_at_next_tick() {
        let ex = BacktestOrderExecutor::new(dec!(10000), dec!(0));
        let o = ex.submit(req(OrderSide::Buy, dec!(1))).await.unwrap();
        assert_eq!(ex.query(&o).await.unwrap().status, OrderStatus::New);
        let fills = ex.drain_pending(dec!(100), dec!(99), 1_000_000);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].price, dec!(100));
        assert_eq!(ex.query(&o).await.unwrap().status, OrderStatus::Filled);
    }

    #[tokio::test]
    async fn cancel_removes_from_pending() {
        let ex = BacktestOrderExecutor::new(dec!(10000), dec!(0));
        let o = ex.submit(req(OrderSide::Buy, dec!(1))).await.unwrap();
        ex.cancel(&o).await.unwrap();
        let fills = ex.drain_pending(dec!(100), dec!(99), 1);
        assert!(fills.is_empty());
        assert_eq!(ex.query(&o).await.unwrap().status, OrderStatus::Cancelled);
    }
}
