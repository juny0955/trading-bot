/// submit → fill 이벤트 → position 반영 라이프사이클 통합 테스트.
///
/// 실제 DB 없이 in-memory fake를 사용한다.
use async_trait::async_trait;
use chrono::Utc;
use rust_decimal_macros::dec;
use std::sync::{Arc, Mutex};
use trading_bot::application::account_event_handler::AccountEventHandler;
use trading_bot::application::order_execution_event::{FillData, OrderExecutionEvent};
use trading_bot::domain::account::ExchangePositionSnapshot;
use trading_bot::domain::order::{Fill, Order, OrderRequest, OrderSide, OrderStatus, OrderType};
use trading_bot::domain::position::StoredPosition;
use trading_bot::ports::fill_sink::FillSink;
use trading_bot::ports::outbound::order_repository::{
    OrderRepository, OrphanOrder, RepositoryError,
};
use trading_bot::ports::outbound::position_repository::PositionRepository;
use uuid::Uuid;

// ──────────────────────────────────────────────
// In-memory fakes
// ──────────────────────────────────────────────

#[derive(Default)]
struct FakeOrderRepo {
    orders: Mutex<Vec<Order>>,
    fills: Mutex<Vec<Fill>>,
    orphans: Mutex<Vec<OrphanOrder>>,
}

#[async_trait]
impl OrderRepository for FakeOrderRepo {
    async fn upsert_order(&self, order: &Order) -> Result<(), RepositoryError> {
        let mut orders = self.orders.lock().unwrap();
        if let Some(existing) = orders.iter_mut().find(|o| o.id == order.id) {
            *existing = order.clone();
        } else {
            orders.push(order.clone());
        }
        Ok(())
    }

    async fn record_fill(&self, fill: &Fill) -> Result<(), RepositoryError> {
        self.fills.lock().unwrap().push(fill.clone());
        Ok(())
    }

