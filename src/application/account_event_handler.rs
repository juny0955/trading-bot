use crate::application::order_execution_event::OrderExecutionEvent;
use crate::ports::fill_sink::FillSink;
use crate::ports::order_repository::OrderRepository;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use crate::domain::order::Fill;

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
                    "ORDER_TRADE_UPDATE: orderId={} DB에 없음",
                    event.exchange_order_id
                );
                return;
            }
            Err(e) => {
                error!("ORDER_TRADE_UPDATE DB 조회 실패: {e}");
                return;
            }
        };

        if let Some(fd) = event.fill {
            let fill = Fill {
                order_id: order.id,
                symbol: order.symbol.clone(),
                side: fd.side,
                qty: fd.qty,
                price: fd.price,
                fee: fd.fee,
                fee_asset: fd.fee_asset,
                filled_at: fd.filled_at,
            };

            if let Err(e) = self.repo.record_fill(&fill).await {
                error!("Fill PG 저장 실패: {e}");
            }
            self.fill_sink.emit(fill).await;
            info!(
                "Fill 수신: symbol={}, side={:?}, qty={}, price={}",
                order.symbol, fd.side, fd.qty, fd.price
            );
        }

        if let Some(status) = event.new_status {
            let mut updated = order;
            updated.status = status;
            updated.filled_qty = event.cumulative_filled_qty;
            updated.avg_fill_price = event.avg_fill_price;
            updated.updated_at = Utc::now();
            if let Err(e) = self.repo.upsert_order(&updated).await {
                error!("ORDER_TRADE_UPDATE 주문 갱신 실패: {e}");
            }
        }
    }
}

#[async_trait::async_trait]
impl FillSink for Sender<Fill> {
    async fn emit(&self, fill: Fill) {
        let _ = self.send(fill).await;
    }
}
