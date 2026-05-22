use crate::application::order_execution_event::OrderExecutionEvent;
use crate::domain::order::Fill;
use crate::ports::fill_sink::FillSink;
use crate::ports::order_repository::OrderRepository;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub struct AccountEventHandler {
    repo: Arc<dyn OrderRepository>,
    fill_sink: Arc<dyn FillSink>,
}

impl AccountEventHandler {
    pub fn new(repo: Arc<dyn OrderRepository>, fill_sink: Arc<dyn FillSink>) -> Self {
        Self { repo, fill_sink }
    }

    pub async fn run(&self, mut rx: Receiver<OrderExecutionEvent>, token: CancellationToken) {
        loop {
            tokio::select! {
                _ = token.cancelled() => return,
                Some(event) = rx.recv() => self.handle(event).await,
            }
        }
    }

    async fn handle(&self, event: OrderExecutionEvent) {
        let order = match self.repo.find_by_exchange_id(event.exchange_order_id).await {
            Ok(Some(o)) => o,
            Ok(None) => {
                warn!(
                    order_id = event.exchange_order_id,
                    "ORDER_TRADE_UPDATE DB에 없음"
                );
                return;
            }
            Err(e) => {
                error!(error = %e, "ORDER_TRADE_UPDATE DB 조회 실패");
                return;
            }
        };

        if let Some(fd) = event.fill {
            let fill = Fill {
                order_id: order.id,
                strategy_instance_id: order.strategy_instance_id,
                symbol: order.symbol.clone(),
                side: fd.side,
                qty: fd.qty,
                price: fd.price,
                fee: fd.fee,
                fee_asset: fd.fee_asset,
                filled_at: fd.filled_at,
            };

            if let Err(e) = self.repo.record_fill(&fill).await {
                error!(error = %e, "Fill PG 저장 실패");
            }
            self.fill_sink.emit(fill).await;
            info!(symbol = %order.symbol, side = ?fd.side, qty = %fd.qty, price = %fd.price, "Fill 수신");
        }

        if let Some(status) = event.new_status {
            let mut updated = order;
            updated.status = status;
            updated.filled_qty = event.cumulative_filled_qty;
            updated.avg_fill_price = event.avg_fill_price;
            updated.updated_at = Utc::now();
            match self.repo.upsert_order(&updated).await {
                Ok(()) => info!(
                    order_id = %updated.id,
                    symbol = %updated.symbol,
                    new_status = ?status,
                    filled_qty = %updated.filled_qty,
                    "주문 상태 갱신"
                ),
                Err(e) => error!(error = %e, "ORDER_TRADE_UPDATE 주문 갱신 실패"),
            }
        }
    }
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
