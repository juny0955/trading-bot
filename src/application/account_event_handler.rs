use crate::application::notification_formatter::{format_fill, format_order_status};
use crate::application::order_execution_event::{FillData, OrderExecutionEvent};
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

/// 거래소 계정 이벤트를 주문 저장소와 체결 sink에 반영합니다.
///
/// # Examples
///
/// ```ignore
/// # use std::sync::Arc;
/// # use trading_bot::application::account_event_handler::AccountEventHandler;
/// # use trading_bot::ports::fill_sink::FillSink;
/// # use trading_bot::ports::order_repository::OrderRepository;
/// # fn example(repo: Arc<dyn OrderRepository>, fill_sink: Arc<dyn FillSink>) {
/// let handler = AccountEventHandler::new(repo, fill_sink);
/// # }
/// ```
pub struct AccountEventHandler {
    repo: Arc<dyn OrderRepository>,
    position_repo: Arc<dyn PositionRepository>,
    fill_sink: Arc<dyn FillSink>,
    notifier: Arc<dyn NotificationSink>,
}

impl AccountEventHandler {
    /// 새 계정 이벤트 핸들러를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use std::sync::Arc;
    /// # use trading_bot::application::account_event_handler::AccountEventHandler;
    /// # use trading_bot::ports::fill_sink::FillSink;
    /// # use trading_bot::ports::order_repository::OrderRepository;
    /// # fn example(repo: Arc<dyn OrderRepository>, position_repo: Arc<dyn PositionRepository>, fill_sink: Arc<dyn FillSink>, notifier: Arc<dyn NotificationSink>) {
    /// let handler = AccountEventHandler::new(repo, position_repo, fill_sink, notifier);
    /// # }
    /// ```
    pub fn new(
        repo: Arc<dyn OrderRepository>,
        position_repo: Arc<dyn PositionRepository>,
        fill_sink: Arc<dyn FillSink>,
        notifier: Arc<dyn NotificationSink>,
    ) -> Self {
        Self {
            repo,
            position_repo,
            fill_sink,
            notifier,
        }
    }

    /// 이벤트 채널을 소비하며 주문 상태와 체결 내역을 반영합니다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # async fn example(handler: trading_bot::application::account_event_handler::AccountEventHandler) {
    /// use tokio::sync::mpsc;
    /// use tokio_util::sync::CancellationToken;
    ///
    /// let (_tx, rx) = mpsc::channel(16);
    /// handler.run(rx, CancellationToken::new()).await;
    /// # }
    /// ```
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
        if let Some(fd) = event.fill.as_ref() {
            let fill = fill_from_data(order, fd);

            if let Err(e) = self.repo.record_fill(&fill).await {
                error!(error = %e, "Fill PG 저장 실패");
            }
            if let Err(e) = self
                .position_repo
                .apply_fill(order.strategy_instance_id, &fill)
                .await
            {
                error!(error = %e, symbol = %order.symbol, "포지션 업데이트 실패");
            }
            self.fill_sink.emit(fill.clone()).await;
            self.notifier.send(format_fill(&fill)).await;
            info!(symbol = %order.symbol, side = ?fd.side, qty = %fd.qty, price = %fd.price, "Fill 수신");
        }
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
                    // 취소·거부·만료 시 알림 전송
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

fn fill_from_data(order: &Order, fd: &FillData) -> Fill {
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
