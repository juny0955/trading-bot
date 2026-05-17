use crate::backtest::portfolio::{ClosedTrade, Portfolio};
use crate::backtest::types::Position;
use crate::order::executor::OrderExecutor;
use crate::order::types::{
    Fill, Order, OrderError, OrderRequest, OrderSide, OrderStatus, OrderType,
};
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
            }),
        }
    }

    pub fn drain_pending(&self, buy_price: Decimal, sell_price: Decimal, ts_ns: i64) -> Vec<Fill> {
        let mut st = self.state.lock().unwrap();
        let mut result = Vec::new();
        let pending: Vec<Uuid> = st.pending.drain(..).collect();

        for id in pending {
            let Some(order) = st.orders.get(&id).cloned() else {
                continue;
            };

            let price = match order.side {
                OrderSide::Buy => buy_price,
                OrderSide::Sell => sell_price,
            };

            let fills = st.portfolio.execute_simulated(&order, price, ts_ns);
            for f in &fills {
                st.fills_log.push(f.clone());
            }
            result.extend(fills);

            if let Some(o) = st.orders.get_mut(&id) {
                o.status = OrderStatus::Filled;
                o.filled_qty = o.qty;
                o.avg_fill_price = Some(price);
                o.updated_at = Utc::now();
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
            return Err(OrderError::ExchangeRejected {
                code: -1,
                msg: format!("백테스트 v1 Market Only 입력: {:?}", request.order_type),
            });
        }
        let order = Order::new_from_request(request);
        let mut st = self.state.lock().unwrap();
        st.orders.insert(order.id, order.clone());
        st.pending.push_back(order.id);
        Ok(order)
    }

    async fn cancel(&self, order_id: Uuid) -> Result<Order, OrderError> {
        let mut st = self.state.lock().unwrap();
        let Some(o) = st.orders.get_mut(&order_id) else {
            return Err(OrderError::NotFound(order_id.to_string()));
        };

        if o.status != OrderStatus::New {
            return Err(OrderError::ExchangeRejected {
                code: -1,
                msg: format!("취소할 수 없는 주문 상태 {:?}", o.status),
            });
        };
        o.status = OrderStatus::Cancelled;
        o.updated_at = Utc::now();
        let cancelled = o.clone();
        st.pending.retain(|id| *id != order_id);
        Ok(cancelled)
    }

    async fn query(&self, order_id: Uuid) -> Result<Order, OrderError> {
        let st = self.state.lock().unwrap();
        st.orders
            .get(&order_id)
            .cloned()
            .ok_or(OrderError::NotFound(order_id.to_string()))
    }

    async fn open_orders(&self, symbol: &str) -> Result<Vec<Order>, OrderError> {
        let st = self.state.lock().unwrap();
        Ok(st
            .orders
            .values()
            .filter(|o| o.symbol == symbol && o.status == OrderStatus::New)
            .cloned()
            .collect())
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
        assert_eq!(ex.query(o.id).await.unwrap().status, OrderStatus::New);
        let fills = ex.drain_pending(dec!(100), dec!(99), 1_000_000);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].price, dec!(100));
        assert_eq!(ex.query(o.id).await.unwrap().status, OrderStatus::Filled);
    }

    #[tokio::test]
    async fn cancel_removes_from_pending() {
        let ex = BacktestOrderExecutor::new(dec!(10000), dec!(0));
        let o = ex.submit(req(OrderSide::Buy, dec!(1))).await.unwrap();
        ex.cancel(o.id).await.unwrap();
        let fills = ex.drain_pending(dec!(100), dec!(99), 1);
        assert!(fills.is_empty());
        assert_eq!(ex.query(o.id).await.unwrap().status, OrderStatus::Cancelled);
    }
}
