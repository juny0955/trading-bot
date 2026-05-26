use crate::adapters::outbound::cache::book_ticker_cache::BookTickerCache;
use crate::application::account_state::AccountState;
use crate::application::decision_logger::DecisionLogger;
use crate::application::limit_order_monitor::LimitOrderMonitor;
use crate::application::position_lifecycle_tracker::PositionLifecycleTracker;
use crate::application::trade_manager::{PositionKey, TradeManager, TradeManagerParams};
use crate::domain::backtest::{Bar, DepthSnapshot};
use crate::domain::event::MarketDataEvent;
use crate::domain::lifecycle::{
    DecisionMarketContext, DecisionPositionContext, LifecycleSide, ManagementCommand,
    PositionRuntimeState, PositionSnapshot,
};
use crate::domain::market_data::{BookTicker, Depth, Kline};
use crate::domain::order::{OrderRequest, OrderSide, OrderType};
use crate::domain::position::Position;
use crate::domain::strategies::{Context, Strategy, StrategyCommand};
use crate::ports::market_data_provider::MarketDataProvider;
use crate::ports::order_service::OrderService;
use crate::ports::position_repository::PositionRepository;
use crate::ports::trading_use_case::TradingUseCase;
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

pub struct LiveTrader {
    order_svc: Arc<dyn OrderService>,
    position_repo: Arc<dyn PositionRepository>,
    book_ticker_cache: Arc<BookTickerCache>,
    account_state: Arc<AccountState>,
    trade_manager: Arc<TradeManager>,
    lifecycle_tracker: Arc<PositionLifecycleTracker>,
    decision_logger: Arc<DecisionLogger>,
    limit_monitor: Arc<LimitOrderMonitor>,
    tm_params: TradeManagerParams,
}

impl LiveTrader {
    pub fn new(
        order_svc: Arc<dyn OrderService>,
        position_repo: Arc<dyn PositionRepository>,
        book_ticker_cache: Arc<BookTickerCache>,
        account_state: Arc<AccountState>,
        trade_manager: Arc<TradeManager>,
        lifecycle_tracker: Arc<PositionLifecycleTracker>,
        decision_logger: Arc<DecisionLogger>,
        limit_monitor: Arc<LimitOrderMonitor>,
        tm_params: TradeManagerParams,
    ) -> Self {
        Self {
            order_svc,
            position_repo,
            book_ticker_cache,
            account_state,
            trade_manager,
            lifecycle_tracker,
            decision_logger,
            limit_monitor,
            tm_params,
        }
    }
}

#[async_trait]
impl TradingUseCase for LiveTrader {
    async fn run(
        self: Box<Self>,
        feed: Box<dyn MarketDataProvider>,
        mut strategies: HashMap<(i64, String), Box<dyn Strategy>>,
        token: CancellationToken,
    ) -> Result<()> {
        let (market_tx, mut market_rx) = mpsc::channel::<MarketDataEvent>(256);
        spawn_market_feed(feed, market_tx, token.clone());

        let equity = self.account_state.available_balance();
        self.start_strategies(&mut strategies, equity, None).await?;

        let mut fear_greed: Option<u8> = None;

        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                event = market_rx.recv() => match event {
                    Some(e) => {
                        if let MarketDataEvent::BookTicker(ref bt) = e {
                            self.book_ticker_cache.update(bt.clone());
                            // Collect matching keys before await to avoid holding &strategies
                            // across an await point (dyn Strategy is Send but not Sync).
                            let matching: Vec<PositionKey> = strategies
                                .keys()
                                .filter(|(_, sym)| sym == &bt.symbol)
                                .cloned()
                                .collect();
                            self.process_book_ticker(bt, matching).await;
                        }
                        if let MarketDataEvent::FearGreed(ref fg) = e {
                            fear_greed = fg.value.parse::<u8>().ok();
                        }
                        let equity = self.account_state.available_balance();
                        self.process_event(&mut strategies, &e, equity, fear_greed).await?;
                    }
                    None => {
                        error!("market feed 채널 닫힘");
                        break;
                    }
                }
            }
        }

        let equity = self.account_state.available_balance();
        self.finish_strategies(&mut strategies, equity, fear_greed)
            .await?;
        Ok(())
    }
}

