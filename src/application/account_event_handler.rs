use crate::application::notification_formatter::{
    format_fill, format_fill_with_action, format_order_status,
};
use crate::application::order_execution_event::{FillData, OrderExecutionEvent};
use crate::application::position_lifecycle_tracker::PositionLifecycleTracker;
use crate::domain::order::{Fill, Order, OrderStatus};
use crate::ports::fill_sink::FillSink;
use crate::ports::order_repository::{OrderRepository, OrphanOrder};
use crate::ports::outbound::notification_sink::NotificationSink;
use crate::ports::position_repository::PositionRepository;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

pub struct AccountEventHandler {
    repo: Arc<dyn OrderRepository>,
    position_repo: Arc<dyn PositionRepository>,
    fill_sink: Arc<dyn FillSink>,
    notifier: Arc<dyn NotificationSink>,
    lifecycle_tracker: Arc<PositionLifecycleTracker>,
}

impl AccountEventHandler {
    pub fn new(
        repo: Arc<dyn OrderRepository>,
        position_repo: Arc<dyn PositionRepository>,
        fill_sink: Arc<dyn FillSink>,
        notifier: Arc<dyn NotificationSink>,
        lifecycle_tracker: Arc<PositionLifecycleTracker>,
    ) -> Self {
        Self {
            repo,
            position_repo,
            fill_sink,
            notifier,
            lifecycle_tracker,
        }
    }

    pub async fn run(&self, mut rx: Receiver<OrderExecutionEvent>, token: CancellationToken) {
        loop {
            tokio::select! {
                _ = token.cancelled() => return,
                event = rx.recv() => match event {
                    Some(e) => self.handle(e).await,
                    None => return,
                },
            }
        }
    }

    async fn handle(&self, event: OrderExecutionEvent) {
        let Some(order) = self.find_order(event.exchange_order_id).await else {
            let orphan = OrphanOrder {
                exchange_order_id: event.exchange_order_id,
                client_order_id: event.client_order_id,
                symbol: event.symbol,
                raw: event.raw_event,
            };
            if let Err(e) = self.repo.record_orphan(&orphan).await {
                error!(error = %e, "orphan 주문 저장 실패");
            }
            return;
        };

        self.record_fill_if_present(&order, &event).await;
        self.update_status_if_present(order, &event).await;
    }

    async fn find_order(&self, exchange_order_id: i64) -> Option<Order> {
        match self.repo.find_by_exchange_id(exchange_order_id).await {
            Ok(Some(o)) => Some(o),
            Ok(None) => {
                warn!(order_id = exchange_order_id, "ORDER_TRADE_UPDATE DB에 없음");
                None
            }
            Err(e) => {
                error!(error = %e, "ORDER_TRADE_UPDATE DB 조회 실패");
                None
            }
        }
    }

    async fn record_fill_if_present(&self, order: &Order, event: &OrderExecutionEvent) {
        let Some(fd) = event.fill.as_ref() else {
            return;
        };

        let lifecycle_id = self.lifecycle_tracker.resolve_lifecycle(order.id).await;
        let fill = fill_from_data(order, fd, lifecycle_id);

        if lifecycle_id.is_some() {
            if let Err(e) = self.repo.record_fill(&fill).await {
                error!(error = %e, "Fill PG 저장 실패");
            }
        } else {
            warn!(
                order_id = %order.id,
                exchange_trade_id = fd.exchange_trade_id,
                "lifecycle 매핑 실패 — unresolved_fills에 격리"
            );
            let trade_id = if fd.exchange_trade_id >= 0 {
                Some(fd.exchange_trade_id)
            } else {
                None
            };
            if let Err(e) = self
                .lifecycle_tracker
                .quarantine_fill(trade_id, event.raw_event.clone(), "no lifecycle mapping")
                .await
            {
                error!(error = %e, "unresolved_fill 저장 실패");
            }
        }

        let msg = match self
            .position_repo
            .apply_fill(order.strategy_instance_id, &fill)
            .await
        {
            Ok((_, action)) => format_fill_with_action(&fill, &action),
            Err(e) => {
                error!(error = %e, symbol = %order.symbol, "포지션 업데이트 실패");
                format_fill(&fill)
            }
        };
        self.fill_sink.emit(fill.clone()).await;
        self.notifier.send(msg).await;
        info!(
            symbol = %order.symbol,
            side = ?fd.side,
            qty = %fd.qty,
            price = %fd.price,
            lifecycle_id = ?lifecycle_id,
            "Fill 수신"
        );
    }

    async fn update_status_if_present(&self, order: Order, event: &OrderExecutionEvent) {
        if let Some(status) = event.new_status {
            let updated = apply_order_status(order, event, status);
            match self.repo.upsert_order(&updated).await {
                Ok(()) => {
                    info!(
                        order_id = %updated.id,
                        symbol = %updated.symbol,
                        new_status = ?status,
                        filled_qty = %updated.filled_qty,
                        "주문 상태 갱신"
                    );
                    if matches!(
                        status,
                        OrderStatus::Cancelled | OrderStatus::Rejected | OrderStatus::Expired
                    ) {
                        self.notifier.send(format_order_status(&updated)).await;
                    }
                }
                Err(e) => error!(error = %e, "ORDER_TRADE_UPDATE 주문 갱신 실패"),
            }
        }
    }
}

fn fill_from_data(order: &Order, fd: &FillData, position_lifecycle_id: Option<Uuid>) -> Fill {
    Fill {
        order_id: order.id,
        strategy_instance_id: order.strategy_instance_id,
        symbol: order.symbol.clone(),
        side: fd.side,
        qty: fd.qty,
        price: fd.price,
        fee: fd.fee,
        fee_asset: fd.fee_asset.clone(),
        filled_at: fd.filled_at,
        exchange_trade_id: fd.exchange_trade_id,
        position_lifecycle_id,
    }
}

fn apply_order_status(mut order: Order, event: &OrderExecutionEvent, status: OrderStatus) -> Order {
    order.status = status;
    order.filled_qty = event.cumulative_filled_qty;
    order.avg_fill_price = event.avg_fill_price;
    order.updated_at = Utc::now();
    order
}

#[async_trait::async_trait]
impl FillSink for Sender<Fill> {
    async fn emit(&self, fill: Fill) {
        let symbol = fill.symbol.clone();
        if self.send(fill).await.is_err() {
            warn!(symbol = %symbol, "Fill 채널 닫힘 — 체결 데이터 손실");
        }
    }
}
