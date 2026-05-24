use crate::domain::backtest::{Bar, DepthSnapshot};
use crate::domain::event::MarketDataEvent;
use crate::domain::market_data::{Depth, Kline};
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
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// 실시간 시장 이벤트를 전략에 전달하고 전략 명령을 주문 서비스로 실행합니다.
///
/// # Examples
///
/// ```ignore
/// # use std::sync::Arc;
/// # use trading_bot::application::live_trader::LiveTrader;
/// # use trading_bot::ports::order_service::OrderService;
/// # use trading_bot::ports::position_repository::PositionRepository;
/// # fn example(order_svc: Arc<dyn OrderService>, position_repo: Arc<dyn PositionRepository>) {
/// let trader = LiveTrader::new(order_svc, position_repo);
/// # }
/// ```
pub struct LiveTrader {
    order_svc: Arc<dyn OrderService>,
    position_repo: Arc<dyn PositionRepository>,
}

impl LiveTrader {
    /// 새 실시간 트레이더 유스케이스를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use std::sync::Arc;
    /// # use trading_bot::application::live_trader::LiveTrader;
    /// # use trading_bot::ports::order_service::OrderService;
    /// # use trading_bot::ports::position_repository::PositionRepository;
    /// # fn example(order_svc: Arc<dyn OrderService>, position_repo: Arc<dyn PositionRepository>) {
    /// let trader = LiveTrader::new(order_svc, position_repo);
    /// # }
    /// ```
    pub fn new(
        order_svc: Arc<dyn OrderService>,
        position_repo: Arc<dyn PositionRepository>,
    ) -> Self {
        Self {
            order_svc,
            position_repo,
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
        start_strategies(&self.position_repo, &self.order_svc, &mut strategies).await?;

        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                event = market_rx.recv() => match event {
                    Some(e) => {
                        process_market_event(&self.position_repo, &self.order_svc, &mut strategies, &e).await?;
                    }
                    None => {
                        error!("market feed 채널 닫힘");
                        break;
                    }
                }
            }
        }

        finish_strategies(&self.position_repo, &self.order_svc, &mut strategies).await?;
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

async fn start_strategies(
    position_repo: &Arc<dyn PositionRepository>,
    order_svc: &Arc<dyn OrderService>,
    strategies: &mut HashMap<(i64, String), Box<dyn Strategy>>,
) -> Result<()> {
    for (&(instance_id, ref symbol), strategy) in strategies.iter_mut() {
        let ctx = make_ctx(position_repo, instance_id, symbol, now_ns()).await;
        let cmds = strategy.on_start(&ctx)?;
        dispatch(order_svc, instance_id, symbol, cmds).await;
    }
    Ok(())
}

async fn process_market_event(
    position_repo: &Arc<dyn PositionRepository>,
    order_svc: &Arc<dyn OrderService>,
    strategies: &mut HashMap<(i64, String), Box<dyn Strategy>>,
    event: &MarketDataEvent,
) -> Result<()> {
    let symbol = event_symbol(event);
    for (&(instance_id, ref entry_symbol), strategy) in strategies.iter_mut() {
        if entry_symbol != &symbol {
            continue;
        }
        let ctx = make_ctx(position_repo, instance_id, entry_symbol, now_ns()).await;
        let cmds = on_event(&mut **strategy, event, &ctx)?;
        dispatch(order_svc, instance_id, entry_symbol, cmds).await;
    }
    Ok(())
}

async fn finish_strategies(
    position_repo: &Arc<dyn PositionRepository>,
    order_svc: &Arc<dyn OrderService>,
    strategies: &mut HashMap<(i64, String), Box<dyn Strategy>>,
) -> Result<()> {
    for (&(instance_id, ref symbol), strategy) in strategies.iter_mut() {
        let ctx = make_ctx(position_repo, instance_id, symbol, now_ns()).await;
        let cmds = strategy.on_finish(&ctx)?;
        info!(cmds = cmds.len(), "전략 on_finish");
        dispatch(order_svc, instance_id, symbol, cmds).await;
    }
    Ok(())
}

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

async fn dispatch(
    svc: &Arc<dyn OrderService>,
    instance_id: i64,
    symbol: &str,
    cmds: Vec<StrategyCommand>,
) {
    for cmd in cmds {
        match cmd {
            StrategyCommand::SubmitOrder(req) => {
                submit_order_command(svc, instance_id, symbol, req).await
            }
        }
    }
}

async fn submit_order_command(
    svc: &Arc<dyn OrderService>,
    instance_id: i64,
    symbol: &str,
    req: crate::domain::order::OrderRequest,
) {
    let req = prepare_order_request(req, instance_id, symbol);
    let side = req.side;
    let qty = req.qty;
    match svc.submit(req).await {
        Ok(order) => {
            info!(id = %order.id, instance_id, symbol, ?side, %qty, "주문 제출")
        }
        Err(e) => error!(error = %e, instance_id, symbol, ?side, %qty, "주문 실패"),
    }
}

fn prepare_order_request(
    mut req: crate::domain::order::OrderRequest,
    instance_id: i64,
    symbol: &str,
) -> crate::domain::order::OrderRequest {
    req.strategy_instance_id = instance_id;
    if req.symbol.is_empty() {
        req.symbol = symbol.to_string();
    }
    req
}

async fn make_ctx(
    repo: &Arc<dyn PositionRepository>,
    instance_id: i64,
    symbol: &str,
    now_ns: i64,
) -> Context {
    let position = match repo.get(instance_id, symbol).await {
        Ok(Some(sp)) => sp.position,
        Ok(None) => Position::default(),
        Err(e) => {
            warn!(error = %e, instance_id, symbol, "position 조회 실패 - flat fallback");
            Position::default()
        }
    };

    Context {
        now_ns,
        equity: Decimal::ZERO,
        position,
    }
}

fn now_ns() -> i64 {
    Utc::now().timestamp_nanos_opt().unwrap_or(0)
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