    async fn find_open(&self, symbol: &str) -> Result<Vec<Order>, RepositoryError> {
        Ok(self
            .orders
            .lock()
            .unwrap()
            .iter()
            .filter(|o| o.symbol == symbol && !o.status.is_terminal())
            .cloned()
            .collect())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, RepositoryError> {
        Ok(self
            .orders
            .lock()
            .unwrap()
            .iter()
            .find(|o| o.id == id)
            .cloned())
    }

    async fn find_by_exchange_id(
        &self,
        exchange_id: i64,
    ) -> Result<Option<Order>, RepositoryError> {
        Ok(self
            .orders
            .lock()
            .unwrap()
            .iter()
            .find(|o| o.exchange_order_id == Some(exchange_id))
            .cloned())
    }

    async fn find_by_client_order_id(
        &self,
        client_order_id: &str,
    ) -> Result<Option<Order>, RepositoryError> {
        Ok(self
            .orders
            .lock()
            .unwrap()
            .iter()
            .find(|o| o.client_order_id == client_order_id)
            .cloned())
    }

    async fn find_by_status(&self, status: OrderStatus) -> Result<Vec<Order>, RepositoryError> {
        Ok(self
            .orders
            .lock()
            .unwrap()
            .iter()
            .filter(|o| o.status == status)
            .cloned()
            .collect())
    }

    async fn record_orphan(&self, orphan: &OrphanOrder) -> Result<(), RepositoryError> {
        self.orphans.lock().unwrap().push(orphan.clone());
        Ok(())
    }

    async fn find_orphans(&self, limit: i64) -> Result<Vec<OrphanOrder>, RepositoryError> {
        Ok(self
            .orphans
            .lock()
            .unwrap()
            .iter()
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn delete_orphan(&self, exchange_order_id: i64) -> Result<(), RepositoryError> {
        self.orphans
            .lock()
            .unwrap()
            .retain(|o| o.exchange_order_id != exchange_order_id);
        Ok(())
    }
}

#[derive(Default)]
struct FakePositionRepo {
    positions: Mutex<Vec<StoredPosition>>,
}

#[async_trait]
impl PositionRepository for FakePositionRepo {
    async fn get(
        &self,
        instance_id: i64,
        symbol: &str,
    ) -> Result<Option<StoredPosition>, RepositoryError> {
        Ok(self
            .positions
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.strategy_instance_id == instance_id && p.symbol == symbol)
            .cloned())
    }

    async fn apply_fill(
        &self,
        instance_id: i64,
        fill: &Fill,
    ) -> Result<StoredPosition, RepositoryError> {
        let mut positions = self.positions.lock().unwrap();
        let stored = positions
            .iter_mut()
            .find(|p| p.strategy_instance_id == instance_id && p.symbol == fill.symbol);

        let updated = match stored {
            Some(s) => {
                s.position = s
                    .position
                    .apply(fill)
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?;
                s.updated_at = Utc::now();
                s.clone()
            }
            None => {
                let new_pos = trading_bot::domain::position::Position::default()
                    .apply(fill)
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?;
                let sp = StoredPosition {
                    strategy_instance_id: instance_id,
                    symbol: fill.symbol.clone(),
                    position: new_pos,
                    last_fill_id: None,
                    updated_at: Utc::now(),
                };
                positions.push(sp.clone());
                sp
            }
        };
        Ok(updated)
    }

    async fn upsert_from_snapshot(
        &self,
        instance_id: i64,
        snap: &ExchangePositionSnapshot,
    ) -> Result<(), RepositoryError> {
        let sp = StoredPosition::flat(instance_id, snap.symbol.clone());
        let mut positions = self.positions.lock().unwrap();
        if let Some(existing) = positions
            .iter_mut()
            .find(|p| p.strategy_instance_id == instance_id && p.symbol == snap.symbol)
        {
            existing.position = sp.position;
        } else {
            positions.push(sp);
        }
        Ok(())
    }
}

#[derive(Default)]
struct CaptureFillSink {
    fills: Mutex<Vec<Fill>>,
}

#[async_trait]
impl FillSink for CaptureFillSink {
    async fn emit(&self, fill: Fill) {
        self.fills.lock().unwrap().push(fill);
    }
}

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

fn new_order() -> Order {
    let req = OrderRequest {
        symbol: "BTCUSDT".into(),
        order_type: OrderType::Market,
        side: OrderSide::Buy,
        qty: dec!(1),
        strategy_instance_id: 1,
        ..Default::default()
    };
    let mut order = Order::new_from_request(req).unwrap();
    order.status = OrderStatus::New;
    order.exchange_order_id = Some(42);
    order
}

fn fill_event(exchange_order_id: i64) -> OrderExecutionEvent {
    OrderExecutionEvent {
        exchange_order_id,
        fill: Some(FillData {
            side: OrderSide::Buy,
            qty: dec!(1),
            price: dec!(50000),
            fee: dec!(0.5),
            fee_asset: "USDT".into(),
            filled_at: Utc::now(),
            exchange_trade_id: 999,
        }),
        new_status: Some(OrderStatus::Filled),
        cumulative_filled_qty: dec!(1),
        avg_fill_price: Some(dec!(50000)),
        client_order_id: Some("test-cid-1".into()),
        symbol: Some("BTCUSDT".into()),
        raw_event: serde_json::json!({"e": "ORDER_TRADE_UPDATE"}),
    }
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[tokio::test]
async fn fill_event_records_fill_and_updates_position() {
    let repo = Arc::new(FakeOrderRepo::default());
    let position_repo = Arc::new(FakePositionRepo::default());
    let fill_sink = Arc::new(CaptureFillSink::default());

    // 사전에 New 상태의 주문을 DB에 저장
    let order = new_order();
    repo.upsert_order(&order).await.unwrap();

    let handler = AccountEventHandler::new(repo.clone(), position_repo.clone(), fill_sink.clone());

    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let token = tokio_util::sync::CancellationToken::new();

    tx.send(fill_event(42)).await.unwrap();
    drop(tx); // 채널을 닫아 run()이 None 수신 후 종료되도록 한다

    handler.run(rx, token).await;

    // Fill이 저장됐는지 확인
    {
        let fills = repo.fills.lock().unwrap();
        assert_eq!(fills.len(), 1, "fill이 1개 기록돼야 한다");
        assert_eq!(fills[0].qty, dec!(1));
        assert_eq!(fills[0].price, dec!(50000));
        assert_eq!(fills[0].exchange_trade_id, 999);
    }

    // FillSink에 체결이 전달됐는지 확인
    {
        let sink_fills = fill_sink.fills.lock().unwrap();
        assert_eq!(sink_fills.len(), 1, "FillSink에 1개 체결이 전달돼야 한다");
    }

    // 주문 상태가 Filled로 갱신됐는지 확인
    {
        let orders = repo.orders.lock().unwrap();
        let updated = orders
            .iter()
            .find(|o| o.exchange_order_id == Some(42))
            .unwrap();
        assert_eq!(updated.status, OrderStatus::Filled);
    }

    // 포지션이 반영됐는지 확인
    let pos = position_repo
        .get(1, "BTCUSDT")
        .await
        .unwrap()
        .expect("포지션이 존재해야 한다");
    assert_eq!(pos.position.qty, dec!(1));
    assert!(pos.position.side.is_some(), "포지션 방향이 설정돼야 한다");
}

#[tokio::test]
async fn unknown_exchange_order_recorded_as_orphan() {
    let repo = Arc::new(FakeOrderRepo::default());
    let position_repo = Arc::new(FakePositionRepo::default());
    let fill_sink = Arc::new(CaptureFillSink::default());

    // DB에 주문을 저장하지 않음 — exchange_order_id=99는 orphan이 된다
    let handler = AccountEventHandler::new(repo.clone(), position_repo.clone(), fill_sink.clone());

    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let token = tokio_util::sync::CancellationToken::new();

    tx.send(fill_event(99)).await.unwrap();
    drop(tx);

    handler.run(rx, token).await;

    let orphans = repo.orphans.lock().unwrap();
    assert_eq!(orphans.len(), 1, "orphan이 1개 기록돼야 한다");
    assert_eq!(orphans[0].exchange_order_id, 99);
}
