use crate::domain::account::AccountSnapshot;
use crate::domain::order::{ExchangeOpenOrder, Order, OrderStatus};
use crate::ports::outbound::account_source::AccountSource;
use crate::ports::outbound::order_executor::OrderExecutor;
use crate::ports::outbound::order_repository::OrderRepository;
use std::future::Future;
use tracing::{info, warn};

#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("계정 동기화 실패: {0}")]
    AccountSync(String),
    #[error("주문 동기화 실패: {0}")]
    OrderSync(String),
}

/// 최대 `retries`회 지수 백오프로 재시도하는 비동기 헬퍼.
async fn retry<F, Fut, T, E>(retries: u32, mut f: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut attempt = 0;
    loop {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                if attempt >= retries {
                    return Err(e);
                }
                attempt += 1;
                tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await;
            }
        }
    }
}

/// 계정 스냅샷 조회 + 오픈 주문 정합 + PendingSubmit 재조정을 수행한다.
pub async fn bootstrap(
    account_src: &dyn AccountSource,
    executor: &dyn OrderExecutor,
    order_repo: &dyn OrderRepository,
    symbols: &[String],
) -> Result<AccountSnapshot, BootstrapError> {
    let snap = retry(3, || account_src.fetch_account())
        .await
        .map_err(|e| BootstrapError::AccountSync(e.to_string()))?;
    info!(
        available_balance = %snap.available_balance,
        wallet_balance = %snap.total_wallet_balance,
        "계정 스냅샷 조회 완료"
    );

    for symbol in symbols {
        if let Err(e) = reconcile_open_orders(executor, order_repo, symbol).await {
            warn!(symbol, error = %e, "오픈 주문 정합 실패 — 계속 진행");
        }
    }

    resolve_pending_submits(executor, order_repo).await;

    info!("부팅 동기화 완료");
    Ok(snap)
}

async fn reconcile_open_orders(
    executor: &dyn OrderExecutor,
    repo: &dyn OrderRepository,
    symbol: &str,
) -> Result<(), BootstrapError> {
    let ex_open = executor
        .open_orders(symbol)
        .await
        .map_err(|e| BootstrapError::OrderSync(e.to_string()))?;
    let db_open = repo
        .find_open(symbol)
        .await
        .map_err(|e| BootstrapError::OrderSync(e.to_string()))?;

    for db_order in &db_open {
        if !is_order_on_exchange(db_order, &ex_open) && let Some(eid) = db_order.exchange_order_id {
            match executor.query(db_order).await {
                Ok(remote) => {
                    let _ = repo.upsert_order(&remote).await;
                    info!(
                        exchange_order_id = eid,
                        new_status = ?remote.status,
                        "부팅 정합: DB open → 거래소 최종 상태로 갱신"
                    );
                }
                Err(e) => {
                    warn!(exchange_order_id = eid, error = %e, "부팅 정합: query 실패");
                }
            }
        }
    }
    Ok(())
}

async fn resolve_pending_submits(executor: &dyn OrderExecutor, repo: &dyn OrderRepository) {
    if let Ok(pendings) = repo.find_by_status(OrderStatus::PendingSubmit).await {
        for pending in pendings {
            match executor
                .query_by_client_id(&pending.symbol, &pending.client_order_id)
                .await
            {
                Ok(remote) => {
                    let updated = Order {
                        exchange_order_id: remote.exchange_order_id,
                        status: remote.status,
                        filled_qty: remote.filled_qty,
                        avg_fill_price: remote.avg_fill_price,
                        ..pending.clone()
                    };
                    let _ = repo.upsert_order(&updated).await;
                    info!(client_order_id=%pending.client_order_id, new_status=?updated.status, "부팅 정합: PendingSubmit → 거래소 상태로 확정");
                }
                Err(crate::domain::order::OrderError::NotFound(_)) => {
                    let rejected = Order { status: OrderStatus::Rejected, ..pending.clone() };
                    let _ = repo.upsert_order(&rejected).await;
                    info!(client_order_id=%pending.client_order_id, "부팅 정합: PendingSubmit → Rejected 확정 (NotFound)");
                }
                Err(e) => {
                    warn!(client_order_id=%pending.client_order_id, error=%e, "부팅 정합: PendingSubmit 상태 조회 실패 (무시)");
                }
            }
        }
    }
}

fn is_order_on_exchange(db_order: &Order, ex_open: &[ExchangeOpenOrder]) -> bool {
    ex_open.iter().any(|ex| Some(ex.exchange_order_id) == db_order.exchange_order_id)
}