fn spawn_market_feed(
    feed: Box<dyn MarketDataProvider>,
    market_tx: mpsc::Sender<MarketDataEvent>,
    token: CancellationToken,
) {
    tokio::spawn(async move {
        feed.run(market_tx, token).await;
    });
}

impl LiveTrader {
    async fn start_strategies(
        &self,
        strategies: &mut HashMap<(i64, String), Box<dyn Strategy>>,
        equity: Decimal,
        fear_greed: Option<u8>,
    ) -> Result<()> {
        for (&(instance_id, ref symbol), strategy) in strategies.iter_mut() {
            let ctx = self.make_ctx(instance_id, symbol, equity, fear_greed).await;
            let cmds = strategy.on_start(&ctx)?;
            self.dispatch(instance_id, symbol, cmds).await;
        }
        Ok(())
    }

    async fn process_event(
        &self,
        strategies: &mut HashMap<(i64, String), Box<dyn Strategy>>,
        event: &MarketDataEvent,
        equity: Decimal,
        fear_greed: Option<u8>,
    ) -> Result<()> {
        let symbol = event_symbol(event);
        for (&(instance_id, ref entry_symbol), strategy) in strategies.iter_mut() {
            if entry_symbol != &symbol {
                continue;
            }
            let ctx = self
                .make_ctx(instance_id, entry_symbol, equity, fear_greed)
                .await;
            let cmds = on_event(&mut **strategy, event, &ctx)?;
            self.dispatch(instance_id, entry_symbol, cmds).await;
        }
        Ok(())
    }

    async fn finish_strategies(
        &self,
        strategies: &mut HashMap<(i64, String), Box<dyn Strategy>>,
        equity: Decimal,
        fear_greed: Option<u8>,
    ) -> Result<()> {
        for (&(instance_id, ref symbol), strategy) in strategies.iter_mut() {
            let ctx = self.make_ctx(instance_id, symbol, equity, fear_greed).await;
            let cmds = strategy.on_finish(&ctx)?;
            info!(cmds = cmds.len(), "전략 on_finish");
            self.dispatch(instance_id, symbol, cmds).await;
        }
        Ok(())
    }

    // ── 전략 명령 실행 ────────────────────────────────────────────────────────

    async fn dispatch(&self, instance_id: i64, symbol: &str, cmds: Vec<StrategyCommand>) {
        for cmd in cmds {
            match cmd {
                StrategyCommand::SubmitOrder(req) => {
                    self.dispatch_submit(instance_id, symbol, req).await;
                }
                StrategyCommand::SetStopLoss {
                    side,
                    qty,
                    stop_price,
                } => {
                    self.dispatch_sl(instance_id, symbol, side, qty, stop_price)
                        .await;
                }
                StrategyCommand::SetTakeProfit {
                    side,
                    qty,
                    stop_price,
                } => {
                    self.dispatch_tp(instance_id, symbol, side, qty, stop_price)
                        .await;
                }
            }
        }
    }

