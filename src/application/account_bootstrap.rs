use crate::config::binance_config::SymbolConfig;
use crate::domain::account::AccountSnapshot;
use crate::domain::order::{ExchangeOpenOrder, Order, OrderStatus};
use crate::domain::strategies::StrategyInstance;
use crate::ports::outbound::account_source::AccountSource;
use crate::ports::outbound::order_executor::OrderExecutor;
use crate::ports::outbound::order_repository::OrderRepository;
use crate::ports::outbound::position_repository::PositionRepository;
use tracing::{info, warn};

#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("계정 동기화 실패: {0}")]
    AccountSync(String),
    #[error("주문 동기화 실패: {0}")]
    OrderSync(String),
}

/// 계정 스냅샷 조회 + 오픈 주문 정합 + PendingSubmit 재조정 + 포지션 동기화 + 레버리지/마진 설정을 수행한다.
pub async fn bootstrap(
    account_src: &dyn AccountSource,
    executor: &dyn OrderExecutor,
    order_repo: &dyn OrderRepository,
    position_repo: &dyn PositionRepository,
    instances: &[StrategyInstance],
    symbols: &[String],
    symbol_configs: &[SymbolConfig],
) -> Result<AccountSnapshot, BootstrapError> {
    // 1단계: 계정 잔고 스냅샷 조회. 실패 시 부팅을 중단한다.
    let snap = account_src
        .fetch_account()
        .await
        .map_err(|e| BootstrapError::AccountSync(e.to_string()))?;
    info!(
        available_balance = %snap.available_balance,
        wallet_balance = %snap.total_wallet_balance,
        "계정 스냅샷 조회 완료"
    );

    // 2단계: 심볼별 레버리지·마진 타입 설정 (설정 실패는 경고만 남기고 계속 진행).
    sync_leverage_margin(account_src, symbol_configs).await;

    // 3단계: DB 오픈 주문과 거래소 상태 정합. 심볼 단위 실패는 soft error로 처리한다.
    for symbol in symbols {
        if let Err(e) = reconcile_open_orders(executor, order_repo, symbol).await {
            warn!(symbol, error = %e, "오픈 주문 정합 실패 — 계속 진행");
        }
    }

    // 4단계: PendingSubmit 주문을 거래소에서 조회해 최종 상태(체결·거부)로 확정한다.
    resolve_pending_submits(executor, order_repo).await;

    // 5단계: 거래소 포지션을 DB에 덮어써 부팅 시점 상태를 일치시킨다.
    sync_positions(account_src, position_repo, instances).await;

    info!("부팅 동기화 완료");
    Ok(snap)
}

async fn sync_leverage_margin(account_src: &dyn AccountSource, symbol_configs: &[SymbolConfig]) {
    for cfg in symbol_configs.iter().filter(|c| c.enabled) {
        match account_src
            .configure_symbol(&cfg.symbol, cfg.leverage, &cfg.margin_type)
            .await
        {
            Ok(()) => info!(
                symbol = %cfg.symbol,
                leverage = cfg.leverage,
                margin_type = %cfg.margin_type,
                "레버리지/마진 모드 설정 완료"
            ),
            Err(e) => warn!(
                symbol = %cfg.symbol,
                error = %e,
                "레버리지/마진 모드 설정 실패 — 계속 진행"
            ),
        }
    }
}

async fn sync_positions(
    account_src: &dyn AccountSource,
    position_repo: &dyn PositionRepository,
    instances: &[StrategyInstance],
) {
    let exchange_positions = match account_src.fetch_positions().await {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "거래소 포지션 조회 실패 — 포지션 동기화 건너뜀");
            return;
        }
    };
    for inst in instances {
        for symbol in &inst.symbols {
            if let Some(snap) = exchange_positions.iter().find(|p| p.symbol == *symbol) {
                if let Err(e) = position_repo.upsert_from_snapshot(inst.id, snap).await {
                    warn!(instance_id = inst.id, %symbol, error = %e, "포지션 동기화 실패");
                } else {
                    info!(instance_id = inst.id, %symbol, qty = %snap.qty, "거래소 포지션 동기화");
                }
            } else {
                // 거래소에 포지션이 없으면(fetch_positions가 flat을 제외) DB의 유령 포지션을
                // flat 처리한다. 그래야 재시작 후에도 reduceOnly -2022 루프가 재개되지 않는다.
                if let Err(e) = position_repo.mark_flat(inst.id, symbol).await {
                    warn!(instance_id = inst.id, %symbol, error = %e, "유령 포지션 flat 처리 실패");
                }
            }
        }
    }
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
        if !is_order_on_exchange(db_order, &ex_open)
            && let Some(eid) = db_order.exchange_order_id
        {
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
                    let rejected = Order {
                        status: OrderStatus::Rejected,
                        ..pending.clone()
                    };
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
    ex_open
        .iter()
        .any(|ex| Some(ex.exchange_order_id) == db_order.exchange_order_id)
}