    async fn dispatch_submit(&self, instance_id: i64, symbol: &str, req: OrderRequest) {
        let is_opening = !req.reduce_only;
        let entry_side = req.side;
        let is_limit = req.order_type == OrderType::Limit;

        // 신규 진입 주문 시 lifecycle 열기
        let lifecycle_id: Option<Uuid> = if is_opening {
            match self
                .lifecycle_tracker
                .open_lifecycle(instance_id, symbol, lifecycle_side(entry_side), None)
                .await
            {
                Ok(id) => {
                    self.lifecycle_tracker
                        .set_active_lifecycle(instance_id, symbol, id)
                        .await;
                    Some(id)
                }
                Err(e) => {
                    error!(error = %e, instance_id, symbol, "lifecycle open 실패");
                    None
                }
            }
        } else {
            self.lifecycle_tracker
                .get_active_lifecycle(instance_id, symbol)
                .await
        };

        let req = prepare_order_request(req, instance_id, symbol);
        let side = req.side;
        let qty = req.qty;
        let market_replace = to_market_order(req.clone());
        match self.order_svc.submit(req).await {
            Ok(order) => {
                info!(id = %order.id, instance_id, symbol, ?side, %qty, "주문 제출");
                if let Some(lid) = lifecycle_id {
                    self.lifecycle_tracker.register_order(order.id, lid).await;
                }
                if is_opening {
                    let key: PositionKey = (instance_id, symbol.to_string());
                    self.trade_manager
                        .init_position(key.clone(), PositionRuntimeState::default(), None)
                        .await;
                    if is_limit {
                        self.limit_monitor
                            .register(
                                order.id,
                                self.tm_params.limit_timeout_secs,
                                market_replace,
                                lifecycle_id,
                                key,
                            )
                            .await;
                    }
                }
            }
            Err(e) => error!(error = %e, instance_id, symbol, ?side, %qty, "주문 실패"),
        }
    }

    async fn dispatch_sl(
        &self,
        instance_id: i64,
        symbol: &str,
        side: OrderSide,
        qty: Decimal,
        stop_price: Decimal,
    ) {
        let active_lid = self
            .lifecycle_tracker
            .get_active_lifecycle(instance_id, symbol)
            .await;
        let req = prepare_order_request(
            OrderRequest {
                order_type: OrderType::StopMarket,
                side,
                qty,
                stop_price: Some(stop_price),
                reduce_only: true,
                ..Default::default()
            },
            instance_id,
            symbol,
        );
        let order_side = req.side;
        let order_qty = req.qty;
        match self.order_svc.submit(req).await {
            Ok(order) => {
                info!(id = %order.id, instance_id, symbol, ?order_side, %order_qty, "SL 주문 제출");
                if let Some(lid) = active_lid {
                    self.lifecycle_tracker.register_order(order.id, lid).await;
                }
                let key: PositionKey = (instance_id, symbol.to_string());
                self.trade_manager.set_sl_order(&key, Some(order.id)).await;
            }
            Err(e) => error!(error = %e, instance_id, symbol, "SL 주문 실패"),
        }
    }

    async fn dispatch_tp(
        &self,
        instance_id: i64,
        symbol: &str,
        side: OrderSide,
        qty: Decimal,
        stop_price: Decimal,
    ) {
        let active_lid = self
            .lifecycle_tracker
            .get_active_lifecycle(instance_id, symbol)
            .await;
        let req = prepare_order_request(
            OrderRequest {
                order_type: OrderType::StopMarket,
                side,
                qty,
                stop_price: Some(stop_price),
                reduce_only: true,
                ..Default::default()
            },
            instance_id,
            symbol,
        );
        let order_side = req.side;
        let order_qty = req.qty;
        match self.order_svc.submit(req).await {
            Ok(order) => {
                info!(id = %order.id, instance_id, symbol, ?order_side, %order_qty, "TP 주문 제출");
                if let Some(lid) = active_lid {
                    self.lifecycle_tracker.register_order(order.id, lid).await;
                }
            }
            Err(e) => error!(error = %e, instance_id, symbol, "TP 주문 실패"),
        }
    }

    // ── BookTicker → TradeManager ─────────────────────────────────────────────

    async fn process_book_ticker(&self, bt: &BookTicker, keys: Vec<PositionKey>) {
        let mid = (bt.bid_price + bt.ask_price) / dec!(2);
        if mid.is_zero() {
            return;
        }

        // 지정가 타임아웃 주문 처리
        self.limit_monitor
            .check_timeouts(
                &self.order_svc,
                &self.lifecycle_tracker,
                &self.trade_manager,
            )
            .await;

        for key in keys {
            let instance_id = key.0;
            let symbol = &key.1;

            // TradeManager에 등록된 포지션만 평가
            let Some(runtime) = self.trade_manager.get_state(&key).await else {
                continue;
            };

            let position = match self.position_repo.get(instance_id, symbol).await {
                Ok(Some(sp)) => sp.position,
                Ok(None) => continue,
                Err(e) => {
                    warn!(error = %e, instance_id, symbol, "position 조회 실패");
                    continue;
                }
            };

            if position.qty.is_zero() || position.side.is_none() {
                self.trade_manager.remove_position(&key).await;
                continue;
            }

            let entry_atr = self
                .trade_manager
                .get_entry_atr(&key)
                .await
                .unwrap_or(Decimal::ZERO);
            let is_long = position.side == Some(OrderSide::Buy);

            // high_water_mark 메모리 갱신 (롱: 최고가 추적)
            if is_long {
                self.trade_manager.update_hwm(&key, mid).await;
            }

            let snapshot = PositionSnapshot {
                current_price: mid,
                unrealized_pnl: position.unrealized(mid),
                atr: Decimal::ZERO, // Phase A: 전략에서 ATR 미공개
                ema200: Decimal::ZERO,
                adx: Decimal::ZERO,
                entry_price: position.avg_entry_price,
                entry_atr,
                qty: position.qty,
                is_long,
            };

            let Some(cmds) = self
                .trade_manager
                .tick(key.clone(), &snapshot, &self.tm_params)
                .await
            else {
                continue;
            };

            if cmds.is_empty() {
                continue;
            }

            let lifecycle_id = self
                .lifecycle_tracker
                .get_active_lifecycle(instance_id, symbol)
                .await;

            for cmd in cmds {
                let decision_id = if let Some(lid) = lifecycle_id {
                    let market_ctx = build_market_ctx(bt, entry_atr, mid);
                    let pos_ctx = build_position_ctx(&position, &runtime, mid, entry_atr);
                    match self
                        .decision_logger
                        .record(lid, symbol, &cmd, &market_ctx, &pos_ctx)
                        .await
                    {
                        Ok(id) => id,
                        Err(e) => {
                            warn!(error = %e, symbol, "decision 기록 실패");
                            None
                        }
                    }
                } else {
                    None
                };

                self.execute_management_cmd(&key, &position, cmd, decision_id)
                    .await;
            }
        }
    }

    async fn execute_management_cmd(
        &self,
        key: &PositionKey,
        position: &Position,
        cmd: ManagementCommand,
        decision_id: Option<Uuid>,
    ) {
        let (instance_id, symbol) = key;
        let t0 = std::time::Instant::now();

        match cmd {
            ManagementCommand::PartialClose { qty, tp_stage, .. } => {
                let close_side = opposite_side(position.side);
                let req = prepare_order_request(
                    OrderRequest {
                        order_type: OrderType::Market,
                        side: close_side,
                        qty,
                        reduce_only: true,
                        ..Default::default()
                    },
                    *instance_id,
                    symbol,
                );
                match self.order_svc.submit(req).await {
                    Ok(order) => {
                        info!(id = %order.id, symbol, %qty, tp_stage, "부분 익절 주문");
                        self.trade_manager.set_pending_reduce(key, order.id).await;
                        if let Some(lid) = self
                            .lifecycle_tracker
                            .get_active_lifecycle(*instance_id, symbol)
                            .await
                        {
                            self.lifecycle_tracker.register_order(order.id, lid).await;
                        }
                        ack_decision(&self.decision_logger, decision_id, t0).await;
                    }
                    Err(e) => {
                        error!(error = %e, symbol, "부분 익절 실패");
                        reject_decision(&self.decision_logger, decision_id, &e.to_string()).await;
                    }
                }
            }

            ManagementCommand::ReplaceSL {
                cancel_order_id,
                desired_price,
                ..
            } => {
                self.trade_manager
                    .on_replace_sl_issued(key, desired_price)
                    .await;
                match self.order_svc.cancel(cancel_order_id).await {
                    Ok(_) => {
                        self.trade_manager.on_sl_cancelled(key).await;
                        let sl_side = opposite_side(position.side);
                        let req = prepare_order_request(
                            OrderRequest {
                                order_type: OrderType::StopMarket,
                                side: sl_side,
                                qty: position.qty,
                                stop_price: Some(desired_price),
                                reduce_only: true,
                                ..Default::default()
                            },
                            *instance_id,
                            symbol,
                        );
                        match self.order_svc.submit(req).await {
                            Ok(order) => {
                                info!(id = %order.id, symbol, %desired_price, "SL 교체 완료");
                                self.trade_manager.set_sl_order(key, Some(order.id)).await;
                                if let Some(lid) = self
                                    .lifecycle_tracker
                                    .get_active_lifecycle(*instance_id, symbol)
                                    .await
                                {
                                    self.lifecycle_tracker.register_order(order.id, lid).await;
                                }
                                ack_decision(&self.decision_logger, decision_id, t0).await;
                            }
                            Err(e) => {
                                error!(error = %e, symbol, "새 SL 주문 실패");
                                self.trade_manager.increment_sl_retry(key).await;
                                reject_decision(&self.decision_logger, decision_id, &e.to_string())
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, symbol, "SL cancel 실패");
                        self.trade_manager.on_replace_sl_cancel_failed(key).await;
                        reject_decision(&self.decision_logger, decision_id, &e.to_string()).await;
                    }
                }
            }

            ManagementCommand::EarlyExit { ref reason, .. } => {
                warn!(symbol, %reason, "조기 손절");
                self.force_close_position(key, position, decision_id, t0)
                    .await;
            }

            ManagementCommand::EmergencyClose { ref reason, .. } => {
                warn!(symbol, %reason, "긴급 청산");
                self.force_close_position(key, position, decision_id, t0)
                    .await;
            }
        }
    }

    async fn force_close_position(
        &self,
        key: &PositionKey,
        position: &Position,
        decision_id: Option<Uuid>,
        t0: std::time::Instant,
    ) {
        let (instance_id, symbol) = key;
        let close_side = opposite_side(position.side);
        let req = prepare_order_request(
            OrderRequest {
                order_type: OrderType::Market,
                side: close_side,
                qty: position.qty,
                reduce_only: true,
                ..Default::default()
            },
            *instance_id,
            symbol,
        );
        match self.order_svc.submit(req).await {
            Ok(order) => {
                info!(id = %order.id, symbol, "포지션 강제 청산 주문");
                if let Some(lid) = self
                    .lifecycle_tracker
                    .get_active_lifecycle(*instance_id, symbol)
                    .await
                {
                    self.lifecycle_tracker.register_order(order.id, lid).await;
                }
                ack_decision(&self.decision_logger, decision_id, t0).await;
            }
            Err(e) => {
                error!(error = %e, symbol, "강제 청산 실패");
                reject_decision(&self.decision_logger, decision_id, &e.to_string()).await;
            }
        }
    }

    // ── 컨텍스트 빌더 ────────────────────────────────────────────────────────

    async fn make_ctx(
        &self,
        instance_id: i64,
        symbol: &str,
        equity: Decimal,
        fear_greed: Option<u8>,
    ) -> Context {
        let position = match self.position_repo.get(instance_id, symbol).await {
            Ok(Some(sp)) => sp.position,
            Ok(None) => Position::default(),
            Err(e) => {
                warn!(error = %e, instance_id, symbol, "position 조회 실패 - flat fallback");
                Position::default()
            }
        };
        Context {
            now_ns: Utc::now().timestamp_nanos_opt().unwrap_or(0),
            equity,
            position,
            fear_greed,
        }
    }
}

// ── 헬퍼 함수 ─────────────────────────────────────────────────────────────────

fn event_symbol(e: &MarketDataEvent) -> String {
    match e {
        MarketDataEvent::Kline(k) => k.symbol.clone(),
        MarketDataEvent::Depth(d) => d.symbol.clone(),
        _ => "".to_string(),
    }
}

fn on_event(
    strategy: &mut dyn Strategy,
    event: &MarketDataEvent,
    ctx: &Context,
) -> Result<Vec<StrategyCommand>> {
    match event {
        MarketDataEvent::Kline(k) if k.is_closed => strategy.on_bar(&kline_to_bar(k), ctx),
        MarketDataEvent::Depth(d) => match depth_to_snapshot(d) {
            Some(s) => strategy.on_depth(&s, ctx),
            None => Ok(vec![]),
        },
        _ => Ok(vec![]),
    }
}

fn prepare_order_request(mut req: OrderRequest, instance_id: i64, symbol: &str) -> OrderRequest {
    req.strategy_instance_id = instance_id;
    if req.symbol.is_empty() {
        req.symbol = symbol.to_string();
    }
    req
}

fn to_market_order(mut req: OrderRequest) -> OrderRequest {
    req.order_type = OrderType::Market;
    req.price = None;
    req
}

fn lifecycle_side(side: OrderSide) -> LifecycleSide {
    match side {
        OrderSide::Buy => LifecycleSide::Long,
        OrderSide::Sell => LifecycleSide::Short,
    }
}

fn opposite_side(side: Option<OrderSide>) -> OrderSide {
    match side {
        Some(OrderSide::Buy) => OrderSide::Sell,
        _ => OrderSide::Buy,
    }
}

fn build_market_ctx(bt: &BookTicker, entry_atr: Decimal, mid: Decimal) -> DecisionMarketContext {
    let spread_pct = if !mid.is_zero() {
        Some((bt.ask_price - bt.bid_price) / mid * dec!(100))
    } else {
        None
    };
    let atr_pct = if !mid.is_zero() && !entry_atr.is_zero() {
        Some(entry_atr / mid * dec!(100))
    } else {
        None
    };
    DecisionMarketContext {
        atr: if !entry_atr.is_zero() {
            Some(entry_atr)
        } else {
            None
        },
        atr_pct,
        adx: None,
        spread_pct,
        fng_value: None,
        ema_alignment: None,
        ob_imbalance: None,
    }
}

fn build_position_ctx(
    position: &Position,
    runtime: &PositionRuntimeState,
    mid: Decimal,
    entry_atr: Decimal,
) -> DecisionPositionContext {
    let unrealized = position.unrealized(mid);
    let cost_basis = position.avg_entry_price * position.qty;
    let unrealized_pnl_pct = if !cost_basis.is_zero() {
        Some(unrealized / cost_basis * dec!(100))
    } else {
        None
    };
    let dist_to_sl_atr = if !entry_atr.is_zero() {
        runtime
            .desired_sl_price
            .map(|sl| ((mid - sl) / entry_atr).abs())
    } else {
        None
    };
    DecisionPositionContext {
        unrealized_pnl_pct,
        time_in_trade_secs: None,
        dist_to_sl_atr,
        dist_to_tp_atr: None,
        tp_stage: Some(runtime.tp_stage as i16),
    }
}

async fn ack_decision(
    logger: &Arc<DecisionLogger>,
    decision_id: Option<Uuid>,
    t0: std::time::Instant,
) {
    if let Some(id) = decision_id {
        let latency_ms = t0.elapsed().as_millis() as i32;
        if let Err(e) = logger.ack(id, latency_ms).await {
            warn!(error = %e, "decision ACK 기록 실패");
        }
    }
}

async fn reject_decision(logger: &Arc<DecisionLogger>, decision_id: Option<Uuid>, error: &str) {
    if let Some(id) = decision_id
        && let Err(e) = logger.reject(id, error).await
    {
        warn!(error = %e, "decision REJECT 기록 실패");
    }
}

fn kline_to_bar(k: &Kline) -> Bar {
    Bar {
        symbol: k.symbol.clone(),
        ts_ns: k.open_time_ms * 1_000_000,
        open: k.open,
        high: k.high,
        low: k.low,
        close: k.close,
        volume: k.volume,
    }
}

fn depth_to_snapshot(d: &Depth) -> Option<DepthSnapshot> {
    let bid = d.bids.first()?;
    let ask = d.asks.first()?;

    Some(DepthSnapshot {
        symbol: d.symbol.clone(),
        ts_ns: d.event_time_ns,
        bid1_price: bid.price,
        bid1_qty: bid.quantity,
        ask1_price: ask.price,
        ask1_qty: ask.quantity,
    })
}
